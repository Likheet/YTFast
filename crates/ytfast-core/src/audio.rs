//! Playing a song's audio.
//!
//! A song plays from its first bytes while the rest downloads
//! ([`crate::stream`]); once all of it has arrived it stays, so pausing for
//! hours, closing the laptop lid or changing Wi-Fi cannot break it.
//!
//! Output goes through `fastframe-audio`, the device stream Spotifast uses:
//! it costs no CPU while paused, follows the default output when headphones
//! come and go, and reopens after a failure. symphonia decodes (AAC in MP4)
//! on a thread of its own per song, a little ahead of the device, and
//! rodio's mixer converts to the device's rate. The device never waits for
//! the download: when the decoder is behind, it plays silence.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use fastframe_audio::{Buffer, BufferSize, Device, Maintained, OutputOptions, Render};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::stream::SongData;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("the audio download failed: {0}")]
    Download(String),
    #[error("the audio could not be decoded: {0}")]
    Decode(String),
    #[error("{0}")]
    Device(String),
}

/// How much to turn a song down, from YouTube's loudness figure. YouTube
/// only turns loud songs down; quiet ones are left alone.
pub fn gain_for_loudness(loudness_db: Option<f64>) -> f32 {
    match loudness_db {
        Some(db) if db > 0.0 => 10f64.powf(-db / 20.0).clamp(0.1, 1.0) as f32,
        _ => 1.0,
    }
}

/// A song's decoder: its samples, one after another, waiting for the
/// download when it gets ahead. [`Playing`] runs it on a thread of its own.
///
/// symphonia is used directly: rodio's own decoder reads a fragmented MP4's
/// length as zero and so cannot seek in YouTube's streams. Seeking happens
/// once, when the source is made (see [`Player`]); after that it only plays
/// forward.
struct SongSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    samples: Option<SampleBuffer<f32>>,
    position: usize,
    channels: u16,
    rate: u32,
}

impl SongSource {
    /// Opens a song and positions it at `at`. From the start it plays as
    /// the bytes arrive; jumping elsewhere needs the whole song, so it
    /// waits for the download to finish (usually seconds).
    fn open(data: &Arc<SongData>, at: Duration) -> Result<Self, AudioError> {
        let fail = |what: &str, e: DecodeError| AudioError::Decode(format!("{what}: {e}"));
        let seekable = !at.is_zero();
        if seekable {
            data.wait_complete(WHOLE_SONG_WAIT)
                .map_err(AudioError::Download)?;
        }
        let stream = MediaSourceStream::new(Box::new(data.reader(seekable)), Default::default());
        let mut hint = Hint::new();
        hint.mime_type("audio/mp4").with_extension("m4a");
        let options = FormatOptions {
            enable_gapless: true,
            ..FormatOptions::default()
        };
        let probed = symphonia::default::get_probe()
            .format(&hint, stream, &options, &MetadataOptions::default())
            .map_err(|e| fail("not a playable song", e))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::Decode("no audio track".into()))?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let rate = params
            .sample_rate
            .filter(|r| *r > 0)
            .ok_or_else(|| AudioError::Decode("no sample rate".into()))?;
        let channels = params.channels.map_or(2, |c| c.count()) as u16;
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| fail("no decoder for this audio", e))?;

        let mut source = Self {
            format,
            decoder,
            track_id,
            samples: None,
            position: 0,
            channels: channels.max(1),
            rate,
        };
        if !at.is_zero() {
            let seeked = source
                .format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::from(at.as_secs_f64()),
                        track_id: Some(track_id),
                    },
                )
                .map_err(|e| fail("could not jump in the song", e))?;
            source.decoder.reset();
            // The demuxer lands on the packet before the target; skip the
            // difference so the jump is exact.
            let short = seeked.required_ts.saturating_sub(seeked.actual_ts);
            let frames = match params.time_base {
                Some(base) => {
                    let time = base.calc_time(short);
                    ((time.seconds as f64 + time.frac) * f64::from(rate)).round() as u64
                }
                None => short,
            };
            for _ in 0..frames * u64::from(source.channels) {
                if source.next().is_none() {
                    break;
                }
            }
        }
        Ok(source)
    }

    /// Decodes the next packet. `false` at the end of the song.
    fn refill(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                // The end of the song, a broken file, or a download that
                // broke off (the player tells which: `Player::broke_off`):
                // either way, stop.
                Err(_) => return false,
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let frames = decoded.capacity() as u64;
                    let fits = self.samples.as_ref().is_some_and(|b| {
                        b.capacity() >= decoded.capacity() * spec.channels.count()
                    });
                    if !fits {
                        self.samples = Some(SampleBuffer::new(frames, spec));
                    }
                    let samples = self.samples.as_mut().expect("made above");
                    samples.copy_interleaved_ref(decoded);
                    self.position = 0;
                    if !samples.samples().is_empty() {
                        return true;
                    }
                }
                // One damaged packet: skip it, as players do.
                Err(DecodeError::DecodeError(_)) => continue,
                Err(_) => return false,
            }
        }
    }

    /// The samples of the next packet (or what is left of the current
    /// one). `None` at the end of the song.
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        let len = self.samples.as_ref().map_or(0, |b| b.samples().len());
        if self.position >= len && !self.refill() {
            return None;
        }
        let samples = self.samples.as_ref()?.samples();
        let chunk = samples[self.position..].to_vec();
        self.position = samples.len();
        Some(chunk)
    }
}

impl Iterator for SongSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let len = self.samples.as_ref().map_or(0, |b| b.samples().len());
        if self.position >= len && !self.refill() {
            return None;
        }
        let sample = self.samples.as_ref()?.samples().get(self.position).copied();
        self.position += 1;
        sample
    }
}

/// Seconds of sound decoded ahead of the device.
const AHEAD_SECONDS: usize = 2;
/// Silence is played in blocks of this many frames while the decoder waits
/// for the download, and after the song's end.
const SILENCE_FRAMES: usize = 256;
/// Silence after a song's end beyond the device's latency, so that its last
/// moment is heard before the song counts as finished: the device takes
/// sound in steps (about 11 ms on Windows), and where in its step the song
/// ended is not known.
const TAIL_MARGIN: Duration = Duration::from_millis(20);
/// The longest latency the silence after a song waits for. AirPlay
/// speakers report about 2 s; a device that reports more is taken to be
/// wrong, and must not hold up the next song for long.
const LONGEST_LATENCY: Duration = Duration::from_secs(3);

/// Samples of silence after a song's end ([`Playing`]), for a device that
/// plays sound `latency` after it is given: whole frames, so the channels
/// stay in step.
fn tail_samples(latency: Duration, rate: u32, channels: u16) -> usize {
    let wait = latency.min(LONGEST_LATENCY) + TAIL_MARGIN;
    let frames = (wait.as_secs_f64() * f64::from(rate)).ceil() as usize;
    frames * usize::from(channels)
}

/// Decoded sound on its way from a song's decoder to the device.
#[derive(Default)]
struct Queue {
    chunks: VecDeque<Vec<f32>>,
    /// Samples in `chunks`.
    samples: usize,
    /// Samples ever pushed: the song's length so far.
    pushed: u64,
    /// Decoding has ended: at the end of the song, or where it broke off.
    ended: bool,
    /// Nobody plays the song any more: decoding stops.
    let_go: bool,
}

impl Queue {
    fn push(&mut self, chunk: Vec<f32>) {
        self.samples += chunk.len();
        self.pushed += chunk.len() as u64;
        self.chunks.push_back(chunk);
    }
}

/// What a song's decoder and the device share.
struct Feed {
    queue: Mutex<Queue>,
    /// Woken when the device takes sound, or the song is let go.
    room: Condvar,
    /// Samples of silence played while the decoder waited for the download.
    silent: AtomicU64,
    /// Samples per second (all channels).
    per_second: u64,
    /// The song's length in samples from where the feed started, once
    /// decoding has ended ([`Feed::end`]); `u64::MAX` until then, and for a
    /// song let go.
    length: AtomicU64,
}

/// What the device gets from a [`Feed`].
enum Taken {
    Sound(Vec<f32>),
    /// Nothing decoded yet: the download is behind.
    Waiting,
    /// All of the song has been taken: it ended, or broke off.
    End,
    /// Nobody plays the song any more.
    LetGo,
}

impl Feed {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next decoded sound. Never waits: the lock is only ever held to
    /// add or take a chunk.
    fn take(&self) -> Taken {
        let mut queue = self.lock();
        match queue.chunks.pop_front() {
            Some(chunk) => {
                queue.samples -= chunk.len();
                drop(queue);
                self.room.notify_one();
                Taken::Sound(chunk)
            }
            None if queue.let_go => Taken::LetGo,
            None if queue.ended => Taken::End,
            None => Taken::Waiting,
        }
    }

    /// Decoding has ended. At the end of the song, or where it broke off,
    /// the song's length (from where the feed started) is known; a song let
    /// go has none.
    fn end(&self, queue: &mut Queue) {
        queue.ended = true;
        if !queue.let_go {
            self.length.store(queue.pushed, Ordering::Release);
        }
    }

    /// The song's length from where the feed started, once decoding has
    /// ended.
    fn length(&self) -> Option<Duration> {
        let samples = self.length.load(Ordering::Acquire);
        (samples != u64::MAX)
            .then(|| Duration::from_secs_f64(samples as f64 / self.per_second.max(1) as f64))
    }

    /// Stops the decoder: the song is no longer played. What it decoded
    /// goes too: a song let go while paused may stay in the player until
    /// Play, and seeking while paused lets go of one each time.
    fn let_go(&self) {
        let mut queue = self.lock();
        queue.let_go = true;
        queue.chunks = VecDeque::new();
        queue.samples = 0;
        drop(queue);
        self.room.notify_all();
    }

    /// How long the silence played while waiting for the download lasted.
    fn silence(&self) -> Duration {
        let samples = self.silent.load(Ordering::Relaxed);
        Duration::from_secs_f64(samples as f64 / self.per_second.max(1) as f64)
    }
}

/// Decodes `song` into `feed`, keeping at most `ahead` samples ready, until
/// the song ends or is let go.
fn decode_ahead(mut song: SongSource, feed: &Feed, ahead: usize) {
    // However decoding stops (a panic in the decoder too), the device
    // hears that it has, so the song ends rather than playing silence for
    // ever.
    struct Ended<'a>(&'a Feed);
    impl Drop for Ended<'_> {
        fn drop(&mut self) {
            self.0.end(&mut self.0.lock());
        }
    }
    let _ended = Ended(feed);
    while let Some(chunk) = song.next_chunk() {
        let mut queue = feed.lock();
        while queue.samples >= ahead && !queue.let_go {
            queue = feed
                .room
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if queue.let_go {
            return;
        }
        queue.push(chunk);
    }
}

/// The time between giving the device a sample and hearing it, asked when
/// a song ends ([`Playing`]).
type Latency = Box<dyn Fn() -> Duration + Send>;

/// A song as the device plays it, as a rodio source.
///
/// The device must never wait (the sound would stop, and pausing or moving
/// to other headphones would wait too), so the song is decoded on a thread
/// of its own ([`decode_ahead`]), a little ahead. When the decoder is
/// behind, because the download is, silence plays, and is counted so the
/// song's position (and the lyrics) do not run ahead. Dropping it ends the
/// decoding thread.
///
/// After the song's end, silence plays for as long as the device takes to
/// play what it holds (its latency: about 0.2 s on Windows), and only then
/// does the source end. So the song is finished ([`Player::finished`]) once
/// its end has been heard, and the next song, which empties the device,
/// cuts off nothing of it.
struct Playing {
    feed: Arc<Feed>,
    chunk: Vec<f32>,
    at: usize,
    /// Samples of silence still to play.
    silence: usize,
    /// Samples of silence still to play after the song's end; `None` before
    /// it.
    tail: Option<usize>,
    latency: Latency,
    channels: u16,
    rate: u32,
}

impl Playing {
    /// Starts playing `song`: its first moments are decoded here, so the
    /// device's first request is answered with sound, and the rest on a
    /// thread of its own.
    fn start(mut song: SongSource, latency: Latency) -> Result<(Self, Arc<Feed>), AudioError> {
        let (channels, rate) = (song.channels, song.rate);
        let per_second = rate as usize * usize::from(channels);
        let mut queue = Queue::default();
        while queue.samples < per_second / 2 {
            match song.next_chunk() {
                Some(chunk) => queue.push(chunk),
                None => {
                    queue.ended = true;
                    break;
                }
            }
        }
        let ended = queue.ended;
        let length = if ended { queue.pushed } else { u64::MAX };
        let feed = Arc::new(Feed {
            queue: Mutex::new(queue),
            room: Condvar::new(),
            silent: AtomicU64::new(0),
            per_second: per_second as u64,
            length: AtomicU64::new(length),
        });
        if !ended {
            let decoding = Arc::clone(&feed);
            std::thread::Builder::new()
                .name("ytfast-decoder".into())
                .spawn(move || decode_ahead(song, &decoding, per_second * AHEAD_SECONDS))
                .map_err(|e| AudioError::Device(format!("could not start decoding: {e}")))?;
        }
        let playing = Self {
            feed: Arc::clone(&feed),
            chunk: Vec::new(),
            at: 0,
            silence: 0,
            tail: None,
            latency,
            channels,
            rate,
        };
        Ok((playing, feed))
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.feed.let_go();
    }
}

impl Iterator for Playing {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(&sample) = self.chunk.get(self.at) {
                self.at += 1;
                return Some(sample);
            }
            if self.silence > 0 {
                self.silence -= 1;
                return Some(0.0);
            }
            match self.feed.take() {
                Taken::Sound(chunk) => {
                    self.chunk = chunk;
                    self.at = 0;
                }
                Taken::Waiting => {
                    // Whole frames, so the channels stay in step.
                    let samples = SILENCE_FRAMES * usize::from(self.channels);
                    self.feed
                        .silent
                        .fetch_add(samples as u64, Ordering::Relaxed);
                    self.silence = samples;
                }
                Taken::End => {
                    // Silence while the device plays the last of the song,
                    // in blocks, so a song let go meanwhile stops at once.
                    // Not counted: it carries the position to the end.
                    let left = match self.tail {
                        Some(left) => left,
                        None => tail_samples((self.latency)(), self.rate, self.channels),
                    };
                    if left == 0 {
                        return None;
                    }
                    let block = left.min(SILENCE_FRAMES * usize::from(self.channels));
                    self.tail = Some(left - block);
                    self.silence = block;
                }
                Taken::LetGo => return None,
            }
        }
    }
}

impl rodio::Source for Playing {
    fn current_span_len(&self) -> Option<usize> {
        // One format for the whole song.
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// The length of a song's audio, by decoding all of it (used to check that
/// a download is a whole, playable song).
pub fn decoded_length(bytes: &[u8]) -> Result<Duration, AudioError> {
    let source = SongSource::open(&SongData::complete(bytes.to_vec()), Duration::ZERO)?;
    let (rate, channels) = (f64::from(source.rate), f64::from(source.channels));
    let samples = source.count() as f64;
    Ok(Duration::from_secs_f64(samples / channels / rate))
}

/// A mixer made for the device's current format, waiting for the player.
type MixerSlot = Arc<Mutex<Option<(rodio::mixer::Mixer, u32)>>>;

/// Fills the device from rodio's mixer, or with silence before there is
/// one. Adapted from Spotifast's `src/sink.rs` (MIT, Copyright (c) 2026
/// Carmine Paolino). A stream in the format the mixer already has keeps it,
/// so a reopen on another device carries on from the same sample.
struct MixerRender {
    source: Option<rodio::mixer::MixerSource>,
    format: (u32, u16),
    made: MixerSlot,
}

impl Render for MixerRender {
    fn configure(&mut self, sample_rate: u32, channels: u16) {
        if self.source.is_some() && self.format == (sample_rate, channels) {
            return;
        }
        let (mixer, source) = rodio::mixer::mixer(channels, sample_rate);
        self.source = Some(source);
        self.format = (sample_rate, channels);
        *self.made.lock().unwrap_or_else(PoisonError::into_inner) = Some((mixer, sample_rate));
    }

    fn render(&mut self, out: &mut [f32]) {
        match &mut self.source {
            Some(source) => out
                .iter_mut()
                .for_each(|s| *s = source.next().unwrap_or(0.0)),
            None => out.fill(0.0),
        }
    }
}

struct Loaded {
    data: Arc<SongData>,
    gain: f32,
}

/// The longest a jump waits for the rest of the song to arrive.
const WHOLE_SONG_WAIT: Duration = Duration::from_secs(60);

/// The device's stream, paused, with the mixer that feeds it.
struct OpenedOutput {
    output: fastframe_audio::Output<MixerRender>,
    made: MixerSlot,
    mixer: rodio::mixer::Mixer,
    /// Opened by the default output's name, not as Windows' default: it
    /// does not follow the default by itself ([`Player::maintain`] does).
    by_name: bool,
}

/// Opens the default output, paused (the device rests until there is a
/// song to play), the ways [`ways_to_open`] lists, until one works.
fn open_output() -> Result<OpenedOutput, AudioError> {
    let mut first_error = None;
    for device in ways_to_open(default_output_name()) {
        let by_name = matches!(device, Device::Named(_));
        match open_output_on(device.clone()) {
            Ok(opened) => {
                if first_error.is_some() {
                    log::warn!("audio output: opened as {device:?}");
                }
                return Ok(OpenedOutput { by_name, ..opened });
            }
            Err(error) => {
                log::warn!("audio output: {device:?} would not open: {error}");
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error
        .unwrap_or_else(|| AudioError::Device("There is no audio output device.".into())))
}

/// The ways to open the default output, in the order they are tried.
///
/// On Windows, by its name first, the way most programs open a device
/// ([`Player::maintain`] then follows the default itself). Windows' virtual
/// default device, which would follow the default by itself, comes second:
/// it refuses to open on some computers, and goes on refusing (seen with
/// Bluetooth headphones, and after moving to a laptop's own speakers:
/// "Failed to get audio client: Cannot change thread mode after it is
/// set"), and the audio library opens a new stream when the default
/// changes all the same. Elsewhere the default output is opened as the
/// default, and by its name only when that fails.
fn ways_to_open(default_name: Option<String>) -> Vec<Device> {
    let named = default_name.map(Device::Named);
    if cfg!(windows) {
        named.into_iter().chain([Device::Default]).collect()
    } else {
        std::iter::once(Device::Default).chain(named).collect()
    }
}

/// The default output's name, or `None` when there is none (or the audio
/// library fails while asking: it can, while a device comes or goes).
pub fn default_output_name() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    std::panic::catch_unwind(|| {
        let device = cpal::default_host().default_output_device()?;
        let description = device.description().ok()?;
        Some(description.name().to_string())
    })
    .ok()
    .flatten()
}

/// How often an output opened by name looks for a new default output.
const DEFAULT_CHECK: Duration = Duration::from_secs(2);

/// What [`Player::maintain`] did that is worth telling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Maintenance {
    /// Sound moved to this device (the default output changed, or the
    /// stream failed and opened again), and plays on.
    Moved(String),
    /// Something is wrong with the sound, in words for the window.
    Problem(String),
}

/// Opens `device`, paused.
fn open_output_on(device: Device) -> Result<OpenedOutput, AudioError> {
    let made = MixerSlot::default();
    let render = MixerRender {
        source: None,
        format: (0, 0),
        made: Arc::clone(&made),
    };
    let options = OutputOptions {
        device,
        channels: 2,
        // YouTube's audio is 44.1 kHz; the device's own rate is used if
        // it cannot run at that.
        sample_rate: Some(44_100),
        buffer: Buffer::FixedOnWindows(BufferSize::Duration(Duration::from_millis(200))),
        follow_default: true,
        ..OutputOptions::default()
    };
    let mut output = fastframe_audio::Output::open(options, render)
        .map_err(|e| AudioError::Device(e.to_string()))?;
    let (mixer, _rate) = made
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| AudioError::Device("the audio output did not start".into()))?;
    output.pause();
    Ok(OpenedOutput {
        output,
        made,
        mixer,
        by_name: false,
    })
}

/// Plays one song at a time. Lives on one thread (the audio device handle
/// cannot move between threads on every platform).
///
/// Every jump (seek, new song, a device that changed format) builds a fresh
/// decoder and a fresh rodio sink instead of asking the playing one to seek:
/// rodio's seek waits for the audio thread, which does not run while the
/// device is paused.
///
/// The device keeps its buffer full ahead of what is heard (200 ms on
/// Windows), and neither pausing nor a new sink empties it. So a new song,
/// or a jump, after sound was played moves to a fresh stream, which starts
/// empty; otherwise the end of what played before would sound first.
/// Opening one takes about 0.15 s, so a spare is opened while a song plays
/// ([`Player::prepare_spare`]), and moving to it takes about 0.01 s. A song
/// that played to its end is finished only once the device has played all
/// of it (silence follows it meanwhile: [`Playing`]), so the move to a fresh
/// stream for the next song loses nothing of it.
pub struct Player {
    output: fastframe_audio::Output<MixerRender>,
    made: MixerSlot,
    mixer: rodio::mixer::Mixer,
    sink: rodio::Sink,
    /// The device's stream has been given sound: its buffer may still hold
    /// some of it.
    fed: bool,
    /// A fresh stream, paused, ready to take over from one that was fed.
    spare: Option<OpenedOutput>,
    /// When opening a spare last failed: it is not tried again for a while.
    spare_failed: Option<Instant>,
    song: Option<Loaded>,
    /// What the current sink plays from: its silence is taken off the
    /// position.
    feed: Option<Arc<Feed>>,
    /// Where the current sink started in the song.
    offset: Duration,
    paused: bool,
    /// The listener's volume, 0 to 1, on top of the song's loudness gain.
    volume: f32,
    /// The output was opened by the default's name ([`OpenedOutput`]), so
    /// `maintain` follows the default itself, looking every
    /// [`DEFAULT_CHECK`].
    by_name: bool,
    default_checked: Instant,
    /// When [`Player::open_again`] last failed: it waits [`DEFAULT_CHECK`]
    /// before it tries again.
    open_again_failed: Option<Instant>,
}

impl Player {
    pub fn open() -> Result<Self, AudioError> {
        let OpenedOutput {
            output,
            made,
            mixer,
            by_name,
        } = open_output()?;
        let sink = rodio::Sink::connect_new(&mixer);
        Ok(Self {
            by_name,
            default_checked: Instant::now(),
            open_again_failed: None,
            output,
            made,
            mixer,
            sink,
            fed: false,
            spare: None,
            spare_failed: None,
            song: None,
            feed: None,
            offset: Duration::ZERO,
            paused: false,
            volume: 1.0,
        })
    }

    pub fn device_name(&self) -> &str {
        self.output.device_name()
    }

    /// Starts a new song from the beginning, playing as its bytes arrive.
    /// `gain` is from [`gain_for_loudness`].
    pub fn play_song(&mut self, data: Arc<SongData>, gain: f32) -> Result<(), AudioError> {
        self.empty_the_device();
        self.song = Some(Loaded { data, gain });
        self.paused = false;
        // The song before the device: a device started first can take its
        // first 0.2 s as silence before the song is there (it did, about
        // every other time).
        self.restart_at(Duration::ZERO)?;
        self.output.resume();
        self.fed = true;
        Ok(())
    }

    /// Swaps the device's stream for a fresh one when the old may still
    /// hold sound, so none of it plays before what comes next: the spare
    /// when there is one, else one opened now. When a fresh one cannot be
    /// opened, the old stays: a moment of the last sound is better than
    /// none.
    fn empty_the_device(&mut self) {
        if !self.fed {
            return;
        }
        let started = Instant::now();
        // Opened beside the old one, which goes only once this one works.
        let fresh = match self.spare.take() {
            Some(spare) => Ok(spare),
            None => open_output(),
        };
        match fresh {
            Ok(fresh) => {
                self.install(fresh);
                self.fed = false;
                log::debug!(
                    "audio output: a fresh stream in {} ms",
                    started.elapsed().as_millis()
                );
            }
            Err(error) => log::warn!(
                "audio output: could not open a fresh stream, so a moment of the last \
                 sound may play: {error}"
            ),
        }
    }

    /// Makes `opened` the device's stream, with a sink of its own.
    fn install(&mut self, opened: OpenedOutput) {
        self.output = opened.output;
        self.made = opened.made;
        self.mixer = opened.mixer;
        self.by_name = opened.by_name;
        self.sink = rodio::Sink::connect_new(&self.mixer);
    }

    /// Opens a spare stream for the next song or jump, while a song plays
    /// and there is none (about 0.15 s; call it when nothing waits). After
    /// a failure it rests for half a minute.
    pub fn prepare_spare(&mut self) {
        const REST: Duration = Duration::from_secs(30);
        let playing = self.song.is_some() && !self.paused;
        let resting = self.spare_failed.is_some_and(|at| at.elapsed() < REST);
        if self.spare.is_some() || !playing || resting {
            return;
        }
        let started = Instant::now();
        match open_output() {
            Ok(spare) => {
                self.spare = Some(spare);
                self.spare_failed = None;
                log::debug!(
                    "audio output: a spare stream in {} ms",
                    started.elapsed().as_millis()
                );
            }
            Err(error) => {
                self.spare_failed = Some(Instant::now());
                log::info!("audio output: no spare stream: {error}");
            }
        }
    }

    fn restart_at(&mut self, at: Duration) -> Result<(), AudioError> {
        let Some(song) = &self.song else {
            return Ok(());
        };
        let clock = self.output.clock();
        let latency = Box::new(move || clock.latency());
        let (source, feed) = Playing::start(SongSource::open(&song.data, at)?, latency)?;
        // A fresh sink; dropping the old one stops it on the audio thread
        // without waiting.
        let sink = rodio::Sink::connect_new(&self.mixer);
        sink.set_volume(song.gain * self.volume);
        if self.paused {
            sink.pause();
        }
        sink.append(source);
        self.sink = sink;
        // The old source may wait in the mixer until the device next plays
        // (it rests while paused); its decoder stops now.
        if let Some(old) = self.feed.replace(feed) {
            old.let_go();
        }
        self.offset = at;
        Ok(())
    }

    pub fn pause(&mut self) {
        if self.paused {
            return;
        }
        self.paused = true;
        self.sink.pause();
        // No callbacks while paused: no CPU.
        self.output.pause();
    }

    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        self.output.resume();
        self.fed = true;
        self.sink.play();
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The listener's volume, 0 (silent) to 1 (full).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let gain = self.song.as_ref().map_or(1.0, |s| s.gain);
        self.sink.set_volume(gain * self.volume);
    }

    /// Jumps to `to` in the current song. Anywhere but the start needs the
    /// whole song, and waits for it (see [`Player::can_jump`]).
    pub fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        self.empty_the_device();
        // The song before the device, as in `play_song`.
        self.restart_at(to)?;
        if !self.paused && self.song.is_some() {
            self.output.resume();
            self.fed = true;
        }
        Ok(())
    }

    /// Whether a jump to `to` can be made without waiting: to the start,
    /// or anywhere once the whole song has arrived.
    pub fn can_jump(&self, to: Duration) -> bool {
        to.is_zero() || self.song.as_ref().is_none_or(|s| s.data.is_complete())
    }

    /// How far into the song playback is, as heard: what the device has
    /// been given, less what it holds before it plays (the latency it
    /// reports: about 0.2 s on Windows), so the time, the lyrics and the
    /// video's pictures are where the sound is, not that far ahead.
    /// Silence played while the download caught up does not count, and
    /// the silence after the song carries it to the end and no further.
    pub fn position(&self) -> Duration {
        let heard = self.heard();
        self.end().map_or(heard, |end| heard.min(end))
    }

    /// [`Player::position`], before it is held at the song's end.
    fn heard(&self) -> Duration {
        let silent = self.feed.as_ref().map_or(Duration::ZERO, |f| f.silence());
        let given = self.sink.get_pos().saturating_sub(silent);
        self.offset + given.saturating_sub(self.output.clock().latency())
    }

    /// Where the song ends, once its decoding has ended: a few seconds
    /// before it is heard, as the song is decoded ahead.
    fn end(&self) -> Option<Duration> {
        Some(self.offset + self.feed.as_ref()?.length()?)
    }

    /// How long until the song is finished ([`Player::finished`]), once
    /// where it ends is known; `None` before then. It counts down only
    /// while the song sounds.
    pub fn until_end(&self) -> Option<Duration> {
        Some((self.end()? + TAIL_MARGIN).saturating_sub(self.heard()))
    }

    /// The current song has stopped sounding: its end has been heard (the
    /// device holds nothing of it any more), or it broke off before its end
    /// (see [`Player::broke_off`]).
    pub fn finished(&self) -> bool {
        self.song.is_some() && self.sink.empty()
    }

    /// Why the current song stopped before its end: its download failed or
    /// stalled partway, so the decoder ran out of audio. `None` when the
    /// whole song arrived.
    pub fn broke_off(&self) -> Option<String> {
        let song = self.song.as_ref()?;
        if song.data.is_complete() {
            return None;
        }
        Some(
            song.data
                .failure()
                .unwrap_or_else(|| "the audio download stalled".into()),
        )
    }

    pub fn stop(&mut self) {
        self.song = None;
        self.sink = rodio::Sink::connect_new(&self.mixer);
        if let Some(old) = self.feed.take() {
            old.let_go();
        }
        self.offset = Duration::ZERO;
        // Nothing to play: the device rests (no callbacks, no CPU, and it
        // does not keep the computer awake) until the next song.
        self.output.pause();
        // A fresh stream now (about 0.1 s), while the next song is still
        // being fetched, rather than when it is ready.
        self.empty_the_device();
    }

    /// Keeps the output on a working device. Call a few times a second.
    /// Returns what changed, when something did.
    pub fn maintain(&mut self) -> Option<Maintenance> {
        if let Some(moved) = self.follow_default_by_name() {
            return Some(moved);
        }
        let mut note = None;
        let maintained = self.output.maintain();
        // The spare was opened on the device of then: after a move to
        // another, or letting the device go, it goes too.
        if matches!(
            maintained,
            Maintained::Reopened { .. } | Maintained::Released
        ) {
            self.spare = None;
        }
        match maintained {
            Maintained::Reopened { device, reason, .. } => {
                let remade = self
                    .made
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some((mixer, _rate)) = remade {
                    // Another rate or channel count: carry on from the same
                    // second on the new mixer.
                    let at = self.position();
                    self.mixer = mixer;
                    self.sink = rodio::Sink::connect_new(&self.mixer);
                    if let Err(error) = self.restart_at(at) {
                        note = Some(Maintenance::Problem(format!(
                            "Could not continue on {device}: {error}"
                        )));
                    }
                }
                if reason != fastframe_audio::Reason::Resumed {
                    log::warn!("audio output: now on {device} ({reason:?})");
                    note.get_or_insert(Maintenance::Moved(device));
                }
            }
            Maintained::Failed(error) => {
                log::warn!("audio output: {error}");
                note = Some(self.open_again(&error));
            }
            Maintained::Released | Maintained::Unchanged => {}
        }
        for error in self.output.take_errors() {
            log::warn!("audio output: {error}");
            if error.is_fatal() {
                note = Some(Maintenance::Problem(format!("Audio problem: {error}")));
            }
        }
        note
    }

    /// An output opened by name stays on its device: when the default
    /// output is another, sound moves there (by the same steps as
    /// [`open_output`]), and the song carries on from where it was.
    fn follow_default_by_name(&mut self) -> Option<Maintenance> {
        if !self.by_name || self.default_checked.elapsed() < DEFAULT_CHECK {
            return None;
        }
        self.default_checked = Instant::now();
        let default = default_output_name()?;
        if default == self.output.device_name() {
            return None;
        }
        log::warn!(
            "audio output: the default output is now {default} (was {})",
            self.output.device_name()
        );
        match open_output() {
            Ok(opened) => Some(self.carry_on_with(opened)),
            Err(error) => Some(Maintenance::Problem(format!(
                "Could not move the sound to {default}: {error}"
            ))),
        }
    }

    /// The audio library could not open its stream again (the stream
    /// failed, or the default output changed), and it tries again only the
    /// same way: the same device, or Windows' virtual default device when
    /// a device opened by name has gone (headphones unplugged), which can go
    /// on refusing ([`ways_to_open`]). So the output is opened as
    /// [`open_output`] does, on what is now the default, and the song
    /// carries on there. After a failure this waits [`DEFAULT_CHECK`]
    /// before it tries again.
    fn open_again(&mut self, error: &fastframe_audio::OpenError) -> Maintenance {
        let problem = || Maintenance::Problem(format!("No sound output: {error}"));
        if self
            .open_again_failed
            .is_some_and(|at| at.elapsed() < DEFAULT_CHECK)
        {
            return problem();
        }
        match open_output() {
            Ok(opened) => {
                self.open_again_failed = None;
                log::warn!(
                    "audio output: opened {} again by YTFast itself",
                    opened.output.device_name()
                );
                self.carry_on_with(opened)
            }
            Err(again) => {
                self.open_again_failed = Some(Instant::now());
                // Opened on the device that failed: no use for the next song.
                self.spare = None;
                log::warn!("audio output: could not open it again either: {again}");
                problem()
            }
        }
    }

    /// Makes `opened` the device's stream (in place of one on another
    /// device, or one that failed), and the song carries on from where it
    /// was.
    fn carry_on_with(&mut self, opened: OpenedOutput) -> Maintenance {
        let at = self.position();
        self.install(opened);
        self.spare = None;
        self.fed = false;
        let device = self.output.device_name().to_string();
        if self.song.is_some() {
            if let Err(error) = self.restart_at(at) {
                return Maintenance::Problem(format!("Could not continue on {device}: {error}"));
            }
            if !self.paused {
                self.output.resume();
                self.fed = true;
            }
        }
        Maintenance::Moved(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a sound device"]
    fn the_default_output_opens_by_its_name() {
        let name = default_output_name().expect("a default output");
        let opened = open_output_on(Device::Named(name.clone())).expect("it opens by name");
        assert_eq!(opened.output.device_name(), name);
        // As Windows' default too, the other way.
        let opened = open_output_on(Device::Default).expect("it opens as the default");
        assert_eq!(opened.output.device_name(), name);
        // And the usual way: by its name on Windows.
        let opened = open_output().expect("it opens");
        assert_eq!(opened.by_name, cfg!(windows));
        assert_eq!(opened.output.device_name(), name);
    }

    #[test]
    fn windows_opens_the_output_by_its_name_first() {
        let name = || Some("Speakers (Realtek(R) Audio)".to_string());
        let named = Device::Named("Speakers (Realtek(R) Audio)".into());
        if cfg!(windows) {
            assert_eq!(ways_to_open(name()), [named, Device::Default]);
        } else {
            assert_eq!(ways_to_open(name()), [Device::Default, named]);
        }
        // Without a name, only as the default.
        assert_eq!(ways_to_open(None), [Device::Default]);
    }

    /// The position is what has been heard, not what the device was given
    /// (on Windows that ran about 0.2 s ahead of the sound, and the lyrics
    /// lit early by as much).
    #[test]
    #[ignore = "needs a sound device"]
    fn the_position_is_what_is_heard() {
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        let started = Instant::now();
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(150));
            let ahead = player.position().as_secs_f64() - started.elapsed().as_secs_f64();
            assert!(
                ahead.abs() < 0.06,
                "{ahead:+.3} s from the time it has played"
            );
        }
        assert!(player.output.clock().latency() > Duration::ZERO);
        player.stop();
    }

    /// A song is finished once its end has been heard, not once the last of
    /// it went to the device: on Windows that was 0.2 s before (the
    /// latency), and the next song, started then, cut that much off the end
    /// of every song.
    #[test]
    #[ignore = "needs a sound device"]
    fn a_song_finishes_once_its_end_is_heard() {
        let length = decoded_length(&tone()).unwrap().as_secs_f64();
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        let started = Instant::now();
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        // The sound starts a moment after the song is given: by how much.
        std::thread::sleep(Duration::from_millis(500));
        let mut lags = Vec::new();
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(100));
            lags.push(started.elapsed().as_secs_f64() - player.position().as_secs_f64());
        }
        let lag = lags.iter().sum::<f64>() / lags.len() as f64;
        while !player.finished() {
            assert!(
                started.elapsed().as_secs_f64() < length + 2.0,
                "it finishes"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let finished = started.elapsed().as_secs_f64();
        let position = player.position().as_secs_f64();
        let latency = player.output.clock().latency().as_secs_f64();
        // When the end was heard, by the clock.
        let heard = lag + length;
        eprintln!(
            "song {length:.3} s, sound {lag:.3} s behind; finished at {finished:.3} s, its end \
             heard at {heard:.3} s; position then {position:.3} s; latency {latency:.3} s"
        );
        assert!(
            finished >= heard - 0.01,
            "finished {:.3} s before its end was heard",
            heard - finished
        );
        assert!(
            finished < heard + 0.06,
            "finished {:.3} s after its end was heard",
            finished - heard
        );
        assert!(
            (position - length).abs() < 0.02,
            "the position is the end: {position:.3} s"
        );
        player.stop();
    }

    /// The next song sounds at once: it is given to the stream before the
    /// stream starts. (Started first, the stream took its first 0.2 s as
    /// silence about every other time.)
    #[test]
    #[ignore = "needs a sound device"]
    fn the_next_song_sounds_at_once() {
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        for _ in 0..6 {
            // A spare opened while the song plays, as the audio thread does.
            std::thread::sleep(Duration::from_millis(150));
            player.prepare_spare();
            std::thread::sleep(Duration::from_millis(50));
            // A stream that sounded before it was first paused holds that
            // silence (about one in 50 opened on the owner's laptop): not
            // what this is about.
            let clean = player
                .spare
                .as_ref()
                .is_some_and(|spare| spare.output.clock().played().is_zero());
            // As at a song's end, or a song chosen: stop, then the next.
            player.stop();
            let started = Instant::now();
            player.play_song(SongData::complete(tone()), 1.0).unwrap();
            std::thread::sleep(Duration::from_millis(500));
            let lag = started.elapsed().as_secs_f64() - player.position().as_secs_f64();
            assert!(
                !clean || lag < 0.1,
                "the next song sounded {lag:.3} s after it was given"
            );
        }
        player.stop();
    }

    #[test]
    #[ignore = "needs a sound device"]
    fn a_song_carries_on_when_ytfast_opens_the_output_again() {
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        std::thread::sleep(Duration::from_millis(700));
        let before = player.position();
        // As after the audio library could not open its stream again.
        let moved = player.open_again(&fastframe_audio::OpenError::NoDevice);
        assert!(matches!(moved, Maintenance::Moved(_)), "{moved:?}");
        assert!(player.position() >= before, "{:?}", player.position());
        std::thread::sleep(Duration::from_millis(700));
        assert!(player.position() > before, "it plays on");
        assert!(!player.finished());
        player.stop();
    }

    fn tone() -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/tone_dash.m4a",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn decodes_youtube_style_fragmented_mp4() {
        let length = decoded_length(&tone()).unwrap();
        assert!(
            (length.as_secs_f64() - 4.0).abs() < 0.2,
            "decoded {length:?}"
        );
    }

    #[test]
    fn seeking_lands_exactly() {
        let bytes = SongData::complete(tone());
        let whole = SongSource::open(&bytes, Duration::ZERO).unwrap().count();
        let source = SongSource::open(&bytes, Duration::from_millis(2500)).unwrap();
        let per_second = f64::from(source.rate) * f64::from(source.channels);
        let rest = source.count();
        // What is left is the song minus 2.5 s, to within a few milliseconds.
        let jumped = (whole - rest) as f64 / per_second;
        assert!((jumped - 2.5).abs() < 0.01, "jumped {jumped} s");
    }

    #[test]
    fn seeking_past_the_end_is_not_a_crash() {
        let bytes = SongData::complete(tone());
        if let Ok(source) = SongSource::open(&bytes, Duration::from_secs(60)) {
            assert!(source.count() < 44_100);
        }
    }

    #[test]
    fn rejects_what_is_not_audio() {
        assert!(decoded_length(b"<html>not audio</html>").is_err());
    }

    #[test]
    fn turns_loud_songs_down_only() {
        assert_eq!(gain_for_loudness(None), 1.0);
        assert_eq!(gain_for_loudness(Some(-4.0)), 1.0);
        let gain = gain_for_loudness(Some(6.0));
        assert!((gain - 0.501).abs() < 0.01, "gain {gain}");
        assert_eq!(gain_for_loudness(Some(80.0)), 0.1);
    }

    #[test]
    fn plays_a_song_still_arriving() {
        // The song arrives in small parts while it is decoded: every sample
        // comes out, the same as from the whole file.
        let bytes = tone();
        let whole = SongSource::open(&SongData::complete(bytes.clone()), Duration::ZERO)
            .unwrap()
            .count();
        let arriving = SongData::new(Some(bytes.len() as u64));
        let feeder = {
            let data = Arc::clone(&arriving);
            std::thread::spawn(move || {
                for part in bytes.chunks(997) {
                    data.push_for_test(part);
                    std::thread::sleep(Duration::from_micros(200));
                }
                data.finish_for_test();
            })
        };
        let streamed = SongSource::open(&arriving, Duration::ZERO).unwrap().count();
        feeder.join().unwrap();
        assert_eq!(streamed, whole);
    }

    #[test]
    fn the_device_never_waits_for_the_download() {
        // Half the song arrives, then nothing for a second, then the rest.
        // The device gets sound, then silence (counted) instead of waiting,
        // then the rest of the sound: every sample, the same as from the
        // whole file.
        let bytes = tone();
        let whole = SongSource::open(&SongData::complete(bytes.clone()), Duration::ZERO)
            .unwrap()
            .count() as u64;
        let arriving = SongData::new(Some(bytes.len() as u64));
        let (first, rest) = bytes.split_at(bytes.len() / 2);
        arriving.push_for_test(first);
        let source = SongSource::open(&arriving, Duration::ZERO).unwrap();
        let (mut playing, feed) = Playing::start(source, Box::new(|| Duration::ZERO)).unwrap();
        let feeder = {
            let data = Arc::clone(&arriving);
            let rest = rest.to_vec();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                for part in rest.chunks(997) {
                    data.push_for_test(part);
                }
                data.finish_for_test();
            })
        };
        let mut played = 0u64;
        let mut slowest = Duration::ZERO;
        let mut silent_before = 0;
        loop {
            let asked = std::time::Instant::now();
            let sample = playing.next();
            slowest = slowest.max(asked.elapsed());
            if sample.is_none() {
                break;
            }
            played += 1;
            let silent = feed.silent.load(Ordering::Relaxed);
            if silent != silent_before {
                // Silence: as a device would, come back a moment later.
                silent_before = silent;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        feeder.join().unwrap();
        let silent = feed.silent.load(Ordering::Relaxed);
        assert!(silent > 0, "it played silence while the song arrived");
        // And after the song, the silence of its end (not counted).
        let tail = tail_samples(Duration::ZERO, playing.rate, playing.channels) as u64;
        assert_eq!(played - silent - tail, whole);
        assert!(
            slowest < Duration::from_millis(500),
            "the device waited {slowest:?}"
        );
        // The silence is taken off the position.
        let per_second = f64::from(playing.rate) * f64::from(playing.channels);
        assert!((feed.silence().as_secs_f64() - silent as f64 / per_second).abs() < 1e-9);
    }

    #[test]
    fn silence_follows_the_end_while_the_device_plays_it() {
        // A device that plays sound 0.2 s after it is given: the song ends
        // 0.22 s after its last sample (with the margin for the device's
        // steps), all of it silence, none of it taken off the position.
        let data = SongData::complete(tone());
        let whole = SongSource::open(&data, Duration::ZERO).unwrap().count();
        let source = SongSource::open(&data, Duration::ZERO).unwrap();
        let per_second = f64::from(source.rate) * f64::from(source.channels);
        let (playing, feed) =
            Playing::start(source, Box::new(|| Duration::from_millis(200))).unwrap();
        let played: Vec<f32> = playing.collect();
        // Silence while the decoder caught up, if any, is counted apart.
        let silent = feed.silent.load(Ordering::Relaxed) as usize;
        let tail = played.len() - silent - whole;
        assert!(
            (tail as f64 / per_second - 0.22).abs() < 0.001,
            "{tail} samples"
        );
        assert!(played[played.len() - tail..].iter().all(|&s| s == 0.0));
        // Where the song ends was known once it was decoded.
        let length = feed.length().expect("known at the end");
        assert!((length.as_secs_f64() - whole as f64 / per_second).abs() < 1e-6);
    }

    #[test]
    fn a_song_let_go_stops_without_the_silence() {
        // Skipped, or replaced by a jump: no silence after it.
        let data = SongData::complete(tone());
        let source = SongSource::open(&data, Duration::ZERO).unwrap();
        let per_second = source.rate as usize * usize::from(source.channels);
        let (mut playing, feed) =
            Playing::start(source, Box::new(|| Duration::from_millis(200))).unwrap();
        assert!(playing.next().is_some());
        feed.let_go();
        // At most what is left of the packet it was playing.
        let rest = playing.count();
        assert!(rest < per_second / 10, "{rest} samples after it was let go");
        assert_eq!(feed.length(), None, "a song let go has no end");
    }

    #[test]
    fn letting_a_song_go_ends_its_decoding() {
        let data = SongData::complete(tone());
        let source = SongSource::open(&data, Duration::ZERO).unwrap();
        let (mut playing, _feed) = Playing::start(source, Box::new(|| Duration::ZERO)).unwrap();
        assert!(playing.next().is_some());
        // The four-second song is decoded two seconds ahead, then the
        // decoder waits for room, holding the song.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(Arc::strong_count(&data), 2);
        drop(playing);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&data) > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(Arc::strong_count(&data), 1, "the decoder let the song go");
    }
}
