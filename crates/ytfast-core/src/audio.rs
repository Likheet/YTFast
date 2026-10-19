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
//!
//! The next song can be queued to follow on the same stream, with nothing
//! between the two (gapless) or with the two overlapping (a crossfade:
//! [`Playing`]); and everything plays through the equalizer
//! ([`crate::eq`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// The song's bytes, to tell its end from a download that broke off.
    data: Arc<SongData>,
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
            data: Arc::clone(data),
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

    /// Decoding stopped short of the song's end: its download did not
    /// arrive whole (it failed or stalled).
    fn broke_off(&self) -> bool {
        !self.data.is_complete()
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
    /// Its samples a second and channels: one song follows another without
    /// a break only in the same format ([`Player::queue_next`]).
    format: (u32, u16),
    /// Decoding ended where the download broke off, short of the song's
    /// end: no song queued follows it ([`Playing`]), and the player stops
    /// ([`Player::broke_off`]). Stored before `length`.
    broke_off: AtomicBool,
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
    fn end(&self, queue: &mut Queue, broke_off: bool) {
        queue.ended = true;
        self.broke_off.store(broke_off, Ordering::Release);
        if !queue.let_go {
            self.length.store(queue.pushed, Ordering::Release);
        }
    }

    /// Decoding ended where the download broke off ([`Feed::broke_off`]).
    fn broke_off(&self) -> bool {
        self.broke_off.load(Ordering::Acquire)
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
    struct Ended<'a>(&'a Feed, Arc<SongData>);
    impl Drop for Ended<'_> {
        fn drop(&mut self) {
            self.0.end(&mut self.0.lock(), !self.1.is_complete());
        }
    }
    let _ended = Ended(feed, Arc::clone(&song.data));
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
/// the last song ends ([`Playing`]).
type Latency = Box<dyn Fn() -> Duration + Send>;

/// Starts decoding `song` into a feed of its own, keeping `ahead` samples
/// ready, on a thread of its own. With `first`, its first half second is
/// decoded here, so the device's first request is answered with sound (a
/// song queued to follow another has seconds for its decoder to start).
fn feed_for(mut song: SongSource, ahead: usize, first: bool) -> Result<Arc<Feed>, AudioError> {
    let (rate, channels) = (song.rate, song.channels);
    let per_second = rate as usize * usize::from(channels);
    let mut queue = Queue::default();
    while first && queue.samples < per_second / 2 {
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
        format: (rate, channels),
        broke_off: AtomicBool::new(ended && song.broke_off()),
    });
    if !ended {
        let decoding = Arc::clone(&feed);
        std::thread::Builder::new()
            .name("ytfast-decoder".into())
            .spawn(move || decode_ahead(song, &decoding, ahead))
            .map_err(|e| AudioError::Device(format!("could not start decoding: {e}")))?;
    }
    Ok(feed)
}

/// One song's sound in a [`Playing`]: its feed, and how loud it plays
/// (YouTube's loudness, [`gain_for_loudness`]).
struct Voice {
    feed: Arc<Feed>,
    chunk: Vec<f32>,
    at: usize,
    gain: f32,
    /// Samples of the song given so far (sound only).
    taken: u64,
}

/// What a [`Voice`] gives next.
enum Sample {
    Sound(f32),
    /// Nothing decoded yet: the download is behind.
    Waiting,
    /// All of the song has been given: it ended, or broke off.
    End,
    /// Nobody plays the song any more.
    LetGo,
}

impl Voice {
    fn new(feed: Arc<Feed>, gain: f32) -> Self {
        Self {
            feed,
            chunk: Vec::new(),
            at: 0,
            gain,
            taken: 0,
        }
    }

    /// The next sample. Never waits.
    fn next(&mut self) -> Sample {
        loop {
            if let Some(&sample) = self.chunk.get(self.at) {
                self.at += 1;
                self.taken += 1;
                return Sample::Sound(sample * self.gain);
            }
            match self.feed.take() {
                Taken::Sound(chunk) => {
                    self.chunk = chunk;
                    self.at = 0;
                }
                Taken::Waiting => return Sample::Waiting,
                Taken::End => return Sample::End,
                Taken::LetGo => return Sample::LetGo,
            }
        }
    }

    /// Samples of the song still to give, once its decoding has ended.
    fn left(&self) -> Option<u64> {
        let length = self.feed.length.load(Ordering::Acquire);
        (length != u64::MAX).then(|| length.saturating_sub(self.taken))
    }

    /// Some of the song is decoded, ready to sound at once.
    fn ready(&self) -> bool {
        self.at < self.chunk.len() || {
            let queue = self.feed.lock();
            queue.samples > 0 || queue.ended
        }
    }
}

/// The song queued to follow the one playing ([`Player::queue_next`]), as
/// the device's thread takes it: its voice, decoding ahead already, and
/// how long the two overlap (0: one follows the other at once).
struct Next {
    id: u64,
    voice: Voice,
    /// Samples the crossfade lasts, in whole frames; 0 for none.
    fade: u64,
}

/// What a [`Playing`] and the [`Player`] share: the song queued to follow,
/// put here by the player and taken by the device's thread when it begins;
/// how much sound the device has been given; and where each song after the
/// first began in it.
#[derive(Default)]
struct Chain {
    next: Mutex<Option<Next>>,
    /// Samples given to the device (all channels, silence included).
    given: AtomicU64,
    /// Each song after the first: its id, and `given` at its first sample.
    starts: Mutex<Vec<(u64, u64)>>,
}

impl Chain {
    fn next(&self) -> MutexGuard<'_, Option<Next>> {
        self.next.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the song `id` began in the sound given, once it has.
    fn start_of(&self, id: u64) -> Option<u64> {
        self.starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(started, _)| *started == id)
            .map(|(_, at)| *at)
    }

    fn began(&self, id: u64, at: u64) {
        self.starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id, at));
    }

    /// Lets the song queued to follow go, if it has not begun.
    fn let_next_go(&self) {
        if let Some(next) = self.next().take() {
            next.voice.feed.let_go();
        }
    }
}

/// The song before, fading out under the one beginning, in a crossfade:
/// its voice, the samples faded so far, and the samples the fade lasts
/// (what was left of it when the fade began).
struct Fading {
    voice: Voice,
    done: u64,
    of: u64,
}

/// The songs as the device plays them, as one rodio source.
///
/// The device must never wait (the sound would stop, and pausing or moving
/// to other headphones would wait too), so each song is decoded on a thread
/// of its own ([`decode_ahead`]), a little ahead. When the decoder is
/// behind, because the download is, silence plays, and is counted so the
/// song's position (and the lyrics) do not run ahead. Dropping it ends the
/// decoding threads.
///
/// A song queued to follow ([`Chain::next`]) begins the moment this one's
/// last sample is given, with nothing between (gapless); or, with a
/// crossfade, that long before this one's end, the two sounding together
/// as one fades in and the other out. Each song plays at its own gain.
///
/// After the last song's end, silence plays for as long as the device takes
/// to play what it holds (its latency: about 0.2 s on Windows), and only
/// then does the source end. So the song is finished ([`Player::finished`])
/// once its end has been heard, and the next song, which empties the device,
/// cuts off nothing of it.
struct Playing {
    voice: Voice,
    fading: Option<Fading>,
    /// Samples of silence still to play.
    silence: usize,
    /// Samples of silence still to play after the last song's end; `None`
    /// before it.
    tail: Option<usize>,
    latency: Latency,
    channels: u16,
    rate: u32,
    chain: Arc<Chain>,
    /// Samples given (as [`Chain::given`]).
    given: u64,
}

impl Playing {
    fn new(voice: Voice, latency: Latency, chain: Arc<Chain>) -> Self {
        let (rate, channels) = voice.feed.format;
        Self {
            voice,
            fading: None,
            silence: 0,
            tail: None,
            latency,
            channels,
            rate,
            chain,
            given: 0,
        }
    }

    /// `song` from its start, at its own gain, on a chain of its own (the
    /// tests').
    #[cfg(test)]
    fn start(song: SongSource, latency: Latency) -> Result<(Self, Arc<Feed>), AudioError> {
        let per_second = song.rate as usize * usize::from(song.channels);
        let feed = feed_for(song, per_second * AHEAD_SECONDS, true)?;
        let playing = Self::new(Voice::new(Arc::clone(&feed), 1.0), latency, Arc::default());
        Ok((playing, feed))
    }

    /// At a frame's start: with a crossfade wanted, the song queued begins
    /// once this one's end is that near, and has sound ready. Not where a
    /// download broke off: the player stops there.
    fn begin_fade(&mut self) {
        if self.fading.is_some() || self.tail.is_some() {
            return;
        }
        let Some(left) = self.voice.left() else {
            return;
        };
        if self.voice.feed.broke_off() {
            return;
        }
        let mut next = self.chain.next();
        let due = next
            .as_ref()
            .is_some_and(|n| n.fade > 0 && left > 0 && left <= n.fade && n.voice.ready());
        if !due {
            return;
        }
        let Some(next) = next.take() else {
            return;
        };
        let channels = u64::from(self.channels);
        let before = std::mem::replace(&mut self.voice, next.voice);
        self.fading = Some(Fading {
            voice: before,
            done: 0,
            of: (left - left % channels).max(channels),
        });
        self.chain.began(next.id, self.given);
    }

    /// The song ended (its last sample given): the song queued begins at
    /// once, when there is one. Not when its download broke off: that song
    /// is not skipped, the player stops ([`Player::broke_off`]).
    fn follow_on(&mut self) -> bool {
        if self.voice.feed.broke_off() {
            return false;
        }
        let Some(next) = self.chain.next().take() else {
            return false;
        };
        let before = std::mem::replace(&mut self.voice, next.voice);
        before.feed.let_go();
        if let Some(fading) = self.fading.take() {
            fading.voice.feed.let_go();
        }
        self.chain.began(next.id, self.given);
        true
    }

    /// `sample`, the newer song's, over the song fading out under it in a
    /// crossfade, each at its share of the fade: equal power, so the two
    /// together hold the loudness through it.
    fn mix(&mut self, sample: f32) -> f32 {
        let Some(fading) = &mut self.fading else {
            return sample;
        };
        let channels = u64::from(self.channels);
        let through = ((fading.done / channels) as f32 / (fading.of / channels).max(1) as f32)
            .min(1.0)
            * std::f32::consts::FRAC_PI_2;
        let under = match fading.voice.next() {
            Sample::Sound(under) => under,
            // Its end came, or its sound is late: the newer song alone.
            _ => 0.0,
        };
        fading.done += 1;
        let mixed = sample * through.sin() + under * through.cos();
        if fading.done >= fading.of
            && let Some(gone) = self.fading.take()
        {
            gone.voice.feed.let_go();
        }
        mixed
    }

    fn give(&mut self, sample: f32) -> Option<f32> {
        self.given += 1;
        self.chain.given.store(self.given, Ordering::Relaxed);
        Some(sample)
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.voice.feed.let_go();
        if let Some(fading) = &self.fading {
            fading.voice.feed.let_go();
        }
        self.chain.let_next_go();
    }
}

impl Iterator for Playing {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if self.silence > 0 {
                self.silence -= 1;
                let sample = self.mix(0.0);
                return self.give(sample);
            }
            if self.given.is_multiple_of(u64::from(self.channels)) {
                self.begin_fade();
            }
            match self.voice.next() {
                Sample::Sound(sample) => {
                    let sample = self.mix(sample);
                    return self.give(sample);
                }
                Sample::Waiting => {
                    // Whole frames, so the channels stay in step.
                    let samples = SILENCE_FRAMES * usize::from(self.channels);
                    self.voice
                        .feed
                        .silent
                        .fetch_add(samples as u64, Ordering::Relaxed);
                    self.silence = samples;
                }
                Sample::End => {
                    // A song queued to follow: at once, nothing between.
                    if self.tail.is_none() && self.follow_on() {
                        continue;
                    }
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
                Sample::LetGo => return None,
            }
        }
    }
}

impl rodio::Source for Playing {
    fn current_span_len(&self) -> Option<usize> {
        // One format for every song (a song in another follows the old way:
        // `Player::queue_next`).
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
/// one, through the equalizer ([`crate::eq`]). Adapted from Spotifast's
/// `src/sink.rs` (MIT, Copyright (c) 2026 Carmine Paolino). A stream in the
/// format the mixer already has keeps it, so a reopen on another device
/// carries on from the same sample.
struct MixerRender {
    source: Option<rodio::mixer::MixerSource>,
    format: (u32, u16),
    made: MixerSlot,
    eq: crate::eq::Processor,
}

impl Render for MixerRender {
    fn configure(&mut self, sample_rate: u32, channels: u16) {
        self.eq.configure(sample_rate, channels);
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
        self.eq.process(out);
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

impl OpenedOutput {
    /// The stream holds silence that would play before its first song.
    /// fastframe-audio opens a stream playing, and YTFast pauses it at
    /// once, but now and then (a few times in a hundred on the owner's
    /// laptop) the device first takes a step of sound from it, up to 0.2 s
    /// on Windows, and keeps it through the pause. Only for a stream not
    /// resumed since it opened, once its pause has taken effect
    /// ([`PAUSE_SETTLES`]).
    fn holds_silence(&self) -> bool {
        !self.output.clock().played().is_zero()
    }
}

/// How long a stream just opened is given for its pause to take effect,
/// before it is asked whether it holds silence.
const PAUSE_SETTLES: Duration = Duration::from_millis(30);

/// Opens a stream with `open`, and again (three times in all, at most)
/// while it holds silence ([`OpenedOutput::holds_silence`]). It waits a
/// moment for each stream's pause, so it is for a stream nothing waits for
/// yet: the player's first, opened as YTFast starts.
fn quietly(
    mut open: impl FnMut() -> Result<OpenedOutput, AudioError>,
) -> Result<OpenedOutput, AudioError> {
    let mut opened = open()?;
    for _ in 1..3 {
        std::thread::sleep(PAUSE_SETTLES);
        if !opened.holds_silence() {
            break;
        }
        log::debug!("audio output: a new stream held silence; opening another");
        opened = open()?;
    }
    Ok(opened)
}

/// Opens the default output, paused (the device rests until there is a
/// song to play), the ways [`ways_to_open`] lists, until one works. Its
/// sound goes through the equalizer `eq` sets.
fn open_output(eq: &Arc<crate::eq::Shared>) -> Result<OpenedOutput, AudioError> {
    let mut first_error = None;
    for device in ways_to_open(default_output_name()) {
        let by_name = matches!(device, Device::Named(_));
        match open_output_on(device.clone(), eq) {
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
fn open_output_on(device: Device, eq: &Arc<crate::eq::Shared>) -> Result<OpenedOutput, AudioError> {
    let made = MixerSlot::default();
    let render = MixerRender {
        source: None,
        format: (0, 0),
        made: Arc::clone(&made),
        eq: crate::eq::Processor::new(Arc::clone(eq)),
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
///
/// A song queued to follow ([`Player::queue_next`]) plays on the same
/// stream, on from the last sample of the one before or fading over its
/// end ([`Playing`]): nothing between them. It becomes the player's song
/// once it is heard ([`Player::take_moved_on`]).
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
    /// The song heard.
    song: Option<Loaded>,
    /// Its feed: its silence is taken off the position, and its end is its
    /// length.
    feed: Option<Arc<Feed>>,
    /// Where the song heard started (a jump).
    offset: Duration,
    /// What the current sink plays from, shared with it ([`Chain`]).
    chain: Arc<Chain>,
    /// Where the song heard began in the sound given ([`Chain::given`]).
    began_at: u64,
    /// The song queued to follow, until it is heard.
    queued: Option<Queued>,
    /// Numbers the songs queued.
    queued_ids: u64,
    /// How long a crossfade lasts: songs are decoded that far ahead (and a
    /// second more), so where one ends is known before its fade begins.
    crossfade: Duration,
    paused: bool,
    /// The listener's volume, 0 to 1, on top of the song's loudness gain.
    volume: f32,
    /// The equalizer's settings, which every stream opened plays through.
    eq: Arc<crate::eq::Shared>,
    /// The output was opened by the default's name ([`OpenedOutput`]), so
    /// `maintain` follows the default itself, looking every
    /// [`DEFAULT_CHECK`].
    by_name: bool,
    default_checked: Instant,
    /// When [`Player::open_again`] last failed: it waits [`DEFAULT_CHECK`]
    /// before it tries again.
    open_again_failed: Option<Instant>,
}

/// The song queued to follow the one playing, as the player keeps it.
struct Queued {
    id: u64,
    data: Arc<SongData>,
    gain: f32,
    fade: Duration,
    feed: Arc<Feed>,
}

impl Player {
    pub fn open() -> Result<Self, AudioError> {
        let eq = Arc::new(crate::eq::Shared::default());
        let OpenedOutput {
            output,
            made,
            mixer,
            by_name,
        } = quietly(|| open_output(&eq))?;
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
            chain: Arc::default(),
            began_at: 0,
            queued: None,
            queued_ids: 0,
            crossfade: Duration::ZERO,
            paused: false,
            volume: 1.0,
            eq,
        })
    }

    /// The equalizer's new settings, for the sound from the next moment on.
    pub fn set_equalizer(&self, settings: crate::eq::Equalizer) {
        self.eq.set(settings);
    }

    /// How long crossfades last (0: none), for the songs started from now
    /// on: each is decoded that far ahead.
    pub fn set_crossfade(&mut self, fade: Duration) {
        self.crossfade = fade.min(Duration::from_secs(12));
    }

    /// Samples decoded ahead of the device, for a song of `per_second`
    /// samples a second: two seconds, or the crossfade and a second more.
    fn ahead(&self, per_second: usize) -> usize {
        let seconds = (self.crossfade.as_secs_f64().ceil() as usize + 1).max(AHEAD_SECONDS);
        per_second * seconds
    }

    /// Queues `data` to follow the song playing, with nothing between them,
    /// or with the two overlapping for `fade` (a crossfade). It must be in
    /// the song's format; one in another follows the old way (the song
    /// ends, then the next is played). Any song queued before is let go.
    pub fn queue_next(
        &mut self,
        data: Arc<SongData>,
        gain: f32,
        fade: Duration,
    ) -> Result<(), AudioError> {
        self.unqueue();
        let format = self
            .feed
            .as_ref()
            .map(|feed| feed.format)
            .ok_or_else(|| AudioError::Device("no song plays".into()))?;
        let source = SongSource::open(&data, Duration::ZERO)?;
        if (source.rate, source.channels) != format {
            return Err(AudioError::Decode(format!(
                "{} Hz in {} channels after {} Hz in {}",
                source.rate, source.channels, format.0, format.1
            )));
        }
        let per_second = format.0 as usize * usize::from(format.1);
        let feed = feed_for(source, self.ahead(per_second), false)?;
        self.queued_ids += 1;
        let id = self.queued_ids;
        let channels = u64::from(format.1);
        let samples = (fade.as_secs_f64() * per_second as f64) as u64;
        *self.chain.next() = Some(Next {
            id,
            voice: Voice::new(Arc::clone(&feed), gain),
            fade: samples - samples % channels,
        });
        self.queued = Some(Queued {
            id,
            data,
            gain,
            fade,
            feed,
        });
        Ok(())
    }

    /// Lets the song queued to follow go. One already sounding (its start
    /// not heard yet) plays on until the next song is played.
    pub fn unqueue(&mut self) {
        self.chain.let_next_go();
        self.queued = None;
    }

    /// The song queued is heard now: it becomes the player's song (its
    /// position, its end, a jump in it). `true` once, when that happens.
    pub fn take_moved_on(&mut self) -> bool {
        let Some(at) = self
            .queued
            .as_ref()
            .and_then(|queued| self.chain.start_of(queued.id))
        else {
            return false;
        };
        if self.heard_given() < at {
            return false;
        }
        let Some(queued) = self.queued.take() else {
            return false;
        };
        self.song = Some(Loaded {
            data: queued.data,
            gain: queued.gain,
        });
        // The song before may still be fading out: the device's thread lets
        // it go at the fade's end.
        self.feed = Some(queued.feed);
        self.offset = Duration::ZERO;
        self.began_at = at;
        true
    }

    /// The sound given that has been heard, in samples: what the device was
    /// given, less what it holds (its latency).
    fn heard_given(&self) -> u64 {
        let latency = self.output.clock().latency().as_secs_f64();
        let held = (latency * self.per_second() as f64) as u64;
        self.chain
            .given
            .load(Ordering::Relaxed)
            .saturating_sub(held)
    }

    /// Samples a second of the song heard (all channels).
    fn per_second(&self) -> u64 {
        self.feed.as_ref().map_or(1, |feed| feed.per_second.max(1))
    }

    /// How long until the song queued is heard, once that is known: it has
    /// begun, or the end of the song playing is known (its fade before).
    pub fn until_next(&self) -> Option<Duration> {
        let queued = self.queued.as_ref()?;
        let per_second = self.per_second() as f64;
        if let Some(at) = self.chain.start_of(queued.id) {
            let left = at.saturating_sub(self.heard_given());
            return Some(Duration::from_secs_f64(left as f64 / per_second));
        }
        let begins = self.end()?.saturating_sub(queued.fade);
        Some(begins.saturating_sub(self.heard()))
    }

    pub fn device_name(&self) -> &str {
        self.output.device_name()
    }

    /// Starts a new song from the beginning, playing as its bytes arrive.
    /// `gain` is from [`gain_for_loudness`].
    pub fn play_song(&mut self, data: Arc<SongData>, gain: f32) -> Result<(), AudioError> {
        self.unqueue();
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
            None => open_output(&self.eq),
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
    /// and there is none (about 0.15 s; call it when nothing waits), or
    /// the spare holds silence. After a failure it rests for half a minute.
    pub fn prepare_spare(&mut self) {
        const REST: Duration = Duration::from_secs(30);
        // A spare that holds silence would play it before the next song.
        // Asked at every call: just opened, its pause takes a moment.
        if self.spare.as_ref().is_some_and(OpenedOutput::holds_silence) {
            log::debug!("audio output: the spare stream held silence; opening another");
            self.spare = None;
        }
        let playing = self.song.is_some() && !self.paused;
        let resting = self.spare_failed.is_some_and(|at| at.elapsed() < REST);
        if self.spare.is_some() || !playing || resting {
            return;
        }
        let started = Instant::now();
        match open_output(&self.eq) {
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

    /// Plays the song heard from `at`, on a sink of its own (a new song, a
    /// jump, a new device), the song queued to follow queued again after it.
    fn restart_at(&mut self, at: Duration) -> Result<(), AudioError> {
        let Some(song) = &self.song else {
            return Ok(());
        };
        let (data, gain) = (Arc::clone(&song.data), song.gain);
        let clock = self.output.clock();
        let latency = Box::new(move || clock.latency());
        let source = SongSource::open(&data, at)?;
        let per_second = source.rate as usize * usize::from(source.channels);
        let feed = feed_for(source, self.ahead(per_second), true)?;
        let chain = Arc::new(Chain::default());
        let playing = Playing::new(
            Voice::new(Arc::clone(&feed), gain),
            latency,
            Arc::clone(&chain),
        );
        // A fresh sink; dropping the old one stops it on the audio thread
        // without waiting.
        let sink = rodio::Sink::connect_new(&self.mixer);
        sink.set_volume(self.volume);
        if self.paused {
            sink.pause();
        }
        sink.append(playing);
        self.sink = sink;
        // The old source may wait in the mixer until the device next plays
        // (it rests while paused); its decoders stop now.
        if let Some(old) = self.feed.replace(feed) {
            old.let_go();
        }
        std::mem::replace(&mut self.chain, chain).let_next_go();
        self.offset = at;
        self.began_at = 0;
        if let Some(queued) = self.queued.take()
            && let Err(error) = self.queue_next(queued.data, queued.gain, queued.fade)
        {
            log::debug!("audio: the song queued could not follow any more: {error}");
        }
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

    /// The listener's volume, 0 (silent) to 1 (full). Each song's own gain
    /// is played by its voice ([`Voice`]).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.sink.set_volume(self.volume);
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

    /// [`Player::position`], before it is held at the song's end: the sound
    /// given since the song heard began in it ([`Chain::given`]).
    fn heard(&self) -> Duration {
        let silent = self.feed.as_ref().map_or(Duration::ZERO, |f| f.silence());
        let given = self
            .chain
            .given
            .load(Ordering::Relaxed)
            .saturating_sub(self.began_at);
        let given = Duration::from_secs_f64(given as f64 / self.per_second() as f64);
        self.offset
            + given
                .saturating_sub(silent)
                .saturating_sub(self.output.clock().latency())
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
        self.unqueue();
        self.sink = rodio::Sink::connect_new(&self.mixer);
        if let Some(old) = self.feed.take() {
            old.let_go();
        }
        self.chain = Arc::default();
        self.offset = Duration::ZERO;
        self.began_at = 0;
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
        match open_output(&self.eq) {
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
        match open_output(&self.eq) {
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
        let opened =
            open_output_on(Device::Named(name.clone()), &Arc::default()).expect("it opens by name");
        assert_eq!(opened.output.device_name(), name);
        // As Windows' default too, the other way.
        let opened =
            open_output_on(Device::Default, &Arc::default()).expect("it opens as the default");
        assert_eq!(opened.output.device_name(), name);
        // And the usual way: by its name on Windows.
        let opened = open_output(&Arc::default()).expect("it opens");
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
    /// stream starts, and the spare holds no silence. (Started first, the
    /// stream took its first 0.2 s as silence about every other time.)
    #[test]
    #[ignore = "needs a sound device"]
    fn the_next_song_sounds_at_once() {
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        for _ in 0..6 {
            // A spare opened while the song plays, and asked again at the
            // next looks, as the audio thread does.
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(100));
                player.prepare_spare();
            }
            // As at a song's end, or a song chosen: stop, then the next.
            player.stop();
            let started = Instant::now();
            player.play_song(SongData::complete(tone()), 1.0).unwrap();
            std::thread::sleep(Duration::from_millis(500));
            let lag = started.elapsed().as_secs_f64() - player.position().as_secs_f64();
            assert!(
                lag < 0.1,
                "the next song sounded {lag:.3} s after it was given"
            );
        }
        player.stop();
    }

    /// A song queued to follow is heard on the same stream the moment the
    /// first one's end is (gapless), or with a crossfade that long before
    /// it; it becomes the player's song then, its position from 0, and it
    /// plays on with the clock.
    #[test]
    #[ignore = "needs a sound device"]
    fn a_queued_song_is_heard_as_the_first_ends() {
        let length = decoded_length(&tone()).unwrap().as_secs_f64();
        for fade in [0.0, 1.0] {
            let mut player = Player::open().expect("a sound device");
            player.set_volume(0.0);
            player.set_crossfade(Duration::from_secs_f64(fade));
            let started = Instant::now();
            player.play_song(SongData::complete(tone()), 1.0).unwrap();
            player
                .queue_next(
                    SongData::complete(tone()),
                    1.0,
                    Duration::from_secs_f64(fade),
                )
                .unwrap();
            // How far the sound is behind the clock.
            std::thread::sleep(Duration::from_millis(500));
            let mut lags = Vec::new();
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(100));
                lags.push(started.elapsed().as_secs_f64() - player.position().as_secs_f64());
            }
            let lag = lags.iter().sum::<f64>() / lags.len() as f64;
            while !player.take_moved_on() {
                assert!(
                    started.elapsed().as_secs_f64() < length + 2.0,
                    "it moves on"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let moved = started.elapsed().as_secs_f64();
            let due = lag + length - fade;
            let position = player.position().as_secs_f64();
            eprintln!(
                "fade {fade} s: moved on at {moved:.3} s, due at {due:.3} s; the new song at \
                 {position:.3} s"
            );
            assert!(
                (moved - due).abs() < 0.06,
                "moved on {:+.3} s from when it was heard",
                moved - due
            );
            assert!(position < 0.06, "the new song at {position:.3} s");
            assert!(!player.finished());
            std::thread::sleep(Duration::from_millis(1500));
            let ran = started.elapsed().as_secs_f64() - moved;
            let position = player.position().as_secs_f64();
            assert!(
                (position - ran).abs() < 0.06,
                "{position:.3} s after {ran:.3} s"
            );
            player.stop();
        }
    }

    /// A spare that holds silence is not kept: another takes its place.
    #[test]
    #[ignore = "needs a sound device"]
    fn a_spare_that_holds_silence_is_not_kept() {
        let mut player = Player::open().expect("a sound device");
        player.set_volume(0.0);
        player.play_song(SongData::complete(tone()), 1.0).unwrap();
        player.prepare_spare();
        // As if the device took a step of sound before its first pause.
        let spare = player.spare.as_mut().expect("a spare");
        spare.output.resume();
        std::thread::sleep(Duration::from_millis(50));
        spare.output.pause();
        std::thread::sleep(PAUSE_SETTLES);
        assert!(spare.holds_silence());
        // The audio thread's next looks.
        for _ in 0..3 {
            player.prepare_spare();
            std::thread::sleep(PAUSE_SETTLES);
        }
        let spare = player.spare.as_ref().expect("another spare");
        assert!(!spare.holds_silence(), "the spare that held silence stayed");
        player.stop();
    }

    /// The player's first stream is opened again when it holds silence.
    #[test]
    #[ignore = "needs a sound device"]
    fn the_first_stream_is_opened_again_when_it_holds_silence() {
        let mut opens = 0;
        let opened = quietly(|| {
            opens += 1;
            let mut opened = open_output(&Arc::default())?;
            if opens == 1 {
                // As if the device took a step of sound before its pause.
                opened.output.resume();
                std::thread::sleep(Duration::from_millis(50));
                opened.output.pause();
            }
            Ok(opened)
        })
        .expect("a sound device");
        assert!(opens > 1, "it was not opened again");
        std::thread::sleep(PAUSE_SETTLES);
        assert!(!opened.holds_silence());
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

    /// A song decoded whole into a feed of its own, before it plays (so the
    /// tests' pulling, faster than any device, never finds it behind).
    fn decoded(data: &Arc<SongData>) -> (Vec<f32>, Arc<Feed>) {
        let samples: Vec<f32> = SongSource::open(data, Duration::ZERO).unwrap().collect();
        let source = SongSource::open(data, Duration::ZERO).unwrap();
        let per_second = source.rate as usize * usize::from(source.channels);
        let feed = feed_for(source, per_second * 30, false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while feed.length().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(feed.length().is_some(), "it was decoded");
        (samples, feed)
    }

    /// A song queued to follow sounds from the sample after the last of the
    /// one before, nothing between them (gapless), at its own gain; the
    /// silence of the end comes after the last song only.
    #[test]
    fn a_queued_song_follows_with_nothing_between() {
        let data = SongData::complete(tone());
        let (first, first_feed) = decoded(&data);
        let (second, second_feed) = decoded(&data);
        let chain = Arc::new(Chain::default());
        let playing = Playing::new(
            Voice::new(first_feed, 1.0),
            Box::new(|| Duration::ZERO),
            Arc::clone(&chain),
        );
        *chain.next() = Some(Next {
            id: 7,
            voice: Voice::new(second_feed, 0.5),
            fade: 0,
        });
        let (rate, channels) = (playing.rate, playing.channels);
        let played: Vec<f32> = playing.collect();
        assert_eq!(chain.start_of(7), Some(first.len() as u64));
        assert_eq!(&played[..first.len()], &first[..]);
        let halved: Vec<f32> = second.iter().map(|s| s * 0.5).collect();
        assert_eq!(
            &played[first.len()..first.len() + second.len()],
            &halved[..]
        );
        let tail = tail_samples(Duration::ZERO, rate, channels);
        assert_eq!(played.len(), first.len() + second.len() + tail);
        assert_eq!(
            chain.given.load(Ordering::Relaxed),
            played.len() as u64,
            "every sample given is counted"
        );
    }

    /// With a crossfade, the song queued begins that long before the end of
    /// the one before, the two sounding together: the newer rising and the
    /// older falling, each sample their equal-power mix.
    #[test]
    fn a_crossfade_overlaps_the_end_and_the_start() {
        let data = SongData::complete(tone());
        let (first, first_feed) = decoded(&data);
        let (second, second_feed) = decoded(&data);
        let chain = Arc::new(Chain::default());
        let playing = Playing::new(
            Voice::new(first_feed, 1.0),
            Box::new(|| Duration::ZERO),
            Arc::clone(&chain),
        );
        let channels = usize::from(playing.channels);
        let per_second = playing.rate as usize * channels;
        let fade = per_second; // one second
        *chain.next() = Some(Next {
            id: 3,
            voice: Voice::new(second_feed, 1.0),
            fade: fade as u64,
        });
        let played: Vec<f32> = playing.collect();
        let begins = first.len() - fade;
        assert_eq!(chain.start_of(3), Some(begins as u64));
        // Before the fade: the first song alone.
        assert_eq!(&played[..begins], &first[..begins]);
        // Through it: the two mixed, at each frame's share of the fade.
        for (i, (&mixed, (&older, &newer))) in played[begins..first.len()]
            .iter()
            .zip(first[begins..].iter().zip(&second))
            .enumerate()
        {
            let through =
                (i / channels) as f32 / (fade / channels) as f32 * std::f32::consts::FRAC_PI_2;
            let wanted = newer * through.sin() + older * through.cos();
            assert!(
                (mixed - wanted).abs() < 1e-6,
                "sample {i}: {mixed} for {wanted}"
            );
        }
        // After it: the second song alone, from where the fade left it.
        assert_eq!(
            &played[first.len()..first.len() + second.len() - fade],
            &second[fade..]
        );
    }

    /// A crossfade waits for the song queued to have sound ready, and never
    /// begins before the end of the one playing is known; a song let go
    /// before it began lets its decoding go too.
    #[test]
    fn a_song_queued_and_let_go_never_sounds() {
        let data = SongData::complete(tone());
        let (first, first_feed) = decoded(&data);
        let (_, second_feed) = decoded(&data);
        let chain = Arc::new(Chain::default());
        let playing = Playing::new(
            Voice::new(first_feed, 1.0),
            Box::new(|| Duration::ZERO),
            Arc::clone(&chain),
        );
        *chain.next() = Some(Next {
            id: 1,
            voice: Voice::new(Arc::clone(&second_feed), 1.0),
            fade: 0,
        });
        chain.let_next_go();
        assert!(matches!(second_feed.take(), Taken::LetGo));
        let tail = tail_samples(Duration::ZERO, playing.rate, playing.channels);
        assert_eq!(playing.count(), first.len() + tail);
        assert_eq!(chain.start_of(1), None);
    }

    /// A song whose download broke off is not followed by the song queued,
    /// at its end or by a crossfade: the player stops there
    /// ([`Player::broke_off`]), as before, rather than skip the rest of it.
    #[test]
    fn a_song_that_broke_off_is_not_followed() {
        let bytes = tone();
        let broken = SongData::new(Some(bytes.len() as u64));
        broken.push_for_test(&bytes[..bytes.len() / 2]);
        broken.fail_for_test("the connection broke");
        let (cut, first_feed) = decoded(&broken);
        assert!(first_feed.broke_off());
        let (_, second_feed) = decoded(&SongData::complete(bytes));
        assert!(!second_feed.broke_off());
        let chain = Arc::new(Chain::default());
        let playing = Playing::new(
            Voice::new(first_feed, 1.0),
            Box::new(|| Duration::ZERO),
            Arc::clone(&chain),
        );
        let per_second = playing.rate as usize * usize::from(playing.channels);
        let tail = tail_samples(Duration::ZERO, playing.rate, playing.channels);
        *chain.next() = Some(Next {
            id: 5,
            voice: Voice::new(Arc::clone(&second_feed), 1.0),
            fade: per_second as u64,
        });
        assert_eq!(playing.count(), cut.len() + tail);
        assert_eq!(chain.start_of(5), None, "the song queued never began");
        assert!(matches!(second_feed.take(), Taken::LetGo));
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
        assert!(Arc::strong_count(&data) > 1, "the decoder holds the song");
        drop(playing);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&data) > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(Arc::strong_count(&data), 1, "the decoder let the song go");
    }
}
