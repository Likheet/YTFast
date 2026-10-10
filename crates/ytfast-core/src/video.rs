//! Music videos for the video mode.
//!
//! YouTube serves a video's picture apart from its sound: H.264 in
//! fragmented MP4 (`ftyp`, `moov`, `sidx`, then `moof` and `mdat` pairs,
//! each pair starting on a key frame). [`Video`] reads it as it downloads,
//! decodes it on a thread of its own (rusty_h264, a pure-Rust H.264
//! decoder) a few pictures ahead of where the music is, and hands the
//! window each picture as RGBA when its time comes. The music is the clock:
//! the window says where it is ([`Video::at`]), and a jump in the music is
//! a jump in the video. The thread rests while nothing is shown
//! ([`Video::rest`]).

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crate::stream::{SongData, SongReader};

/// Pictures decoded ahead of the one shown (about a fifth of a second).
const AHEAD: usize = 6;
/// A music this much behind the next picture to decode, in seconds, has
/// jumped back; this much ahead, it has jumped on (or decoding fell
/// behind): the video goes to it instead of decoding its way there.
const JUMPED_BACK: f64 = 0.25;
const JUMPED_ON: f64 = 1.5;

/// One picture of the video.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Red, green, blue and opacity, a byte each, row after row.
    pub rgba: Vec<u8>,
    /// When it shows, in seconds from the video's start.
    pub at: f64,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("size", &(self.width, self.height))
            .field("at", &self.at)
            .finish()
    }
}

/// What the window shows at a moment.
#[derive(Debug, Default)]
pub struct Shown {
    /// The newest picture due, once there is one.
    pub picture: Option<Arc<Picture>>,
    /// When the next picture is due, in seconds from the video's start.
    pub next: Option<f64>,
}

/// A video being shown: its thread decodes while it is wanted, and stops
/// when this is dropped.
pub struct Video {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    /// Where the music is, as the window last said; `None` while nothing
    /// is shown.
    wanted: Option<f64>,
    /// Pictures decoded ahead, oldest first.
    ready: VecDeque<Arc<Picture>>,
    /// The picture last handed out.
    shown: Option<Arc<Picture>>,
    /// The memory of pictures no longer shown, for the next ones (a
    /// picture is 3.7 MB at 720p: asked of the system for each, it costs
    /// more than drawing it).
    spare: Vec<Vec<u8>>,
    failed: Option<String>,
    stop: bool,
}

/// How many pictures' memory is kept for the next ones.
const SPARE: usize = 2;

impl State {
    /// Keeps `picture`'s memory for a next one, once nothing else holds it.
    fn recycle(&mut self, picture: Option<Arc<Picture>>) {
        if let Some(picture) = picture.and_then(|p| Arc::try_unwrap(p).ok())
            && self.spare.len() < SPARE
        {
            self.spare.push(picture.rgba);
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Video {
    /// Starts decoding the video in `data` (still arriving), with H.264.
    pub fn start(data: Arc<SongData>) -> Self {
        Self::start_with(data, || Ok(Box::new(H264::new()) as Box<dyn Decode>))
    }

    fn start_with(
        data: Arc<SongData>,
        decoder: impl Fn() -> Result<Box<dyn Decode>, String> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let thread = Arc::clone(&shared);
        let started = std::thread::Builder::new()
            .name("ytfast-video".into())
            .spawn(move || {
                if let Err(e) = run(&thread, data, &decoder) {
                    log::warn!("video: {e}");
                    thread.lock().failed = Some(e);
                }
            });
        if let Err(e) = started {
            shared.lock().failed = Some(format!("the video could not start: {e}"));
        }
        Self { shared }
    }

    /// The picture to show with the music at `clock` seconds, and when the
    /// next one is due. Also tells the thread where the music is: call it
    /// each frame the video shows.
    pub fn at(&self, clock: f64) -> Shown {
        let mut state = self.shared.lock();
        state.wanted = Some(clock);
        while state.ready.front().is_some_and(|p| p.at <= clock) {
            let next = state.ready.pop_front();
            let old = std::mem::replace(&mut state.shown, next);
            state.recycle(old);
        }
        // After a jump back, the pictures kept are later than the music:
        // the thread brings earlier ones.
        if state
            .shown
            .as_ref()
            .is_some_and(|p| p.at > clock + JUMPED_BACK)
        {
            let old = state.shown.take();
            state.recycle(old);
        }
        let shown = Shown {
            picture: state.shown.clone(),
            next: state.ready.front().map(|p| p.at),
        };
        drop(state);
        self.shared.wake.notify_all();
        shown
    }

    /// Nothing is shown (the player page is closed): the thread rests
    /// until [`Video::at`] is called again.
    pub fn rest(&self) {
        self.shared.lock().wanted = None;
    }

    /// Why the video stopped, if it failed.
    pub fn failure(&self) -> Option<String> {
        self.shared.lock().failed.clone()
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
    }
}

/// Turns a video's samples into pictures: each sample (one picture of
/// H.264 in MP4, in the order they are decoded) gives its own picture at
/// once. B-frames make that order other than the order they show in; the
/// caller puts each in its place by the sample's time.
trait Decode: Send {
    /// Decodes one sample; its picture (RGBA, in `memory`, without its
    /// time, which the caller knows), if it gives one and it is `wanted`
    /// (a picture before where a jump lands is decoded, for those after
    /// it, but not kept).
    fn decode(
        &mut self,
        sample: Vec<u8>,
        wanted: bool,
        memory: Vec<u8>,
    ) -> Result<Option<Frame>, String>;
}

/// A decoded picture before its time is known.
struct Frame {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

/// The decoding thread: keeps [`AHEAD`] pictures ready after where the
/// music is, going to where it jumps, resting while nothing is shown.
fn run(
    shared: &Shared,
    data: Arc<SongData>,
    new_decoder: &dyn Fn() -> Result<Box<dyn Decode>, String>,
) -> Result<(), String> {
    let mut file = Mp4::open(data)?;
    let mut decoder = new_decoder()?;
    // The fragment being read: its number and its samples, and the next.
    let mut at: Option<(usize, Vec<Sample>, usize)> = None;
    // Pictures before this time (a jump's) are decoded but not kept.
    let mut keep_from = f64::NEG_INFINITY;
    // The latest time among the samples sent to the decoder.
    let mut decoded_to = f64::NEG_INFINITY;
    let mut ended = false;
    loop {
        // What to do: go to where the music is, or decode on.
        let next_time = at
            .as_ref()
            .and_then(|(_, samples, next)| samples.get(*next))
            .map(|s| s.at);
        // When the fragment after this one starts: a jump on lands there
        // or later (past the end there is none, and the last picture stays).
        let later = match &at {
            Some((fragment, ..)) => file.fragment_start(fragment + 1)?,
            None => None,
        };
        let (target, jump) = {
            let mut state = shared.lock();
            loop {
                if state.stop {
                    return Ok(());
                }
                if let Some(wanted) = state.wanted {
                    // The earliest picture this can still show.
                    let earliest = state
                        .ready
                        .front()
                        .map(|p| p.at)
                        .or(next_time)
                        .unwrap_or(decoded_to);
                    // Decoding is this far (after a jump, from where it
                    // jumped to, while it decodes its way there).
                    let reached = decoded_to.max(keep_from);
                    let jump = at.is_none()
                        || wanted < earliest - JUMPED_BACK
                        || (wanted > reached + JUMPED_ON && later.is_some_and(|l| wanted >= l));
                    if jump || (!ended && state.ready.len() < AHEAD) {
                        break (wanted, jump);
                    }
                }
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        if jump {
            let fragment = file.fragment_at(target)?;
            let samples = file.samples(fragment)?;
            // Past the end, the last picture.
            let last = samples.iter().map(|s| s.at).reduce(f64::max);
            keep_from = last.map_or(target, |last| (target - 0.1).min(last));
            decoded_to = f64::NEG_INFINITY;
            at = Some((fragment, samples, 0));
            decoder = new_decoder()?;
            let mut state = shared.lock();
            for picture in std::mem::take(&mut state.ready) {
                state.recycle(Some(picture));
            }
            drop(state);
            ended = false;
            continue;
        }
        let Some((fragment, samples, next)) = at.as_mut() else {
            continue;
        };
        let (time, frame) = if let Some(sample) = samples.get(*next) {
            let bytes = file.sample_bytes(sample, *next == 0)?;
            let time = sample.at;
            *next += 1;
            decoded_to = decoded_to.max(time);
            let memory = shared.lock().spare.pop().unwrap_or_default();
            (time, decoder.decode(bytes, time >= keep_from, memory)?)
        } else if *fragment + 1 < file.fragment_count()? {
            *fragment += 1;
            *samples = file.samples(*fragment)?;
            *next = 0;
            continue;
        } else {
            ended = true;
            continue;
        };
        let Some(frame) = frame else {
            continue;
        };
        let picture = Arc::new(Picture {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
            at: time,
        });
        // In its place by time: a B-frame shows before pictures decoded
        // ahead of it (the decoding keeps several pictures ahead, more than
        // YouTube's B-frames reach, so none is late for its place).
        let mut state = shared.lock();
        let place = state.ready.partition_point(|p| p.at <= time);
        state.ready.insert(place, picture);
    }
}

/// One sample: where its bytes are, and when it shows.
#[derive(Clone, Debug, PartialEq)]
struct Sample {
    offset: u64,
    size: u32,
    /// Seconds from the video's start.
    at: f64,
}

/// One fragment: a `moof` (and the `mdat` after it), and when it starts.
#[derive(Clone, Debug, PartialEq)]
struct Fragment {
    offset: u64,
    start: f64,
}

/// A fragmented MP4 as it downloads.
struct Mp4 {
    reader: SongReader,
    /// Units a second of the video's times.
    timescale: u32,
    /// The decoder's settings (H.264's parameter sets, with start codes),
    /// given with the first sample after a jump.
    config: Vec<u8>,
    /// How many bytes say each piece's length in a sample (`avcC`'s).
    length_size: usize,
    fragments: Vec<Fragment>,
    /// Without a `sidx`, the fragments are found by reading on: where to
    /// look next, and whether the end was reached.
    scan: Option<(u64, bool)>,
}

impl Mp4 {
    /// Reads the header: the track, its settings, and the fragments' index.
    fn open(data: Arc<SongData>) -> Result<Self, String> {
        let mut file = Self {
            reader: data.reader(false),
            timescale: 0,
            config: Vec::new(),
            length_size: 4,
            fragments: Vec::new(),
            scan: None,
        };
        let mut at = 0;
        loop {
            let (kind, header, size) = file
                .box_at(at)?
                .ok_or("the video ended before its first picture")?;
            match &kind {
                b"moov" => {
                    let moov = file.read_at(at + header, size - header)?;
                    file.read_moov(&moov)?;
                }
                b"sidx" => {
                    let sidx = file.read_at(at + header, size - header)?;
                    file.fragments = read_sidx(&sidx, at + size)?;
                }
                b"moof" => {
                    if file.timescale == 0 {
                        return Err("the video has no track".into());
                    }
                    if file.fragments.is_empty() {
                        file.scan = Some((at, false));
                    }
                    return Ok(file);
                }
                _ => {}
            }
            at += size;
        }
    }

    /// The type, header length and size of the box at `at`; `None` at the
    /// end.
    fn box_at(&mut self, at: u64) -> Result<Option<([u8; 4], u64, u64)>, String> {
        let head = match self.read_at(at, 8) {
            Ok(head) => head,
            Err(_) if self.ended_at(at) => return Ok(None),
            Err(e) => return Err(e),
        };
        let kind = [head[4], head[5], head[6], head[7]];
        let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let (header, size) = if size == 1 {
            let large = self.read_at(at + 8, 8)?;
            (16, u64::from_be_bytes(large.try_into().unwrap_or([0; 8])))
        } else {
            (8, size)
        };
        if size < header {
            return Err("the video's file is damaged".into());
        }
        Ok(Some((kind, header, size)))
    }

    fn ended_at(&mut self, at: u64) -> bool {
        self.reader
            .seek(SeekFrom::End(0))
            .is_ok_and(|end| at >= end)
    }

    /// `len` bytes from `at`, waiting for them to arrive.
    fn read_at(&mut self, at: u64, len: u64) -> Result<Vec<u8>, String> {
        let len = usize::try_from(len).map_err(|_| "the video's file is damaged")?;
        let mut bytes = vec![0; len];
        self.reader
            .seek(SeekFrom::Start(at))
            .and_then(|_| self.reader.read_exact(&mut bytes))
            .map_err(|e| format!("the video stopped arriving: {e}"))?;
        Ok(bytes)
    }

    fn read_moov(&mut self, moov: &[u8]) -> Result<(), String> {
        let trak = child(moov, b"trak").ok_or("the video has no track")?;
        let mdia = child(trak, b"mdia").ok_or("the video has no track")?;
        let mdhd = child(mdia, b"mdhd").ok_or("the video has no track")?;
        self.timescale = if mdhd.first() == Some(&1) {
            be32(mdhd, 20)
        } else {
            be32(mdhd, 12)
        }
        .filter(|t| *t > 0)
        .ok_or("the video has no time scale")?;
        let stsd = child(mdia, b"minf")
            .and_then(|minf| child(minf, b"stbl"))
            .and_then(|stbl| child(stbl, b"stsd"))
            .ok_or("the video has no format")?;
        let (kind, entry) = boxes(stsd.get(8..).unwrap_or_default())
            .next()
            .ok_or("the video has no format")?;
        if !matches!(&kind, b"avc1" | b"avc3") {
            return Err(format!(
                "the video is {}, not H.264",
                String::from_utf8_lossy(&kind)
            ));
        }
        // A visual sample entry's fields come before its boxes (78 bytes).
        let avcc = child(entry.get(78..).unwrap_or_default(), b"avcC")
            .ok_or("the video has no H.264 settings")?;
        let (length_size, sets) =
            read_avcc(avcc).ok_or("the video's H.264 settings are damaged")?;
        self.length_size = length_size;
        self.config = sets
            .iter()
            .flat_map(|set| START_CODE.iter().chain(set.iter()).copied())
            .collect();
        Ok(())
    }

    fn fragment_count(&mut self) -> Result<usize, String> {
        while matches!(self.scan, Some((_, false))) {
            self.scan_one()?;
        }
        Ok(self.fragments.len())
    }

    /// Without an index: finds the next fragment by reading on.
    fn scan_one(&mut self) -> Result<(), String> {
        let Some((mut at, false)) = self.scan else {
            return Ok(());
        };
        loop {
            let Some((kind, header, size)) = self.box_at(at)? else {
                self.scan = Some((at, true));
                return Ok(());
            };
            if &kind == b"moof" {
                let moof = self.read_at(at + header, size - header)?;
                let start = read_moof(&moof, at, self.timescale)?
                    .first()
                    .map_or(0.0, |s| s.at);
                self.fragments.push(Fragment { offset: at, start });
                self.scan = Some((at + size, false));
                return Ok(());
            }
            at += size;
        }
    }

    /// When fragment `n` starts, if there is one (without an index, found
    /// by reading on).
    fn fragment_start(&mut self, n: usize) -> Result<Option<f64>, String> {
        while self.fragments.len() <= n && matches!(self.scan, Some((_, false))) {
            self.scan_one()?;
        }
        Ok(self.fragments.get(n).map(|f| f.start))
    }

    /// The fragment to play `time` from: the last that starts by then.
    fn fragment_at(&mut self, time: f64) -> Result<usize, String> {
        // Without an index, read on until a fragment starts after `time`.
        while matches!(self.scan, Some((_, false)))
            && self.fragments.last().is_none_or(|f| f.start <= time)
        {
            self.scan_one()?;
        }
        if self.fragments.is_empty() {
            return Err("the video has no pictures".into());
        }
        Ok(self
            .fragments
            .iter()
            .rposition(|f| f.start <= time)
            .unwrap_or(0))
    }

    fn samples(&mut self, fragment: usize) -> Result<Vec<Sample>, String> {
        let offset = self
            .fragments
            .get(fragment)
            .ok_or("the video has no such part")?
            .offset;
        let (kind, header, size) = self.box_at(offset)?.ok_or("the video ended early")?;
        if &kind != b"moof" {
            return Err("the video's index is wrong".into());
        }
        let moof = self.read_at(offset + header, size - header)?;
        read_moof(&moof, offset, self.timescale)
    }

    /// A sample's bytes as the decoder takes them (each piece after a
    /// start code, not its length); the first after a jump with the
    /// decoder's settings before it.
    fn sample_bytes(&mut self, sample: &Sample, first: bool) -> Result<Vec<u8>, String> {
        let bytes = self.read_at(sample.offset, u64::from(sample.size))?;
        let mut out = Vec::with_capacity(bytes.len() + self.config.len() + 16);
        if first {
            out.extend_from_slice(&self.config);
        }
        let mut at = 0;
        while at + self.length_size <= bytes.len() {
            let len = bytes[at..at + self.length_size]
                .iter()
                .fold(0usize, |n, b| (n << 8) | usize::from(*b));
            at += self.length_size;
            let piece = bytes
                .get(at..at + len)
                .ok_or("the video's picture is damaged")?;
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(piece);
            at += len;
        }
        Ok(out)
    }
}

/// What starts each piece of H.264 the decoder is given.
const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// An `avcC` box's contents: how many bytes say each piece's length, and
/// the parameter sets (sequence, then picture).
fn read_avcc(avcc: &[u8]) -> Option<(usize, Vec<&[u8]>)> {
    let length_size = usize::from(avcc.get(4)? & 3) + 1;
    let mut at = 5;
    let mut sets = Vec::new();
    for mask in [0x1f, 0xff] {
        let count = avcc.get(at)? & mask;
        at += 1;
        for _ in 0..count {
            let len = usize::from(u16::from_be_bytes([*avcc.get(at)?, *avcc.get(at + 1)?]));
            sets.push(avcc.get(at + 2..at + 2 + len)?);
            at += 2 + len;
        }
    }
    Some((length_size, sets))
}

/// The fragments a `sidx` box lists (`end` is where it ends, which its
/// offsets count from).
fn read_sidx(sidx: &[u8], end: u64) -> Result<Vec<Fragment>, String> {
    let damaged = || "the video's index is damaged".to_string();
    let version = *sidx.first().ok_or_else(damaged)?;
    let timescale = be32(sidx, 8).filter(|t| *t > 0).ok_or_else(damaged)?;
    let (earliest, first_offset, rest) = if version == 0 {
        (
            u64::from(be32(sidx, 12).ok_or_else(damaged)?),
            u64::from(be32(sidx, 16).ok_or_else(damaged)?),
            20,
        )
    } else {
        (
            be64(sidx, 12).ok_or_else(damaged)?,
            be64(sidx, 20).ok_or_else(damaged)?,
            28,
        )
    };
    let count = usize::from(u16::from_be_bytes(
        sidx.get(rest + 2..rest + 4)
            .ok_or_else(damaged)?
            .try_into()
            .map_err(|_| damaged())?,
    ));
    let mut offset = end + first_offset;
    let mut time = earliest;
    let mut fragments = Vec::with_capacity(count);
    for n in 0..count {
        let at = rest + 4 + n * 12;
        let size = u64::from(be32(sidx, at).ok_or_else(damaged)? & 0x7fff_ffff);
        let duration = u64::from(be32(sidx, at + 4).ok_or_else(damaged)?);
        fragments.push(Fragment {
            offset,
            start: time as f64 / f64::from(timescale),
        });
        offset += size;
        time += duration;
    }
    Ok(fragments)
}

/// The samples of a `moof` (its contents), which starts at `start`.
fn read_moof(moof: &[u8], start: u64, timescale: u32) -> Result<Vec<Sample>, String> {
    let damaged = || "the video's part is damaged".to_string();
    let traf = child(moof, b"traf").ok_or_else(damaged)?;
    let tfhd = child(traf, b"tfhd").ok_or_else(damaged)?;
    let tf_flags = be32(tfhd, 0).ok_or_else(damaged)? & 0xff_ffff;
    let mut at = 8;
    // The data's offsets count from the `moof`'s start unless it says.
    let mut base = start;
    if tf_flags & 0x01 != 0 {
        base = be64(tfhd, at).ok_or_else(damaged)?;
        at += 8;
    }
    if tf_flags & 0x02 != 0 {
        at += 4;
    }
    let mut default_duration = 0;
    if tf_flags & 0x08 != 0 {
        default_duration = be32(tfhd, at).ok_or_else(damaged)?;
        at += 4;
    }
    let mut default_size = 0;
    if tf_flags & 0x10 != 0 {
        default_size = be32(tfhd, at).ok_or_else(damaged)?;
    }
    let mut time = match child(traf, b"tfdt") {
        Some(tfdt) if tfdt.first() == Some(&1) => be64(tfdt, 4).ok_or_else(damaged)?,
        Some(tfdt) => u64::from(be32(tfdt, 4).ok_or_else(damaged)?),
        None => 0,
    };
    let trun = child(traf, b"trun").ok_or_else(damaged)?;
    let version = *trun.first().ok_or_else(damaged)?;
    let flags = be32(trun, 0).ok_or_else(damaged)? & 0xff_ffff;
    let count = be32(trun, 4).ok_or_else(damaged)?;
    let mut at = 8;
    let mut data_offset = 0i64;
    if flags & 0x01 != 0 {
        data_offset = i64::from(be32(trun, at).ok_or_else(damaged)? as i32);
        at += 4;
    }
    if flags & 0x04 != 0 {
        at += 4;
    }
    let mut offset = base.checked_add_signed(data_offset).ok_or_else(damaged)?;
    let mut samples = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut field = |present: u32, default: u32| -> Result<u32, String> {
            if flags & present == 0 {
                return Ok(default);
            }
            let value = be32(trun, at).ok_or_else(damaged)?;
            at += 4;
            Ok(value)
        };
        let duration = field(0x100, default_duration)?;
        let size = field(0x200, default_size)?;
        field(0x400, 0)?;
        // When it shows, after when it is decoded (B-frames come later).
        let shift = field(0x800, 0)?;
        let shift = if version == 0 {
            i64::from(shift)
        } else {
            i64::from(shift as i32)
        };
        samples.push(Sample {
            offset,
            size,
            at: (time as i64 + shift) as f64 / f64::from(timescale),
        });
        offset += u64::from(size);
        time += u64::from(duration);
    }
    Ok(samples)
}

/// The boxes in `data`: each one's type and contents.
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let size = be32(data, at)? as usize;
        let kind: [u8; 4] = data.get(at + 4..at + 8)?.try_into().ok()?;
        let (header, size) = match size {
            1 => (16, usize::try_from(be64(data, at + 8)?).ok()?),
            0 => (8, data.len() - at),
            n => (8, n),
        };
        let body = data.get(at + header..at.checked_add(size)?)?;
        at += size;
        Some((kind, body))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| k == kind).map(|(_, body)| body)
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// H.264, decoded by rusty_h264: Baseline and Main (what YouTube's 720p
/// and smaller are), CABAC and B-frames, in safe Rust.
struct H264 {
    decoder: rusty_h264_decoder::Decoder,
    /// YouTube's H.264 is BT.709 in studio range.
    colours: Colours,
}

impl H264 {
    fn new() -> Self {
        Self {
            decoder: rusty_h264_decoder::Decoder::new(),
            colours: Colours::new(&Matrix::new(false, false)),
        }
    }
}

impl Decode for H264 {
    fn decode(
        &mut self,
        sample: Vec<u8>,
        wanted: bool,
        memory: Vec<u8>,
    ) -> Result<Option<Frame>, String> {
        // A decoder reads whatever arrives: should it fail on a picture,
        // the video stops, not YTFast.
        let decoder = &mut self.decoder;
        let decoded =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decoder.decode(&sample)))
                .map_err(|_| "the video could not be decoded".to_string())?
                .map_err(|e| format!("the video could not be decoded: {e}"))?;
        Ok(decoded
            .filter(|_| wanted)
            .map(|picture| rgba(&picture, &self.colours, memory)))
    }
}

/// [`Matrix`] worked out for every value a byte can have: each pixel then
/// takes a few additions (twice as fast as multiplying, on the owner's
/// laptop: 3.3 ms a 720p picture).
struct Colours {
    luma: [i32; 256],
    red: [i32; 256],
    green_u: [i32; 256],
    green_v: [i32; 256],
    blue: [i32; 256],
}

impl Colours {
    fn new(matrix: &Matrix) -> Self {
        let mut colours = Self {
            luma: [0; 256],
            red: [0; 256],
            green_u: [0; 256],
            green_v: [0; 256],
            blue: [0; 256],
        };
        for value in 0..=255u8 {
            let i = usize::from(value);
            let chroma = i32::from(value) - 128;
            // Rounded here, once, rather than for every pixel.
            colours.luma[i] = (i32::from(value) - matrix.y_offset) * matrix.y_scale + 128;
            colours.red[i] = matrix.r_v * chroma;
            colours.green_u[i] = -matrix.g_u * chroma;
            colours.green_v[i] = -matrix.g_v * chroma;
            colours.blue[i] = matrix.b_u * chroma;
        }
        colours
    }
}

/// A decoded picture (planar 4:2:0) in RGBA by `colours`, in `memory`
/// (a picture's no longer shown, or new).
fn rgba(picture: &rusty_h264_common::YuvFrame, colours: &Colours, memory: Vec<u8>) -> Frame {
    let (width, height) = (picture.width, picture.height);
    let chroma_width = width.div_ceil(2);
    let mut rgba = memory;
    rgba.clear();
    rgba.resize(width * height * 4, 255);
    let byte = |x: i32| (x >> 8).clamp(0, 255) as u8;
    for row in 0..height {
        let ys = picture
            .y
            .get(row * width..(row + 1) * width)
            .unwrap_or_default();
        let chroma = (row / 2) * chroma_width;
        let us = picture
            .u
            .get(chroma..chroma + chroma_width)
            .unwrap_or_default();
        let vs = picture
            .v
            .get(chroma..chroma + chroma_width)
            .unwrap_or_default();
        let line = &mut rgba[row * width * 4..(row + 1) * width * 4];
        // Two pixels share their colour: worked out once for both.
        let (pairs, _) = line.as_chunks_mut::<8>();
        let (lumas, _) = ys.as_chunks::<2>();
        for ((pair, y), (u, v)) in pairs.iter_mut().zip(lumas).zip(us.iter().zip(vs)) {
            let (u, v) = (usize::from(*u), usize::from(*v));
            let red = colours.red[v];
            let green = colours.green_u[u] + colours.green_v[v];
            let blue = colours.blue[u];
            let (first, second) = (
                colours.luma[usize::from(y[0])],
                colours.luma[usize::from(y[1])],
            );
            *pair = [
                byte(first + red),
                byte(first + green),
                byte(first + blue),
                255,
                byte(second + red),
                byte(second + green),
                byte(second + blue),
                255,
            ];
        }
        // An odd width's last pixel.
        if width % 2 == 1
            && let (Some(y), Some(u), Some(v)) = (ys.last(), us.last(), vs.last())
        {
            let (u, v) = (usize::from(*u), usize::from(*v));
            let luma = colours.luma[usize::from(*y)];
            let end = line.len() - 4;
            line[end..].copy_from_slice(&[
                byte(luma + colours.red[v]),
                byte(luma + colours.green_u[u] + colours.green_v[v]),
                byte(luma + colours.blue[u]),
                255,
            ]);
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

/// YUV to RGB in whole numbers (8.8 fixed point).
struct Matrix {
    y_scale: i32,
    y_offset: i32,
    r_v: i32,
    g_u: i32,
    g_v: i32,
    b_u: i32,
}

impl Matrix {
    fn new(bt601: bool, full: bool) -> Self {
        // Studio range spreads 16..235 over 0..255.
        let (y_scale, y_offset, chroma) = if full {
            (256, 0, 1.0)
        } else {
            (298, 16, 255.0 / 224.0)
        };
        let (r_v, g_u, g_v, b_u) = if bt601 {
            (1.402, 0.344_136, 0.714_136, 1.772)
        } else {
            (1.5748, 0.187_324, 0.468_124, 1.8556)
        };
        let fixed = |k: f64| (k * chroma * 256.0).round() as i32;
        Self {
            y_scale,
            y_offset,
            r_v: fixed(r_v),
            g_u: fixed(g_u),
            g_v: fixed(g_v),
            b_u: fixed(b_u),
        }
    }

    /// One pixel's colour by the matrix ([`Colours`] does it by table).
    #[cfg(test)]
    fn rgb(&self, y: u8, u: u8, v: u8) -> [u8; 3] {
        let luma = (i32::from(y) - self.y_offset) * self.y_scale;
        let (u, v) = (i32::from(u) - 128, i32::from(v) - 128);
        let clamp = |x: i32| ((x + 128) >> 8).clamp(0, 255) as u8;
        [
            clamp(luma + self.r_v * v),
            clamp(luma - self.g_u * u - self.g_v * v),
            clamp(luma + self.b_u * u),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
        let mut head = vec![version];
        head.extend_from_slice(&flags.to_be_bytes()[1..]);
        head.extend_from_slice(body);
        mp4_box(kind, &head)
    }

    /// A fragmented MP4 in YouTube's layout: `fragments` fragments of
    /// `per` samples, 25 a second (timescale 1000, 40 each), each sample
    /// four bytes saying its number; with a `sidx` when `indexed`.
    fn video_file(fragments: u32, per: u32, indexed: bool) -> Vec<u8> {
        video_file_with(fragments, per, indexed, &|_| 0)
    }

    /// The same, each sample showing `shift(n)` units (of 1000 a second)
    /// after it is decoded, as B-frames do.
    fn video_file_with(
        fragments: u32,
        per: u32,
        indexed: bool,
        shift: &dyn Fn(u32) -> i32,
    ) -> Vec<u8> {
        let mut avc1 = vec![0u8; 78];
        // avcC: version 1, Main profile, lengths in 4 bytes, one sequence
        // and one picture parameter set.
        avc1.extend(mp4_box(
            b"avcC",
            &[
                1, 0x4d, 0x40, 0x1f, 0xff, 0xe1, 0, 2, 0x67, 0xaa, 1, 0, 2, 0x68, 0xbb,
            ],
        ));
        let stsd = full_box(b"stsd", 0, 0, &{
            let mut b = 1u32.to_be_bytes().to_vec();
            b.extend(mp4_box(b"avc1", &avc1));
            b
        });
        let stbl = mp4_box(b"stbl", &stsd);
        let minf = mp4_box(b"minf", &stbl);
        let mut mdhd = vec![0u8; 8];
        mdhd.extend(1000u32.to_be_bytes());
        mdhd.extend([0u8; 8]);
        let mdia = mp4_box(b"mdia", &[full_box(b"mdhd", 0, 0, &mdhd), minf].concat());
        let moov = mp4_box(b"moov", &mp4_box(b"trak", &mdia));
        let head = [mp4_box(b"ftyp", b"dash"), moov].concat();
        let mut pairs = Vec::new();
        let mut number = 0u32;
        for f in 0..fragments {
            let tfhd = full_box(b"tfhd", 0, 0x02_0000 | 0x08, &{
                let mut b = 1u32.to_be_bytes().to_vec();
                b.extend(40u32.to_be_bytes());
                b
            });
            let tfdt = full_box(b"tfdt", 1, 0, &u64::from(f * per * 40).to_be_bytes());
            // trun: data offset, and each sample's size and shift.
            let trun_len = 8 + 4 + 8 + 8 * per as usize;
            let traf_len = 8 + tfhd.len() + tfdt.len() + trun_len;
            let moof_len = 8 + traf_len;
            let mut trun_body = per.to_be_bytes().to_vec();
            trun_body.extend(((moof_len + 8) as u32).to_be_bytes());
            for n in 0..per {
                trun_body.extend(8u32.to_be_bytes());
                trun_body.extend(shift(number + n).to_be_bytes());
            }
            let trun = full_box(b"trun", 1, 0x01 | 0x200 | 0x800, &trun_body);
            let moof = mp4_box(b"moof", &mp4_box(b"traf", &[tfhd, tfdt, trun].concat()));
            assert_eq!(moof.len(), moof_len);
            // Each sample one piece of four bytes, its number.
            let mut mdat = Vec::new();
            for _ in 0..per {
                mdat.extend(4u32.to_be_bytes());
                mdat.extend(number.to_be_bytes());
                number += 1;
            }
            pairs.push([moof, mp4_box(b"mdat", &mdat)].concat());
        }
        if !indexed {
            return [head, pairs.concat()].concat();
        }
        let mut sidx = 1u32.to_be_bytes().to_vec();
        sidx.extend(1000u32.to_be_bytes());
        sidx.extend(0u32.to_be_bytes());
        sidx.extend(0u32.to_be_bytes());
        sidx.extend(0u16.to_be_bytes());
        sidx.extend((fragments as u16).to_be_bytes());
        for pair in &pairs {
            sidx.extend((pair.len() as u32).to_be_bytes());
            sidx.extend((per * 40).to_be_bytes());
            sidx.extend(0x9000_0000u32.to_be_bytes());
        }
        [head, full_box(b"sidx", 0, 0, &sidx), pairs.concat()].concat()
    }

    /// A stand-in decoder: each sample becomes a 1 by 1 picture whose red
    /// says the sample's number.
    struct Numbers;

    impl Decode for Numbers {
        fn decode(
            &mut self,
            sample: Vec<u8>,
            wanted: bool,
            _memory: Vec<u8>,
        ) -> Result<Option<Frame>, String> {
            if !wanted {
                return Ok(None);
            }
            // The first sample after a jump carries the settings first.
            Ok(Some(Frame {
                width: 1,
                height: 1,
                rgba: sample[sample.len() - 4..].to_vec(),
            }))
        }
    }

    fn numbers() -> impl Fn() -> Result<Box<dyn Decode>, String> + Send {
        || Ok(Box::new(Numbers) as Box<dyn Decode>)
    }

    fn number(picture: &Picture) -> u32 {
        u32::from_be_bytes(picture.rgba[..4].try_into().unwrap())
    }

    /// Waits until the picture shown at `clock` is the one wanted.
    fn wait_for(video: &Video, clock: f64, wanted: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let shown = video.at(clock);
            if shown.picture.as_deref().map(number) == Some(wanted) {
                return;
            }
            assert!(Instant::now() < deadline, "at {clock}: {shown:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn reads_the_fragments_index_and_samples() {
        let bytes = video_file(3, 5, true);
        let mut file = Mp4::open(SongData::complete(bytes.clone())).unwrap();
        assert_eq!(file.timescale, 1000);
        assert_eq!(
            file.config,
            [0, 0, 0, 1, 0x67, 0xaa, 0, 0, 0, 1, 0x68, 0xbb]
        );
        assert_eq!(file.fragment_count().unwrap(), 3);
        assert_eq!(file.fragments[1].start, 0.2);
        let samples = file.samples(1).unwrap();
        assert_eq!(samples.len(), 5);
        assert!((samples[2].at - 0.28).abs() < 1e-9);
        // Pieces after start codes, not lengths; the settings before the
        // first sample after a jump.
        assert_eq!(
            file.sample_bytes(&samples[0], true).unwrap(),
            [
                0, 0, 0, 1, 0x67, 0xaa, 0, 0, 0, 1, 0x68, 0xbb, 0, 0, 0, 1, 0, 0, 0, 5
            ]
        );
        assert_eq!(
            file.sample_bytes(&samples[2], false).unwrap(),
            [0, 0, 0, 1, 0, 0, 0, 7]
        );
        assert_eq!(file.fragment_at(0.0).unwrap(), 0);
        assert_eq!(file.fragment_at(0.39).unwrap(), 1);
        assert_eq!(file.fragment_at(9.0).unwrap(), 2);
        // Without an index, the same, found by reading on.
        let mut scanned = Mp4::open(SongData::complete(video_file(3, 5, false))).unwrap();
        assert_eq!(scanned.fragment_at(0.39).unwrap(), 1);
        assert_eq!(scanned.fragment_count().unwrap(), 3);
        let times = |samples: &[Sample]| -> Vec<(f64, u32)> {
            samples.iter().map(|s| (s.at, s.size)).collect()
        };
        assert_eq!(times(&scanned.samples(1).unwrap()), times(&samples));
        assert_eq!(scanned.fragment_start(2).unwrap(), Some(0.4));
        assert_eq!(scanned.fragment_start(3).unwrap(), None);
    }

    #[test]
    fn pictures_come_with_the_music_and_follow_its_jumps() {
        // 4 fragments of 10 pictures, 25 a second: 1.6 s of video.
        let video = Video::start_with(SongData::complete(video_file(4, 10, true)), numbers());
        wait_for(&video, 0.0, 0);
        // As the music goes on, the picture due: 0.5 s is the 12th.
        wait_for(&video, 0.5, 12);
        let shown = video.at(0.5);
        assert!(
            shown.next.is_some_and(|n| (n - 0.52).abs() < 1e-9),
            "{shown:?}"
        );
        // A jump on, into the last fragment.
        wait_for(&video, 1.3, 32);
        // A jump back.
        wait_for(&video, 0.1, 2);
        // At the end, the last picture stays.
        wait_for(&video, 5.0, 39);
    }

    /// B-frames: decoded before the pictures they show between, each
    /// shows in its time's place.
    #[test]
    fn pictures_show_in_their_order_not_the_decoders() {
        // Of each three: the third shows first, 80 earlier; the others 40
        // later (0, 2, 1 at 0.04, 0.08, 0.0 ... in time: 2, 0, 1).
        let shift = |n: u32| if n % 3 == 2 { -80 } else { 40 };
        let file = video_file_with(4, 9, true, &shift);
        let samples = Mp4::open(SongData::complete(file.clone()))
            .unwrap()
            .samples(0)
            .unwrap();
        assert!((samples[2].at - 0.0).abs() < 1e-9);
        assert!((samples[0].at - 0.04).abs() < 1e-9);
        let video = Video::start_with(SongData::complete(file), numbers());
        wait_for(&video, 0.0, 2);
        wait_for(&video, 0.04, 0);
        wait_for(&video, 0.08, 1);
        wait_for(&video, 0.12, 5);
    }

    #[test]
    fn a_video_still_arriving_shows_what_has_arrived() {
        let bytes = video_file(2, 10, true);
        let data = SongData::new(Some(bytes.len() as u64));
        // The header and the first fragment, not the second.
        let half = bytes.len() - 200;
        data.push_for_test(&bytes[..half]);
        let video = Video::start_with(Arc::clone(&data), numbers());
        wait_for(&video, 0.2, 5);
        data.push_for_test(&bytes[half..]);
        data.finish_for_test();
        wait_for(&video, 0.6, 15);
    }

    #[test]
    fn a_resting_video_decodes_nothing_more() {
        let video = Video::start_with(SongData::complete(video_file(4, 10, true)), numbers());
        wait_for(&video, 0.0, 0);
        video.rest();
        std::thread::sleep(Duration::from_millis(50));
        let ready = video.shared.lock().ready.len();
        assert!(ready <= AHEAD, "{ready}");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(video.shared.lock().ready.len(), ready);
    }

    /// A real video, as YouTube serves it (H.264 in fragmented MP4, with
    /// B-frames), decoded by rusty_h264: runs only when `YTFAST_TEST_VIDEO`
    /// names such a file (for one, `yt-dlp -f 136` of a music video); run
    /// it with `--release`.
    #[test]
    fn decodes_a_real_video_in_time() {
        let Ok(path) = std::env::var("YTFAST_TEST_VIDEO") else {
            return;
        };
        let video = Video::start(SongData::complete(std::fs::read(path).unwrap()));
        let started = Instant::now();
        // Ten seconds of music, 25 pictures a second, as the window asks.
        let mut shown = 0;
        let mut late = 0;
        let mut last = None;
        for step in 0..250 {
            let clock = f64::from(step) * 0.04;
            let due = started + Duration::from_secs_f64(clock);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            let now = video.at(clock);
            if let Some(picture) = now.picture {
                assert_eq!(picture.rgba.len(), picture.width * picture.height * 4);
                if last.is_none_or(|at| at != picture.at) {
                    shown += 1;
                }
                if clock - picture.at > 0.1 {
                    late += 1;
                }
                last = Some(picture.at);
            }
        }
        assert_eq!(video.failure(), None);
        println!("VIDEO {shown} pictures shown in 10 s, {late} steps late");
        assert!(shown > 200, "{shown}");
        // A jump: a picture from there soon after.
        let jumped = Instant::now();
        loop {
            let now = video.at(60.0);
            if now
                .picture
                .as_ref()
                .is_some_and(|p| (p.at - 60.0).abs() < 0.2)
            {
                break;
            }
            assert!(jumped.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(5));
        }
        println!("VIDEO jumped to 60 s in {:?}", jumped.elapsed());
    }

    /// The tables give the matrix's colours, for every pixel of a
    /// picture of odd width, in the memory handed over.
    #[test]
    fn a_picture_comes_out_as_the_matrix_says_in_the_memory_given() {
        let matrix = Matrix::new(false, false);
        let colours = Colours::new(&matrix);
        let (width, height) = (5usize, 3usize);
        let frame = rusty_h264_common::YuvFrame {
            width,
            height,
            y: (0..width * height).map(|i| (16 + i * 13) as u8).collect(),
            u: (0..6).map(|i| (60 + i * 30) as u8).collect(),
            v: (0..6).map(|i| (220 - i * 25) as u8).collect(),
        };
        let memory = Vec::with_capacity(width * height * 4);
        let place = memory.as_ptr();
        let out = rgba(&frame, &colours, memory);
        assert_eq!(out.rgba.as_ptr(), place);
        for row in 0..height {
            for column in 0..width {
                let c = (row / 2) * 3 + column / 2;
                let [r, g, b] = matrix.rgb(frame.y[row * width + column], frame.u[c], frame.v[c]);
                let at = (row * width + column) * 4;
                assert_eq!(out.rgba[at..at + 4], [r, g, b, 255], "{row} {column}");
            }
        }
    }

    #[test]
    fn colours_come_out_as_the_matrix_says() {
        let studio = Matrix::new(false, false);
        // Black and white in studio range, and a mid grey.
        assert_eq!(studio.rgb(16, 128, 128), [0, 0, 0]);
        assert_eq!(studio.rgb(235, 128, 128), [255, 255, 255]);
        assert_eq!(studio.rgb(126, 128, 128), [128, 128, 128]);
        // BT.709 red (studio): Y 63, U 102, V 240.
        let [r, g, b] = studio.rgb(63, 102, 240);
        assert!(r > 250 && g < 5 && b < 5, "{r} {g} {b}");
        let full = Matrix::new(true, true);
        assert_eq!(full.rgb(0, 128, 128), [0, 0, 0]);
        assert_eq!(full.rgb(255, 128, 128), [255, 255, 255]);
    }
}
