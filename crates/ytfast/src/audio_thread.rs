//! The sound, on a thread of its own. The audio device handle cannot move
//! between threads on every platform, and nothing the window does should
//! make the music stutter. The window sends commands and reads a status.
//!
//! Without a sound device (or in demo mode) a silent clock stands in, so
//! the window behaves the same.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ytfast_core::audio::Player;

pub enum Command {
    /// Play a whole song from the start. `length` is in seconds.
    Play {
        entry: u64,
        data: std::sync::Arc<ytfast_core::stream::SongData>,
        gain: f32,
        length: f64,
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

    fn maintain(&mut self) -> Option<String> {
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
    let mut engine = if silent {
        Engine::Silent(Silent::default())
    } else {
        match Player::open() {
            Ok(player) => Engine::Real(Box::new(player)),
            Err(e) => {
                problem = Some(format!("No sound output: {e}"));
                Engine::Silent(Silent::default())
            }
        }
    };
    let mut entry: Option<u64> = None;
    let mut length = 0.0;
    let mut ended_sent = false;
    // A jump waiting for the rest of the song to arrive. Waiting here, not
    // in the jump, keeps Pause, Next and Stop answering meanwhile.
    let mut pending_jump: Option<f64> = None;
    let update = |f: &mut dyn FnMut(&mut Status)| {
        let mut s = status.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s);
    };
    update(&mut |s| {
        s.device = engine.device();
        s.problem = problem.clone();
    });
    wake();

    loop {
        let mut changed = false;
        // A sounding song is followed closely (its position, its end);
        // otherwise a look now and then is enough (a device that changed).
        let sounding = entry.is_some() && !engine.paused();
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
                    } => {
                        pending_jump = None;
                        match engine.play(data, gain, song_length) {
                            Ok(()) => {
                                entry = Some(id);
                                length = song_length;
                                ended_sent = false;
                                problem = None;
                            }
                            Err(e) => {
                                // The song cannot be played: the window
                                // decides whether to move on.
                                update(&mut |s| {
                                    s.failed = Some(PlayFailure {
                                        entry: id,
                                        message: format!("This song could not be played: {e}"),
                                        song_only: true,
                                    });
                                });
                                entry = None;
                            }
                        }
                    }
                    Command::Pause => engine.pause(),
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
                        // Not the next song's length.
                        length = 0.0;
                    }
                    Command::Volume(v) => engine.set_volume(v),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Some(to) = pending_jump
            && engine.can_jump(to)
        {
            pending_jump = None;
            jump(&mut engine, to, &mut ended_sent, &mut problem);
            changed = true;
        }
        if let Some(note) = engine.maintain() {
            // The window hears of a problem once, not at every try.
            changed |= problem.as_ref() != Some(&note);
            problem = Some(note);
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
        update(&mut |s| {
            s.entry = entry;
            s.position = engine.position().min(length);
            s.length = length;
            s.paused = paused;
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
    }
}
