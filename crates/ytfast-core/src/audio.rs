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

use fastframe_audio::{Buffer, BufferSize, Maintained, OutputOptions, Render};
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
/// for the download.
const SILENCE_FRAMES: usize = 256;

/// Decoded sound on its way from a song's decoder to the device.
#[derive(Default)]
struct Queue {
    chunks: VecDeque<Vec<f32>>,
    /// Samples in `chunks`.
    samples: usize,
    /// Decoding has ended: at the end of the song, or where it broke off.
    ended: bool,
    /// Nobody plays the song any more: decoding stops.
    let_go: bool,
}

impl Queue {
    fn push(&mut self, chunk: Vec<f32>) {
        self.samples += chunk.len();
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
}

/// What the device gets from a [`Feed`].
enum Taken {
    Sound(Vec<f32>),
    /// Nothing decoded yet: the download is behind.
    Waiting,
    End,
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
            None if queue.ended || queue.let_go => Taken::End,
            None => Taken::Waiting,
        }
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
            self.0.lock().ended = true;
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

/// A song as the device plays it, as a rodio source.
///
/// The device must never wait (the sound would stop, and pausing or moving
/// to other headphones would wait too), so the song is decoded on a thread
/// of its own ([`decode_ahead`]), a little ahead. When the decoder is
/// behind, because the download is, silence plays, and is counted so the
/// song's position (and the lyrics) do not run ahead. Dropping it ends the
/// decoding thread.
struct Playing {
    feed: Arc<Feed>,
    chunk: Vec<f32>,
    at: usize,
    /// Samples of silence still to play.
    silence: usize,
    channels: u16,
    rate: u32,
}

impl Playing {
    /// Starts playing `song`: its first moments are decoded here, so the
    /// device's first request is answered with sound, and the rest on a
    /// thread of its own.
    fn start(mut song: SongSource) -> Result<(Self, Arc<Feed>), AudioError> {
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
        let feed = Arc::new(Feed {
            queue: Mutex::new(queue),
            room: Condvar::new(),
            silent: AtomicU64::new(0),
            per_second: per_second as u64,
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
                Taken::End => return None,
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
type OpenedOutput = (
    fastframe_audio::Output<MixerRender>,
    MixerSlot,
    rodio::mixer::Mixer,
);

/// Opens the default output, paused (the device rests until there is a
/// song to play).
fn open_output() -> Result<OpenedOutput, AudioError> {
    let made = MixerSlot::default();
    let render = MixerRender {
        source: None,
        format: (0, 0),
        made: Arc::clone(&made),
    };
    let options = OutputOptions {
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
    Ok((output, made, mixer))
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
/// ([`Player::prepare_spare`]), and moving to it takes about 0.01 s.
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
}

impl Player {
    pub fn open() -> Result<Self, AudioError> {
        let (output, made, mixer) = open_output()?;
        let sink = rodio::Sink::connect_new(&mixer);
        Ok(Self {
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
        self.output.resume();
        self.fed = true;
        self.restart_at(Duration::ZERO)
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
            Ok((output, made, mixer)) => {
                self.output = output;
                self.made = made;
                self.mixer = mixer;
                self.sink = rodio::Sink::connect_new(&self.mixer);
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
        let (source, feed) = Playing::start(SongSource::open(&song.data, at)?)?;
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
        if !self.paused && self.song.is_some() {
            self.output.resume();
            self.fed = true;
        }
        self.restart_at(to)
    }

    /// Whether a jump to `to` can be made without waiting: to the start,
    /// or anywhere once the whole song has arrived.
    pub fn can_jump(&self, to: Duration) -> bool {
        to.is_zero() || self.song.as_ref().is_none_or(|s| s.data.is_complete())
    }

    /// How far into the song playback is. Silence played while the download
    /// caught up does not count.
    pub fn position(&self) -> Duration {
        let silent = self.feed.as_ref().map_or(Duration::ZERO, |f| f.silence());
        self.offset + self.sink.get_pos().saturating_sub(silent)
    }

    /// The current song has stopped sounding: played to its end, or broken
    /// off before it (see [`Player::broke_off`]).
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
    /// Returns a message worth showing when something changed.
    pub fn maintain(&mut self) -> Option<String> {
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
                        note = Some(format!("Could not continue on {device}: {error}"));
                    }
                }
                if reason != fastframe_audio::Reason::Resumed {
                    note.get_or_insert(format!("Sound is now playing on {device}"));
                }
            }
            Maintained::Failed(error) => note = Some(format!("No sound output: {error}")),
            Maintained::Released | Maintained::Unchanged => {}
        }
        for error in self.output.take_errors() {
            if error.is_fatal() {
                note = Some(format!("Audio problem: {error}"));
            }
        }
        note
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (mut playing, feed) = Playing::start(source).unwrap();
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
        assert_eq!(played - silent, whole);
        assert!(
            slowest < Duration::from_millis(500),
            "the device waited {slowest:?}"
        );
        // The silence is taken off the position.
        let per_second = f64::from(playing.rate) * f64::from(playing.channels);
        assert!((feed.silence().as_secs_f64() - silent as f64 / per_second).abs() < 1e-9);
    }

    #[test]
    fn letting_a_song_go_ends_its_decoding() {
        let data = SongData::complete(tone());
        let source = SongSource::open(&data, Duration::ZERO).unwrap();
        let (mut playing, _feed) = Playing::start(source).unwrap();
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
