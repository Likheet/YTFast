//! Playing songs with keyboard controls, the way YtFast will: the next song
//! is fetched while the current one plays, every jump is exact, and each
//! play is reported to YouTube so it lands in History.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToColumn;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use ytfast_core::audio::{self, Player};
use ytfast_core::innertube::Session;
use ytfast_core::playreport::PlayReport;
use ytfast_core::read::PlayerInfo;
use ytfast_core::ytdlp::{Resolved, YtDlp};

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

/// What playing needs, cloneable into background tasks.
#[derive(Clone)]
pub struct Context {
    pub session: Arc<Session>,
    pub yt_dlp: YtDlp,
    pub download: reqwest::Client,
    pub cookies_file: PathBuf,
}

/// A song ready to play.
struct Prepared {
    song: Song,
    info: PlayerInfo,
    bytes: Vec<u8>,
    result: SongResult,
}

/// Finds, describes and downloads one song. Errors are plain sentences.
async fn prepare(
    ctx: Context,
    song: Song,
    resolved: Option<Resolved>,
    show_progress: bool,
) -> Result<Prepared, Box<SongResult>> {
    let mut result = SongResult {
        expected_seconds: song.expected_seconds,
        ..SongResult::default()
    };
    let fail = |mut result: SongResult, problem: String| {
        result.problem = Some(problem);
        Err(Box::new(result))
    };

    let resolved = match resolved {
        Some(r) => r,
        None => match ctx.yt_dlp.resolve(&song.video_id, &ctx.cookies_file).await {
            Ok(r) => r,
            Err(e) => return fail(result, e.to_string()),
        },
    };
    result.find_seconds = resolved.took.as_secs_f64();
    if result.expected_seconds.is_none() {
        result.expected_seconds = resolved.duration_seconds;
    }
    let Some(format) = resolved.best_playable().cloned() else {
        return fail(result, "YouTube offered no AAC audio for this song".into());
    };
    result.format = format.describe();
    result.premium = format.is_premium();

    // Loudness and the History report addresses. Playing works without
    // them, so a failure here is only noted.
    let info = match ctx.session.player(&song.video_id).await {
        Ok(info) => info,
        Err(e) => {
            result.problem = Some(format!("could not get song details for History: {e}"));
            PlayerInfo::default()
        }
    };
    result.loudness_db = info.loudness_db;

    let progress = |done: u64, total: Option<u64>| {
        if show_progress {
            match total {
                Some(total) => ui::progress(&format!(
                    "Downloading the audio: {} of {}",
                    ui::megabytes(done),
                    ui::megabytes(total)
                )),
                None => ui::progress(&format!("Downloading the audio: {}", ui::megabytes(done))),
            }
        }
    };
    let downloaded = match audio::download(&ctx.download, &format, &progress).await {
        Ok(d) => d,
        Err(e) => {
            if show_progress {
                ui::end_progress();
            }
            return fail(result, e.to_string());
        }
    };
    if show_progress {
        ui::end_progress();
    }
    result.size_bytes = downloaded.bytes.len() as u64;
    result.download_seconds = downloaded.took.as_secs_f64();

    // Decode it all once: proves the download is a whole, playable song.
    let (bytes, length) = tokio::task::spawn_blocking(move || {
        let length = audio::decoded_length(&downloaded.bytes);
        (downloaded.bytes, length)
    })
    .await
    .map_err(|e| {
        Box::new(SongResult {
            problem: Some(e.to_string()),
            ..result.clone()
        })
    })?;
    match length {
        Ok(length) => result.decoded_seconds = Some(length.as_secs_f64()),
        Err(e) => return fail(result, e.to_string()),
    }
    Ok(Prepared {
        song,
        info,
        bytes,
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
    ctx: &Context,
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
                rt.block_on(prepare(ctx.clone(), song, first.take().flatten(), true))
            }
        };
        // Get the following song while this one plays.
        if let Some(next) = queue.next() {
            pending = Some(rt.spawn(prepare(ctx.clone(), next, None, false)));
        }

        let Prepared {
            song,
            info,
            bytes,
            mut result,
        } = match prepared {
            Ok(p) => p,
            Err(result) => {
                ui::result(
                    Outcome::Fail,
                    &format!(
                        "Song {} could not be prepared: {}",
                        index + 1,
                        result.problem.clone().unwrap_or_default()
                    ),
                );
                report.songs.push(*result);
                continue;
            }
        };

        let gain = audio::gain_for_loudness(info.loudness_db);
        if let Err(e) = player.play_song(bytes, gain) {
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
            let session = Arc::clone(&ctx.session);
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
            let session = Arc::clone(&ctx.session);
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

    let outcome = if played == total || (quit && played > 0) {
        Outcome::Ok
    } else {
        Outcome::Fail
    };
    let detail = format!(
        "{played} of {total} songs played{} on \"{}\"",
        if quit { " (stopped early)" } else { "" },
        player.device_name()
    );
    ui::result(outcome, &detail);
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
