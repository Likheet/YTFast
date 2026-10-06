//! Playing a song's audio.
//!
//! A song plays from its first bytes while the rest downloads
//! ([`crate::stream`]); once all of it has arrived it stays in memory, so
//! pausing for hours, closing the laptop lid or changing Wi-Fi cannot break
//! it.
//!
//! Output goes through `fastframe-audio`, the device stream Spotifast uses:
//! it costs no CPU while paused, follows the default output when headphones
//! come and go, and reopens after a failure. symphonia decodes (AAC in MP4)
//! and rodio's mixer converts to the device's rate.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

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

/// A song decoded as it plays, as a rodio source.
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
                // The end of the song, or a broken file: either way, stop.
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

impl rodio::Source for SongSource {
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

/// Plays one song at a time. Lives on one thread (the audio device handle
/// cannot move between threads on every platform).
///
/// Every jump (seek, new song, a device that changed format) builds a fresh
/// decoder and a fresh rodio sink instead of asking the playing one to seek:
/// rodio's seek waits for the audio thread, which does not run while the
/// device is paused.
pub struct Player {
    output: fastframe_audio::Output<MixerRender>,
    made: MixerSlot,
    mixer: rodio::mixer::Mixer,
    sink: rodio::Sink,
    song: Option<Loaded>,
    /// Where the current sink started in the song.
    offset: Duration,
    paused: bool,
    /// The listener's volume, 0 to 1, on top of the song's loudness gain.
    volume: f32,
}

impl Player {
    pub fn open() -> Result<Self, AudioError> {
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
        let output = fastframe_audio::Output::open(options, render)
            .map_err(|e| AudioError::Device(e.to_string()))?;
        let (mixer, _rate) = made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| AudioError::Device("the audio output did not start".into()))?;
        let sink = rodio::Sink::connect_new(&mixer);
        Ok(Self {
            output,
            made,
            mixer,
            sink,
            song: None,
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
        self.song = Some(Loaded { data, gain });
        self.paused = false;
        self.output.resume();
        self.restart_at(Duration::ZERO)
    }

    fn restart_at(&mut self, at: Duration) -> Result<(), AudioError> {
        let Some(song) = &self.song else {
            return Ok(());
        };
        let source = SongSource::open(&song.data, at)?;
        // A fresh sink; dropping the old one stops it on the audio thread
        // without waiting.
        let sink = rodio::Sink::connect_new(&self.mixer);
        sink.set_volume(song.gain * self.volume);
        if self.paused {
            sink.pause();
        }
        sink.append(source);
        self.sink = sink;
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

    /// Jumps to `to` in the current song.
    pub fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        self.restart_at(to)
    }

    /// How far into the song playback is.
    pub fn position(&self) -> Duration {
        self.offset + self.sink.get_pos()
    }

    /// The current song has played to its end.
    pub fn finished(&self) -> bool {
        self.song.is_some() && self.sink.empty()
    }

    pub fn stop(&mut self) {
        self.song = None;
        self.sink = rodio::Sink::connect_new(&self.mixer);
        self.offset = Duration::ZERO;
    }

    /// Keeps the output on a working device. Call a few times a second.
    /// Returns a message worth showing when something changed.
    pub fn maintain(&mut self) -> Option<String> {
        let mut note = None;
        match self.output.maintain() {
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
}
