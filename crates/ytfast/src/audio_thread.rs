//! The sound, on a thread of its own. The audio device handle cannot move
//! between threads on every platform, and nothing the window does should
//! make the music stutter. The window sends commands and reads a status.
//!
//! In demo mode a silent clock stands in, so the window behaves the same.
//! Without a sound device, songs do not play: each Play looks for one
//! again, and says so while there is none.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ytfast_core::audio::{Maintenance, Player};

pub enum Command {
    /// Play a song from `from` seconds (the start, unless the video mode
    /// changed which version plays partway). `length` is in seconds.
    Play {
        entry: u64,
        data: std::sync::Arc<ytfast_core::stream::SongData>,
        gain: f32,
        length: f64,
        from: f64,
    },
    Pause,
    Resume,
    /// Jump to this many seconds into the song.
    Seek(f64),
    Stop,
    /// 0 to 1.
    Volume(f32),
}

/// What the window shows about playback.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// The queue entry playing (or paused).
    pub entry: Option<u64>,
    pub position: f64,
    pub length: f64,
    pub paused: bool,
    /// An entry that played to its end, for the window to move on from.
    pub ended: Option<u64>,
    /// An entry that could not play to its end.
    pub failed: Option<PlayFailure>,
    pub device: String,
    pub problem: Option<String>,
    /// When `position` was read, and whether it was moving then (playing,
    /// not paused nor waiting to jump), for a clock finer than the reads
    /// (the video mode's pictures).
    pub measured: Option<Instant>,
    pub running: bool,
}

impl Status {
    /// Where the song is now: `position`, moved on by the time since it
    /// was read while it runs.
    pub fn clock(&self) -> f64 {
        match self.measured {
            Some(at) if self.running => {
                (self.position + at.elapsed().as_secs_f64()).min(self.length.max(self.position))
            }
            _ => self.position,
        }
    }
}

/// A song that could not play to its end.
#[derive(Clone, Debug)]
pub struct PlayFailure {
    pub entry: u64,
    pub message: String,
    /// The song's own audio is at fault (it cannot be decoded), so the next
    /// song may well play. Otherwise its download broke off, which the next
    /// song would likely run into too.
    pub song_only: bool,
}

pub struct Audio {
    commands: Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Audio {
    /// Starts the audio thread. `wake` asks the window to redraw.
    pub fn start(wake: impl Fn() + Send + 'static, silent: bool) -> Self {
        let (commands, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = Arc::clone(&status);
        std::thread::Builder::new()
            .name("ytfast-audio".into())
            .spawn(move || run(receiver, shared, wake, silent))
            .expect("the audio thread starts");
        Self { commands, status }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A player with nothing behind it, for the app's tests: the commands
    /// come out of the receiver, and [`Audio::set_status`] answers.
    #[cfg(test)]
    pub fn for_tests() -> (Self, Receiver<Command>) {
        let (commands, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        (Self { commands, status }, receiver)
    }

    /// Changes the status, as the audio thread would.
    #[cfg(test)]
    pub fn set_status(&self, change: impl FnOnce(&mut Status)) {
        change(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// The entry that ended, once.
    pub fn take_ended(&self) -> Option<u64> {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ended
            .take()
    }

    /// The entry that could not play to its end, once.
    pub fn take_failed(&self) -> Option<PlayFailure> {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .failed
            .take()
    }
}

/// A clock that plays nothing, for demo mode and computers without sound.
#[derive(Default)]
struct Silent {
    started: Option<Instant>,
    offset: f64,
    length: f64,
    loaded: bool,
}

impl Silent {
    fn position(&self) -> f64 {
        let running = self.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
        (self.offset + running).min(self.length)
    }
}

enum Engine {
    Real(Box<Player>),
    Silent(Silent),
}

impl Engine {
    fn play(
        &mut self,
        data: std::sync::Arc<ytfast_core::stream::SongData>,
        gain: f32,
        length: f64,
    ) -> Result<(), String> {
        match self {
            Self::Real(player) => player.play_song(data, gain).map_err(|e| e.to_string()),
            Self::Silent(s) => {
                *s = Silent {
                    started: Some(Instant::now()),
                    offset: 0.0,
                    length,
                    loaded: true,
                };
                Ok(())
            }
        }
    }

    fn pause(&mut self) {
        match self {
            Self::Real(player) => player.pause(),
            Self::Silent(s) => {
                s.offset = s.position();
                s.started = None;
            }
        }
    }

    fn resume(&mut self) {
        match self {
            Self::Real(player) => player.resume(),
            Self::Silent(s) => {
                if s.started.is_none() && s.loaded {
                    s.started = Some(Instant::now());
                }
            }
        }
    }

    fn paused(&self) -> bool {
        match self {
            Self::Real(player) => player.is_paused(),
            Self::Silent(s) => s.started.is_none(),
        }
    }

    fn seek(&mut self, to: f64) -> Result<(), String> {
        match self {
            Self::Real(player) => player
                .seek(Duration::from_secs_f64(to.max(0.0)))
                .map_err(|e| e.to_string()),
            Self::Silent(s) => {
                s.offset = to.clamp(0.0, s.length);
                if s.started.is_some() {
                    s.started = Some(Instant::now());
                }
                Ok(())
            }
        }
    }

    /// Whether a jump to `to` seconds can be made now, without waiting for
    /// the rest of the song to arrive.
    fn can_jump(&self, to: f64) -> bool {
        match self {
            Self::Real(player) => player.can_jump(Duration::from_secs_f64(to.max(0.0))),
            Self::Silent(_) => true,
        }
    }

    fn stop(&mut self) {
        match self {
            Self::Real(player) => player.stop(),
            Self::Silent(s) => *s = Silent::default(),
        }
    }

    fn position(&self) -> f64 {
        match self {
            Self::Real(player) => player.position().as_secs_f64(),
            Self::Silent(s) => s.position(),
        }
    }

    fn finished(&self) -> bool {
        match self {
            Self::Real(player) => player.finished(),
            Self::Silent(s) => s.loaded && s.position() >= s.length,
        }
    }

    /// Why a finished song stopped before its end, if it did.
    fn broke_off(&self) -> Option<String> {
        match self {
            Self::Real(player) => player.broke_off(),
            Self::Silent(_) => None,
        }
    }

    fn set_volume(&mut self, volume: f32) {
        if let Self::Real(player) = self {
            player.set_volume(volume);
        }
    }

    fn prepare_spare(&mut self) {
        if let Self::Real(player) = self {
            player.prepare_spare();
        }
    }

    fn maintain(&mut self) -> Option<Maintenance> {
        match self {
            Self::Real(player) => player.maintain(),
            Self::Silent(_) => None,
        }
    }

    fn device(&self) -> String {
        match self {
            Self::Real(player) => player.device_name().to_string(),
            Self::Silent(_) => "no sound device (silent)".into(),
        }
    }
}

/// What a Play says while no sound device can be found.
const NO_DEVICE: &str =
    "No sound output was found. Connect speakers or headphones, then press Play.";

/// Jumps to `to` seconds into the song.
fn jump(engine: &mut Engine, to: f64, ended_sent: &mut bool, problem: &mut Option<String>) {
    match engine.seek(to) {
        // Playing again (after the end, to repeat it): its next end is news
        // too.
        Ok(()) => *ended_sent = false,
        Err(e) => *problem = Some(format!("Could not jump: {e}")),
    }
}

fn run(commands: Receiver<Command>, status: Arc<Mutex<Status>>, wake: impl Fn(), silent: bool) {
    let mut problem = None;
    // No sound device was found: the silent clock only holds the place.
    let mut no_device = false;
    let mut engine = if silent {
        Engine::Silent(Silent::default())
    } else {
        match Player::open() {
            Ok(player) => Engine::Real(Box::new(player)),
            Err(e) => {
                problem = Some(format!("No sound output: {e}"));
                no_device = true;
                Engine::Silent(Silent::default())
            }
        }
    };
    // The last volume asked for, for a device found later.
    let mut volume: Option<f32> = None;
    let mut entry: Option<u64> = None;
    let mut length = 0.0;
    let mut ended_sent = false;
    // A jump waiting for the rest of the song to arrive. Waiting here, not
    // in the jump, keeps Pause, Next and Stop answering meanwhile.
    let mut pending_jump: Option<f64> = None;
    // A song started partway waits for that jump silent, then sounds.
    let mut sound_after_jump = false;
    let update = |f: &mut dyn FnMut(&mut Status)| {
        let mut s = status.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s);
    };
    update(&mut |s| {
        s.device = engine.device();
        s.problem = problem.clone();
    });
    wake();
    // The song playing, to carry on with when the device has to be opened
    // again, and where it was.
    let mut current: Option<(std::sync::Arc<ytfast_core::stream::SongData>, f32)> = None;
    let mut last_position = 0.0;
    let mut last_paused = false;
    // The audio library failed (it can while a device comes or goes, as
    // Bluetooth headphones do): when the device was last tried again.
    let mut recovering: Option<Instant> = None;

    loop {
        // Each turn is guarded: a failure in the audio library leaves this
        // thread running, the device is opened again, and the song carries
        // on from where it was, instead of every song after it being
        // silent until YTFast is opened again.
        let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> bool {
            let mut changed = false;
            if let Some(tried) = recovering
                && tried.elapsed() >= RECOVER_EVERY
            {
                recovering = Some(Instant::now());
                if let Ok(mut player) = Player::open() {
                    if let Some(v) = volume {
                        player.set_volume(v);
                    }
                    engine = Engine::Real(Box::new(player));
                    recovering = None;
                    no_device = false;
                    problem = None;
                    changed = true;
                    log::warn!("the sound device works again: {}", engine.device());
                    if let (Some((data, gain)), Some(_)) = (&current, entry) {
                        match engine.play(std::sync::Arc::clone(data), *gain, length) {
                            Ok(()) => {
                                if last_position > 0.5 {
                                    pending_jump = Some(last_position);
                                }
                                if last_paused {
                                    engine.pause();
                                }
                            }
                            Err(e) => problem = Some(format!("This song could not be played: {e}")),
                        }
                    }
                }
            }
            // A sounding song is followed closely (its position, its end),
            // as is one about to sound once its jump is made; otherwise a
            // look now and then is enough (a device that changed).
            let sounding = entry.is_some() && (!engine.paused() || sound_after_jump);
            let wait = Duration::from_millis(if sounding { 100 } else { 1000 });
            match commands.recv_timeout(wait) {
                Ok(command) => {
                    changed = true;
                    match command {
                        Command::Play {
                            entry: id,
                            data,
                            gain,
                            length: song_length,
                            from,
                        } => {
                            pending_jump = None;
                            sound_after_jump = false;
                            // Speakers or headphones may have been connected
                            // since: look again rather than play to nobody.
                            if no_device {
                                match Player::open() {
                                    Ok(mut player) => {
                                        if let Some(v) = volume {
                                            player.set_volume(v);
                                        }
                                        engine = Engine::Real(Box::new(player));
                                        no_device = false;
                                    }
                                    Err(e) => problem = Some(format!("No sound output: {e}")),
                                }
                            }
                            let kept = std::sync::Arc::clone(&data);
                            let played = if no_device {
                                Err(NO_DEVICE.to_string())
                            } else {
                                engine
                                    .play(data, gain, song_length)
                                    .map_err(|e| format!("This song could not be played: {e}"))
                            };
                            match played {
                                Ok(()) => {
                                    current = Some((kept, gain));
                                    entry = Some(id);
                                    length = song_length;
                                    ended_sent = false;
                                    problem = None;
                                    // Partway: silent until there, not the
                                    // song's start heard first.
                                    let from = from.clamp(0.0, (length - 0.5).max(0.0));
                                    if from > 0.5 {
                                        engine.pause();
                                        pending_jump = Some(from);
                                        sound_after_jump = true;
                                    }
                                }
                                Err(message) => {
                                    // The song cannot be played: the window
                                    // decides whether to move on. Without a
                                    // device, the next song could not either.
                                    update(&mut |s| {
                                        s.failed = Some(PlayFailure {
                                            entry: id,
                                            message: message.clone(),
                                            song_only: !no_device,
                                        });
                                    });
                                    entry = None;
                                    length = 0.0;
                                }
                            }
                        }
                        Command::Pause => {
                            sound_after_jump = false;
                            engine.pause();
                        }
                        // Waiting silent for a jump, it sounds once there.
                        Command::Resume if sound_after_jump => {}
                        Command::Resume => engine.resume(),
                        Command::Seek(to) => {
                            let to = to.clamp(0.0, (length - 0.5).max(0.0));
                            if engine.can_jump(to) {
                                pending_jump = None;
                                jump(&mut engine, to, &mut ended_sent, &mut problem);
                            } else {
                                pending_jump = Some(to);
                            }
                        }
                        Command::Stop => {
                            engine.stop();
                            entry = None;
                            pending_jump = None;
                            sound_after_jump = false;
                            // Not the next song's length.
                            length = 0.0;
                        }
                        Command::Volume(v) => {
                            volume = Some(v);
                            engine.set_volume(v);
                        }
                    }
                }
                // Nothing waits: time to open a spare stream, if one is due.
                Err(RecvTimeoutError::Timeout) => engine.prepare_spare(),
                Err(RecvTimeoutError::Disconnected) => return false,
            }
            if let Some(to) = pending_jump
                && engine.can_jump(to)
            {
                pending_jump = None;
                jump(&mut engine, to, &mut ended_sent, &mut problem);
                if std::mem::take(&mut sound_after_jump) {
                    engine.resume();
                }
                changed = true;
            }
            match engine.maintain() {
                // Playing on another device (headphones connected): a device
                // problem shown before is over.
                Some(Maintenance::Moved(_)) => {
                    problem = None;
                    changed = true;
                }
                Some(Maintenance::Problem(note)) => {
                    // The window hears of a problem once, not at every try.
                    changed |= problem.as_ref() != Some(&note);
                    problem = Some(note);
                }
                None => {}
            }
            // Say once that the song ended, or that it broke off before its end
            // (its download failed or stalled): that is no reason to move on.
            let just_ended = entry.is_some() && !ended_sent && engine.finished();
            let broke_off = if just_ended { engine.broke_off() } else { None };
            if just_ended {
                ended_sent = true;
                changed = true;
            }
            let paused = engine.paused();
            last_position = engine.position().min(length);
            last_paused = paused;
            update(&mut |s| {
                s.entry = entry;
                // Waiting silent for a jump: where it starts.
                s.position = match (sound_after_jump, pending_jump) {
                    (true, Some(to)) => to,
                    _ => engine.position().min(length),
                };
                s.measured = Some(Instant::now());
                s.running = entry.is_some() && !paused && !sound_after_jump;
                s.length = length;
                s.paused = paused && !sound_after_jump;
                s.device = engine.device();
                s.problem = problem.clone();
                match (&broke_off, entry) {
                    (Some(why), Some(id)) => {
                        s.failed = Some(PlayFailure {
                            entry: id,
                            message: format!(
                                "The song stopped: its download broke off ({why}). Press Play to try again."
                            ),
                            song_only: false,
                        });
                    }
                    _ if just_ended => s.ended = entry,
                    _ => {}
                }
            });
            if changed {
                wake();
            }
            true
        }));
        match turn {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => {
                // The panic hook has written why in the log.
                log::warn!("the sound device failed; opening it again");
                let broken = std::mem::replace(&mut engine, Engine::Silent(Silent::default()));
                // Letting it go can fail too: that is no reason to stop.
                let _ =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(broken)));
                no_device = true;
                problem = Some("The sound device stopped. Trying it again...".into());
                // At once, then every RECOVER_EVERY.
                recovering = Some(Instant::now() - RECOVER_EVERY);
                update(&mut |s| s.problem = problem.clone());
                wake();
            }
        }
    }
}

/// How often a device that failed is tried again.
const RECOVER_EVERY: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    /// The status once `ready` says so, or after two seconds.
    fn status_when(audio: &Audio, ready: impl Fn(&Status) -> bool) -> Status {
        let since = Instant::now();
        loop {
            let status = audio.status();
            if ready(&status) || since.elapsed() > Duration::from_secs(2) {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A song started partway (the other version, in the video mode) is
    /// heard from there, not from its start, and its clock runs from there.
    #[test]
    fn a_song_started_partway_plays_from_there() {
        let audio = Audio::start(|| {}, true);
        audio.send(Command::Play {
            entry: 7,
            data: ytfast_core::stream::SongData::complete(Vec::new()),
            gain: 1.0,
            length: 200.0,
            from: 42.0,
        });
        let status = status_when(&audio, |s| s.entry == Some(7) && s.running);
        assert!(status.running, "it plays: {status:?}");
        assert!(!status.paused);
        assert!((42.0..43.0).contains(&status.position), "{status:?}");
        assert!((42.0..43.0).contains(&status.clock()), "{status:?}");
        // Paused, the clock stays where it is.
        audio.send(Command::Pause);
        let paused = status_when(&audio, |s| s.paused);
        assert!(paused.paused && !paused.running);
        assert!((paused.clock() - paused.position).abs() < 1e-9);
    }
}
