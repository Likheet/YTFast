//! Playing songs with keyboard controls, the way YtFast will: the next song
//! is fetched while the current one plays, every jump is exact, and each
//! play is reported to YouTube so it lands in History.

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToColumn;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use ytfast_core::audio::{self, Player};
use ytfast_core::playreport::PlayReport;
use ytfast_core::prepare::{PrepareError, Preparer};
use ytfast_core::read::PlayerInfo;
use ytfast_core::ytdlp::Resolved;

use crate::report::{Outcome, Report, SongResult, clock};
use crate::ui;

/// A song to play.
#[derive(Clone, Debug)]
pub struct Song {
    pub video_id: String,
    pub title: String,
    pub artists: String,
    pub expected_seconds: Option<f64>,
}

impl Song {
    fn name(&self, info: &PlayerInfo) -> String {
        let title = if self.title.is_empty() {
            info.title.clone().unwrap_or_else(|| self.video_id.clone())
        } else {
            self.title.clone()
        };
        let artists = if self.artists.is_empty() {
            info.author.clone().unwrap_or_default()
        } else {
            self.artists.clone()
        };
        if artists.is_empty() {
            title
        } else {
            format!("{title} by {artists}")
        }
    }
}

/// A song ready to play.
struct Prepared {
    song: Song,
    info: PlayerInfo,
    data: std::sync::Arc<ytfast_core::stream::SongData>,
    gain: f32,
    result: SongResult,
}

/// Finds, describes and downloads one song (with the engine's own
/// [`Preparer`]), then decodes it once to prove the download is a whole,
/// playable song. Errors are plain sentences.
async fn prepare(
    preparer: Preparer,
    song: Song,
    resolved: Option<Resolved>,
    show_progress: bool,
) -> Result<Prepared, Box<SongResult>> {
    let mut result = SongResult {
        expected_seconds: song.expected_seconds,
        ..SongResult::default()
    };
    if show_progress {
        ui::progress("Finding and downloading the audio...");
    }
    let prepared = preparer.prepare(&song.video_id, resolved).await;
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            result.problem = Some(e.to_string());
            result.unavailable = matches!(e, PrepareError::Unavailable(_));
            return Err(Box::new(result));
        }
    };
    result.format = prepared.format.clone();
    result.premium = prepared.premium;
    result.find_seconds = prepared.find_time.as_secs_f64();
    result.loudness_db = prepared.info.loudness_db;
    if result.expected_seconds.is_none() {
        result.expected_seconds = prepared.duration_seconds;
    }
    if let Some(problem) = &prepared.details_problem {
        result.problem = Some(format!("could not get song details for History: {problem}"));
    }

    // The check waits for the whole song, to prove it is whole and
    // playable.
    let download_started = std::time::Instant::now();
    let data = std::sync::Arc::clone(&prepared.data);
    let checked = tokio::task::spawn_blocking(move || {
        data.wait_complete(std::time::Duration::from_secs(300))?;
        let bytes = data.to_vec();
        let length = audio::decoded_length(&bytes).map_err(|e| e.to_string());
        Ok::<_, String>((bytes.len(), length))
    })
    .await
    .map_err(|e| {
        Box::new(SongResult {
            problem: Some(e.to_string()),
            ..result.clone()
        })
    })?;
    if show_progress {
        ui::end_progress();
    }
    let (size, length) = match checked {
        Ok(done) => done,
        Err(e) => {
            result.problem = Some(e);
            return Err(Box::new(result));
        }
    };
    result.download_seconds = (prepared.start_time + download_started.elapsed()).as_secs_f64();
    result.size_bytes = size as u64;
    match length {
        Ok(length) => result.decoded_seconds = Some(length.as_secs_f64()),
        Err(e) => {
            result.problem = Some(e);
            return Err(Box::new(result));
        }
    }
    Ok(Prepared {
        song,
        gain: prepared.gain,
        info: prepared.info,
        data: prepared.data,
        result,
    })
}

enum Leave {
    Ended,
    Next,
    Quit,
}

/// Puts the terminal in raw mode for single key presses, and back.
struct RawMode;

impl RawMode {
    fn on() -> Option<Self> {
        terminal::enable_raw_mode().ok().map(|()| Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn status_line(text: &str) {
    let mut out = std::io::stdout();
    let _ = crossterm::execute!(out, MoveToColumn(0), Clear(ClearType::CurrentLine));
    let _ = write!(out, "   {text}");
    let _ = out.flush();
}

/// Plays `songs` in order. Returns the IDs of songs that played and were
/// reported, for the History check.
pub fn run(
    rt: &Runtime,
    preparer: &Preparer,
    songs: Vec<Song>,
    first_resolved: Option<Resolved>,
    report: &mut Report,
) -> Vec<String> {
    let mut reported = Vec::new();
    let mut player = match Player::open() {
        Ok(p) => p,
        Err(e) => {
            ui::result(Outcome::Fail, &format!("No sound output: {e}"));
            report.step("Playback", Outcome::Fail, format!("no sound output: {e}"));
            return reported;
        }
    };
    ui::say(&format!("Sound is playing on: {}", player.device_name()));
    ui::say("Keys: Space = pause/play, Left/Right = back/forward 10 s, N = next song, Q = stop");
    ui::say("Try pausing for a few minutes, or closing the lid, then pressing Space.");

    let total = songs.len();
    let mut queue = songs.into_iter();
    let mut pending: Option<JoinHandle<Result<Prepared, Box<SongResult>>>> = None;
    let mut first = Some(first_resolved);
    let mut report_tasks: Vec<(usize, &'static str, JoinHandle<String>)> = Vec::new();
    let mut played = 0usize;
    // Songs that cannot be played here (removed, or not offered in this
    // country): skipped, as the app skips them, not failures.
    let mut unavailable = 0usize;
    let mut quit = false;

    for index in 0..total {
        let prepared = match pending.take() {
            Some(handle) => {
                ui::say("Getting the next song...");
                rt.block_on(handle).unwrap_or_else(|e| {
                    let problem = format!("getting the song stopped unexpectedly: {e}");
                    Err(Box::new(SongResult {
                        problem: Some(problem),
                        ..SongResult::default()
                    }))
                })
            }
            None => {
                let Some(song) = queue.next() else { break };
                ui::say("Finding and downloading the song...");
                rt.block_on(prepare(
                    preparer.clone(),
                    song,
                    first.take().flatten(),
                    true,
                ))
            }
        };
        // Get the following song while this one plays.
        if let Some(next) = queue.next() {
            pending = Some(rt.spawn(prepare(preparer.clone(), next, None, false)));
        }

        let Prepared {
            song,
            info,
            data,
            gain,
            mut result,
        } = match prepared {
            Ok(p) => p,
            Err(result) => {
                let problem = result.problem.clone().unwrap_or_default();
                if result.unavailable {
                    unavailable += 1;
                    ui::result(
                        Outcome::Skip,
                        &format!("Song {}: {problem}. Skipped.", index + 1),
                    );
                } else {
                    ui::result(
                        Outcome::Fail,
                        &format!("Song {} could not be prepared: {problem}", index + 1),
                    );
                }
                report.songs.push(*result);
                continue;
            }
        };

        if let Err(e) = player.play_song(data, gain) {
            ui::result(
                Outcome::Fail,
                &format!("Song {} could not start: {e}", index + 1),
            );
            result.problem = Some(e.to_string());
            report.songs.push(result);
            continue;
        }
        played += 1;
        let length = result
            .decoded_seconds
            .or(result.expected_seconds)
            .unwrap_or(0.0);
        ui::say(&format!(
            "Now playing ({} of {total}): {} [{}]",
            index + 1,
            song.name(&info),
            result.format
        ));

        let play_report = PlayReport::new(&info);
        let song_index = report.songs.len();
        if let Some(url) = play_report.started(0.0) {
            let session = Arc::clone(&preparer.session);
            report_tasks.push((
                song_index,
                "started",
                rt.spawn(async move { describe_report(session.send_report(&url).await) }),
            ));
        }

        let leave = play_loop(&mut player, length);
        let reached = player.position().as_secs_f64().min(length.max(0.0));
        result.played_seconds = reached;
        println!();

        if let Some(url) = play_report.listened(0.0, reached) {
            let session = Arc::clone(&preparer.session);
            report_tasks.push((
                song_index,
                "listened",
                rt.spawn(async move { describe_report(session.send_report(&url).await) }),
            ));
        }
        if play_report.is_possible() {
            reported.push(song.video_id.clone());
        }
        report.songs.push(result);

        if matches!(leave, Leave::Quit) {
            quit = true;
            break;
        }
    }
    player.stop();
    if let Some(handle) = pending.take() {
        handle.abort();
    }

    // Wait briefly for the reports to be answered.
    for (song_index, kind, handle) in report_tasks {
        let answer = rt
            .block_on(async { tokio::time::timeout(Duration::from_secs(15), handle).await })
            .map(|r| r.unwrap_or_else(|e| e.to_string()))
            .unwrap_or_else(|_| "no answer within 15 s".into());
        if let Some(song) = report.songs.get_mut(song_index) {
            match kind {
                "started" => song.started_report = Some(answer),
                _ => song.listened_report = Some(answer),
            }
        }
    }

    let outcome = if played > 0 && (quit || played + unavailable == total) {
        Outcome::Ok
    } else {
        Outcome::Fail
    };
    let mut detail = format!("{played} of {total} songs played");
    if unavailable > 0 {
        detail.push_str(&format!(
            " ({unavailable} not available to this account, skipped)"
        ));
    }
    if quit {
        detail.push_str(" (stopped early)");
    }
    // The device's name stays off the report: headphones are often named
    // after their owner ("Sam's AirPods").
    ui::result(
        outcome,
        &format!("{detail}, on \"{}\"", player.device_name()),
    );
    report.step("Playback", outcome, detail);
    reported
}

fn describe_report(result: Result<u16, ytfast_core::innertube::ApiError>) -> String {
    match result {
        Ok(204) | Ok(200) => "accepted".into(),
        Ok(status) => format!("HTTP {status}"),
        Err(e) => e.to_string(),
    }
}

/// Plays until the song ends or a key says otherwise.
fn play_loop(player: &mut Player, length: f64) -> Leave {
    let raw = RawMode::on();
    if raw.is_none() {
        // Not a terminal that takes single keys: play to the end.
        while !player.finished() {
            std::thread::sleep(Duration::from_millis(250));
            let _ = player.maintain();
        }
        return Leave::Ended;
    }
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        if let Some(note) = player.maintain() {
            status_line(&note);
            print!("\r\n");
        }
        if player.finished() {
            return Leave::Ended;
        }
        if last_draw.elapsed() >= Duration::from_millis(250) {
            let state = if player.is_paused() {
                "Paused "
            } else {
                "Playing"
            };
            status_line(&format!(
                "{state} {} / {}   (Space, Left/Right, N, Q)",
                clock(player.position().as_secs_f64()),
                clock(length)
            ));
            last_draw = Instant::now();
        }
        if !event::poll(Duration::from_millis(100)).unwrap_or(false) {
            continue;
        }
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let jump = |player: &mut Player, by: f64| {
            let to = (player.position().as_secs_f64() + by).clamp(0.0, (length - 1.0).max(0.0));
            if let Err(e) = player.seek(Duration::from_secs_f64(to)) {
                status_line(&format!("Could not jump: {e}"));
                print!("\r\n");
            }
        };
        match key.code {
            KeyCode::Char(' ') => {
                if player.is_paused() {
                    player.resume();
                } else {
                    player.pause();
                }
            }
            KeyCode::Left => jump(player, -10.0),
            KeyCode::Right => jump(player, 10.0),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => return Leave::Next,
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => return Leave::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Leave::Quit;
            }
            _ => {}
        }
        last_draw = Instant::now() - Duration::from_secs(1);
    }
}
