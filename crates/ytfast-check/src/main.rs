//! `ytfast-check`: step 0 of YtFast.
//!
//! Before any app is built, this proves on a real computer, with a real
//! Premium account, that the YtFast way works: reading the YouTube sign-in,
//! talking to YouTube Music, getting Premium-quality audio, playing it with
//! pause and seek, and reporting plays so they land in History. It prints
//! what it finds in plain language and saves a report with nothing personal
//! in it.

mod play;
mod report;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::runtime::Runtime;
use ytfast_core::cookies::CookieJar;
use ytfast_core::helpers::{self, Progress};
use ytfast_core::innertube::{ApiError, Session};
use ytfast_core::net;
use ytfast_core::prepare::Preparer;
use ytfast_core::ytdlp::{Browser, YtDlp, is_video_id};

use crate::play::Song;
use crate::report::{Outcome, Report, clock};

const STEPS: usize = 7;

#[derive(Parser, Debug)]
#[command(
    name = "ytfast-check",
    version,
    about = "Checks that YtFast can sign in, get Premium audio, play it and report plays, on this computer."
)]
struct Args {
    /// The browser you use YouTube Music in: chrome, firefox, safari, edge or brave.
    #[arg(long, value_name = "NAME")]
    browser: Option<String>,

    /// A cookies.txt file (exported from a private browser window) to use instead of a browser.
    #[arg(long, value_name = "FILE")]
    cookies: Option<PathBuf>,

    /// Play this song instead of your Liked songs: a YouTube Music link or its 11-character ID. Can be repeated.
    #[arg(long = "song", value_name = "LINK")]
    songs: Vec<String>,

    /// How many of your Liked songs to play.
    #[arg(long, default_value_t = 3, value_name = "N")]
    count: usize,

    /// Check everything except playing sound.
    #[arg(long)]
    no_play: bool,

    /// Show technical details while running.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    env_logger::Builder::new()
        .filter_level(if args.verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Warn
        })
        .init();

    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Could not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let folders = match Folders::new() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Could not prepare YtFast's folders: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Ctrl+C outside the player: remove the sign-in copy before leaving.
    let session_dir = folders.session.clone();
    rt.spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = std::fs::remove_dir_all(&session_dir);
            eprintln!("\nStopped.");
            std::process::exit(130);
        }
    });

    println!("YtFast check (step 0)");
    println!();
    println!("This checks that YouTube Music can work the YtFast way on this computer.");
    println!("It reads your YouTube sign-in from your browser, plays a few of your Liked");
    println!("songs, and saves a report you can share. Your account is not changed,");
    println!("except that the songs played here appear in your YouTube Music History,");
    println!("just as they would on the website.");

    let mut report = Report::default();
    run(&rt, &args, &folders, &mut report);

    println!();
    println!("Summary");
    for step in &report.steps {
        println!("   {} {}", step.outcome.tag(), step.name);
    }
    println!();
    if report.passed() {
        println!("Everything worked. Step 0 passed on this computer.");
    } else {
        println!("Some checks did not pass. The report says what went wrong.");
    }
    match report.save() {
        Ok(path) => {
            println!();
            println!("Report saved to: {}", path.display());
            println!("It has no account name, song titles, passwords or cookies in it.");
            println!("Share it with Claude to plan the next step.");
        }
        Err(e) => println!("The report could not be saved: {e}"),
    }
    let _ = std::fs::remove_dir_all(&folders.session);
    if ui::interactive() && cfg!(windows) {
        // Keep the window open when started by double-clicking.
        let _ = ui::ask("Press Enter to close.");
    }
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Where YtFast keeps its helpers and caches.
struct Folders {
    helpers: PathBuf,
    yt_dlp_cache: PathBuf,
    /// A private folder for this run's copy of the YouTube cookies. Removed
    /// at the end of the run, and at the start of the next one if a run was
    /// cut short.
    session: PathBuf,
}

impl Folders {
    fn new() -> std::io::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "YtFast")
            .ok_or_else(|| std::io::Error::other("no home folder"))?;
        let helpers = dirs.data_local_dir().join("helpers");
        let cache = dirs.cache_dir().to_path_buf();
        let sessions = cache.join("sessions");
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        let session = sessions.join(std::process::id().to_string());
        std::fs::create_dir_all(&session)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            helpers,
            yt_dlp_cache: cache.join("yt-dlp"),
            session,
        })
    }
}

/// Writes a file only this user can read.
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

enum SignInSource {
    /// A browser, and optionally one of its profiles ("Profile 1").
    Browser(Browser, Option<String>),
    File(PathBuf),
}

fn run(rt: &Runtime, args: &Args, folders: &Folders, report: &mut Report) {
    // Step 1: helper programs.
    ui::heading(1, STEPS, "Getting the helper programs (yt-dlp and Deno)");
    let Some(helpers) = step_helpers(rt, folders, report) else {
        return;
    };
    let yt_dlp = YtDlp::new(&helpers, folders.yt_dlp_cache.clone());

    // Step 2: the sign-in.
    ui::heading(2, STEPS, "Reading your YouTube sign-in");
    let Some(cookies) = step_sign_in(rt, args, folders, &yt_dlp, report) else {
        return;
    };
    let cookies_file = folders.session.join("youtube-cookies.txt");
    if let Err(e) = write_private(&cookies_file, &cookies.to_netscape()) {
        report.step(
            "Sign-in",
            Outcome::Fail,
            format!("could not keep the sign-in for yt-dlp: {e}"),
        );
        return;
    }

    // Step 3: the account.
    ui::heading(3, STEPS, "Checking your YouTube Music account");
    let Some(session) = step_account(rt, &cookies, report) else {
        return;
    };
    let session = Arc::new(session);

    // Step 4: Liked songs.
    ui::heading(4, STEPS, "Reading your Liked songs");
    let Some(songs) = step_songs(rt, args, &session, report) else {
        return;
    };

    // Step 5: Premium audio, from the first song that is available.
    ui::heading(5, STEPS, "Checking for Premium-quality audio");
    let (songs, resolved) = step_premium_audio(rt, &yt_dlp, songs, &cookies_file, report);

    // Step 6: playing.
    ui::heading(6, STEPS, "Playing your songs");
    let reported: (Vec<String>, Vec<String>) = if args.no_play {
        ui::result(Outcome::Skip, "Skipped (--no-play).");
        report.step("Playback", Outcome::Skip, "skipped with --no-play");
        (Vec::new(), Vec::new())
    } else if resolved
        .as_ref()
        .is_none_or(|r| r.best_playable().is_none())
    {
        ui::result(
            Outcome::Skip,
            "Skipped: no playable audio was found in step 5.",
        );
        report.step("Playback", Outcome::Skip, "no playable audio from step 5");
        (Vec::new(), Vec::new())
    } else {
        // What History looked like before, to tell new plays from old ones.
        let before: Vec<String> = rt
            .block_on(session.history())
            .map(|page| page.tracks.into_iter().map(|t| t.video_id).collect())
            .unwrap_or_default();
        let preparer = Preparer {
            session: Arc::clone(&session),
            yt_dlp: yt_dlp.clone(),
            download: net::download_client(),
            cookies_file: cookies_file.clone(),
            // The check tests yt-dlp's way, which the app falls back on.
            direct: None,
        };
        let reported = play::run(rt, &preparer, songs, resolved, report);
        (reported, before)
    };

    // Step 7: History.
    ui::heading(7, STEPS, "Checking that your plays reached your History");
    step_history(rt, &session, &reported.0, &reported.1, report);
}

fn step_helpers(rt: &Runtime, folders: &Folders, report: &mut Report) -> Option<helpers::Helpers> {
    let started = Instant::now();
    let progress = |p: Progress| match p {
        Progress::Checking => ui::say("Checking for the latest yt-dlp..."),
        Progress::UsingInstalled { what, version } => {
            ui::say(&format!("{what} {version} is already here."))
        }
        Progress::Downloading { what, done, total } => {
            let total = total
                .map(|t| format!(" of {}", ui::megabytes(t)))
                .unwrap_or_default();
            ui::progress(&format!(
                "Downloading {what}: {}{total}",
                ui::megabytes(done)
            ));
        }
        Progress::Unpacking(what) => {
            ui::end_progress();
            ui::say(&format!("Checked {what}'s fingerprint. Unpacking..."));
        }
    };
    let http = net::download_client();
    let helpers = match rt.block_on(helpers::ensure(&http, &folders.helpers, &progress)) {
        Ok(h) => h,
        Err(e) => {
            ui::end_progress();
            let detail = format!("{e:#}");
            ui::result(
                Outcome::Fail,
                &format!("Could not get the helper programs: {detail}"),
            );
            ui::say("Check the internet connection and try again.");
            report.step(
                "Helper programs",
                Outcome::Fail,
                ytfast_core::redact::urls(&detail),
            );
            return None;
        }
    };
    // Prove they run here (a blocked or damaged program fails now, not
    // halfway through a song).
    let yt_dlp = YtDlp::new(&helpers, folders.yt_dlp_cache.clone());
    match rt.block_on(yt_dlp.version()) {
        Ok(version) => {
            let detail = format!(
                "yt-dlp {version} and Deno {} ready in {:.0} s, in {}",
                helpers.deno_version,
                started.elapsed().as_secs_f64(),
                folders.helpers.display()
            );
            ui::result(Outcome::Ok, &detail);
            report.fact("yt-dlp", &version);
            report.fact("Deno", &helpers.deno_version);
            report.step(
                "Helper programs",
                Outcome::Ok,
                format!(
                    "yt-dlp {version}, Deno {}, {:.0} s",
                    helpers.deno_version,
                    started.elapsed().as_secs_f64()
                ),
            );
            Some(helpers)
        }
        Err(e) => {
            ui::result(
                Outcome::Fail,
                &format!("yt-dlp would not start on this computer: {e}"),
            );
            if cfg!(windows) {
                ui::say(
                    "Windows security software sometimes blocks it. Check Windows Security > Protection history.",
                );
            }
            report.step(
                "Helper programs",
                Outcome::Fail,
                format!("yt-dlp would not start: {e}"),
            );
            None
        }
    }
}

fn choose_source(args: &Args) -> Result<SignInSource, String> {
    if let Some(path) = &args.cookies {
        return Ok(SignInSource::File(path.clone()));
    }
    if let Some(given) = &args.browser {
        // "chrome:Profile 1" picks one of the browser's profiles.
        let (name, profile) = match given.split_once(':') {
            Some((name, profile)) => (name, Some(profile.trim().to_string())),
            None => (given.as_str(), None),
        };
        let browser = Browser::parse(name).ok_or_else(|| {
            format!("\"{name}\" is not one of: chrome, firefox, safari, edge, brave")
        })?;
        if let Some(problem) = browser.problem_here() {
            return Err(problem.to_string());
        }
        return Ok(SignInSource::Browser(
            browser,
            profile.filter(|p| !p.is_empty()),
        ));
    }
    if !ui::interactive() {
        return Err(
            "Run it again with --browser NAME (for example --browser firefox) or --cookies FILE."
                .into(),
        );
    }
    ui::say("Which browser are you signed in to YouTube Music with?");
    for (i, browser) in Browser::ALL.iter().enumerate() {
        let note = match browser.problem_here() {
            Some(_) if cfg!(windows) => "  (does not work on Windows)",
            Some(_) => "  (not on this computer)",
            None => "",
        };
        ui::say(&format!("  {}) {}{note}", i + 1, browser.label()));
    }
    let file_choice = Browser::ALL.len() + 1;
    ui::say(&format!("  {file_choice}) I have a cookies.txt file"));
    if cfg!(windows) {
        ui::say("On Windows, Firefox is the one that works: install it, sign in to");
        ui::say("music.youtube.com there once, then choose it here.");
    }
    loop {
        let answer = ui::ask("Type a number and press Enter:").ok_or("No answer was given.")?;
        let Ok(n) = answer.parse::<usize>() else {
            continue;
        };
        if n == file_choice {
            let path =
                ui::ask("Drag the cookies.txt file here (or type its path) and press Enter:")
                    .ok_or("No file was given.")?;
            let path = path.trim_matches(['"', '\'']).trim();
            return Ok(SignInSource::File(PathBuf::from(path)));
        }
        if let Some(browser) = n.checked_sub(1).and_then(|i| Browser::ALL.get(i)).copied() {
            match browser.problem_here() {
                Some(problem) => ui::say(problem),
                None => return Ok(SignInSource::Browser(browser, None)),
            }
        }
    }
}

fn step_sign_in(
    rt: &Runtime,
    args: &Args,
    folders: &Folders,
    yt_dlp: &YtDlp,
    report: &mut Report,
) -> Option<CookieJar> {
    let source = match choose_source(args) {
        Ok(s) => s,
        Err(e) => {
            ui::result(Outcome::Fail, &e);
            report.step("Sign-in", Outcome::Fail, "no sign-in source was chosen");
            return None;
        }
    };
    let (jar, from) = match source {
        SignInSource::File(path) => match std::fs::read_to_string(&path) {
            Ok(text) => (
                CookieJar::parse_netscape(&text).youtube_only(),
                "a cookies.txt file".to_string(),
            ),
            Err(e) => {
                ui::result(
                    Outcome::Fail,
                    &format!("Could not read {}: {e}", path.display()),
                );
                report.step(
                    "Sign-in",
                    Outcome::Fail,
                    "the cookies file could not be read",
                );
                return None;
            }
        },
        SignInSource::Browser(browser, profile) => {
            ui::say(&format!("Reading the sign-in from {}...", browser.label()));
            if browser == Browser::Chrome && cfg!(target_os = "macos") {
                ui::say("If your Mac asks about \"Chrome Safe Storage\", click Always Allow.");
            }
            let read = yt_dlp.read_browser_sign_in(browser, profile.as_deref(), &folders.session);
            match rt.block_on(read) {
                Ok(jar) => (jar, browser.label().to_string()),
                Err(e) => {
                    ui::result(Outcome::Fail, &e.to_string());
                    if let ytfast_core::ytdlp::YtDlpError::Failed { details, .. } = &e
                        && !details.is_empty()
                    {
                        ui::say("yt-dlp said:");
                        ui::say(details);
                    }
                    report.step(
                        "Sign-in",
                        Outcome::Fail,
                        format!("{} ({})", e, browser.label()),
                    );
                    return None;
                }
            }
        }
    };
    report.fact("Sign-in from", &from);
    if !jar.looks_signed_in() {
        ui::result(
            Outcome::Fail,
            &format!(
                "Found {} YouTube cookies in {from}, but they are not a signed-in session.\n\
                 Open music.youtube.com there, make sure you are signed in, then run this again.",
                jar.len()
            ),
        );
        report.step(
            "Sign-in",
            Outcome::Fail,
            format!("YouTube cookies from {from} are not signed in"),
        );
        return None;
    }
    ui::result(
        Outcome::Ok,
        &format!(
            "Found your YouTube sign-in in {from} ({} YouTube cookies).",
            jar.len()
        ),
    );
    report.step(
        "Sign-in",
        Outcome::Ok,
        format!("signed-in YouTube cookies from {from}"),
    );
    Some(jar)
}

fn step_account(rt: &Runtime, cookies: &CookieJar, report: &mut Report) -> Option<Session> {
    let session = match rt.block_on(Session::start(net::api_client(), cookies)) {
        Ok(s) => s,
        Err(e) => {
            ui::result(Outcome::Fail, &format!("Could not open YouTube Music: {e}"));
            report.step("Account", Outcome::Fail, e.to_string());
            return None;
        }
    };
    let config = session.config();
    if config.client_version.is_none() {
        ui::say("Note: the YouTube Music page looked unusual; using built-in settings.");
    }
    match rt.block_on(session.account()) {
        Ok((account, flags)) => {
            if flags.logged_in == Some(false)
                || (account.is_none() && config.logged_in != Some(true))
            {
                ui::result(
                    Outcome::Fail,
                    "YouTube Music sees you as signed out.\nOpen music.youtube.com in your browser, sign in, then run this again.",
                );
                report.step(
                    "Account",
                    Outcome::Fail,
                    "YouTube Music treated the sign-in as signed out",
                );
                return None;
            }
            let who = account
                .map(|a| match a.handle {
                    Some(handle) => format!("{} ({handle})", a.name),
                    None => a.name,
                })
                .unwrap_or_else(|| "your account".into());
            let premium = match flags.premium {
                Some(true) => "yes",
                Some(false) => "no",
                // Not every reply says; step 5 shows it for certain.
                None => "checked in step 5",
            };
            let channel = if config.delegated_session_id.is_some() {
                "a secondary (brand) channel"
            } else {
                "the main channel"
            };
            ui::result(Outcome::Ok, &format!("Signed in as {who}, on {channel}."));
            ui::say(&format!("YouTube Music Premium: {premium}"));
            ui::say(
                "If that is not the account you expected, sign in to the right one in your browser.",
            );
            let outcome = if flags.premium == Some(false) {
                Outcome::Fail
            } else {
                Outcome::Ok
            };
            if outcome == Outcome::Fail {
                ui::result(
                    Outcome::Fail,
                    "This account does not have YouTube Music Premium, which YtFast needs.",
                );
            }
            report.step(
                "Account",
                outcome,
                format!(
                    "signed in; Premium: {premium}; {channel}; Google account #{} in the browser",
                    config.session_index.unwrap_or(0) + 1
                ),
            );
            Some(session)
        }
        Err(ApiError::SignedOut) => {
            ui::result(
                Outcome::Fail,
                "YouTube Music says you are signed out. Sign in again in your browser.",
            );
            report.step(
                "Account",
                Outcome::Fail,
                "YouTube Music treated the sign-in as signed out",
            );
            None
        }
        Err(e) => {
            ui::result(Outcome::Fail, &format!("Could not read your account: {e}"));
            report.step("Account", Outcome::Fail, e.to_string());
            None
        }
    }
}

/// The ID in a YouTube or YouTube Music link, or the ID itself.
fn video_id_from(link: &str) -> Option<String> {
    let link = link.trim();
    if is_video_id(link) {
        return Some(link.to_string());
    }
    let candidates = [link.split("v=").nth(1), link.split("youtu.be/").nth(1)];
    candidates
        .into_iter()
        .flatten()
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
        })
        .find(|id| is_video_id(id))
}

fn step_songs(
    rt: &Runtime,
    args: &Args,
    session: &Session,
    report: &mut Report,
) -> Option<Vec<Song>> {
    let mut chosen: Vec<Song> = Vec::new();
    for link in &args.songs {
        match video_id_from(link) {
            Some(video_id) => chosen.push(Song {
                video_id,
                title: String::new(),
                artists: String::new(),
                expected_seconds: None,
            }),
            None => ui::say(&format!(
                "Ignoring \"{link}\": it is not a YouTube Music song link."
            )),
        }
    }

    match rt.block_on(session.liked_songs()) {
        Ok(page) => {
            let count = page.tracks.len();
            let more = if page.more { " (and more)" } else { "" };
            if count == 0 && chosen.is_empty() {
                ui::result(
                    Outcome::Fail,
                    "Your Liked songs list is empty here.\nLike a few songs in YouTube Music, or run again with --song and a song link.",
                );
                report.step(
                    "Liked songs",
                    Outcome::Fail,
                    "no liked songs and no --song given",
                );
                return None;
            }
            ui::result(
                Outcome::Ok,
                &format!("Read {count} Liked songs{more}. The first few:"),
            );
            for (i, t) in page.tracks.iter().take(5).enumerate() {
                let length = t
                    .duration_seconds
                    .map(|s| format!(" ({})", clock(f64::from(s))))
                    .unwrap_or_default();
                ui::say(&format!(
                    "  {}. {} by {}{length}",
                    i + 1,
                    t.title,
                    t.artists
                ));
            }
            report.step(
                "Liked songs",
                Outcome::Ok,
                format!("{count} on the first page{more}"),
            );
            if let Some(premium) = page.flags.premium {
                let answer = if premium { "yes" } else { "no" };
                ui::say(&format!("YouTube Music Premium: {answer}"));
                report.fact("Premium (from Liked songs)", answer);
            }
            if chosen.is_empty() {
                chosen = page
                    .tracks
                    .iter()
                    .take(args.count.max(1))
                    .map(|t| Song {
                        video_id: t.video_id.clone(),
                        title: t.title.clone(),
                        artists: t.artists.clone(),
                        expected_seconds: t.duration_seconds.map(f64::from),
                    })
                    .collect();
            }
        }
        Err(e) => {
            ui::result(
                Outcome::Fail,
                &format!("Could not read your Liked songs: {e}"),
            );
            report.step("Liked songs", Outcome::Fail, e.to_string());
            if chosen.is_empty() {
                return None;
            }
        }
    }
    Some(chosen)
}

/// Asks yt-dlp for the audio of the first song that is available (one of
/// the first three), and puts that song first. A song can be unavailable
/// (removed, or not in this country) without anything being wrong.
fn step_premium_audio(
    rt: &Runtime,
    yt_dlp: &YtDlp,
    mut songs: Vec<Song>,
    cookies_file: &Path,
    report: &mut Report,
) -> (Vec<Song>, Option<ytfast_core::ytdlp::Resolved>) {
    ui::say("Asking yt-dlp for a song's audio (this can take a few seconds)...");
    let mut last_error = None;
    let mut resolved = None;
    for index in 0..songs.len().min(3) {
        match rt.block_on(yt_dlp.resolve(&songs[index].video_id, cookies_file)) {
            Ok(r) => {
                let song = songs.remove(index);
                songs.insert(0, song);
                resolved = Some(r);
                break;
            }
            Err(e) => {
                ui::result(Outcome::Fail, &format!("Song {}: {e}", index + 1));
                if let ytfast_core::ytdlp::YtDlpError::Failed { details, .. } = &e
                    && !details.is_empty()
                {
                    ui::say("yt-dlp said:");
                    ui::say(details);
                }
                last_error = Some(e.to_string());
            }
        }
    }
    let Some(resolved) = resolved else {
        let detail = last_error.unwrap_or_else(|| "no song to try".into());
        report.step("Premium audio", Outcome::Fail, detail);
        return (songs, None);
    };

    let took = resolved.took.as_secs_f64();
    let premium = resolved.premium_formats();
    let found: Vec<String> = resolved.formats.iter().map(|f| f.describe()).collect();
    ui::say(&format!(
        "Found {} audio streams in {took:.1} s.",
        resolved.formats.len()
    ));
    if log::log_enabled!(log::Level::Debug) {
        for f in &found {
            ui::say(&format!("  {f}"));
        }
    }
    let best = resolved.best_playable();
    let (outcome, detail) = match (best, premium.is_empty()) {
        (Some(best), _) if best.is_premium() => (
            Outcome::Ok,
            format!(
                "Premium-quality audio: {} (found in {took:.1} s)",
                best.describe()
            ),
        ),
        (Some(best), false) => (
            Outcome::Fail,
            format!(
                "Premium quality was only offered as Opus, which YtFast cannot play yet; it would play {}",
                best.describe()
            ),
        ),
        (Some(best), true) => (
            Outcome::Fail,
            format!(
                "No Premium-quality (256 kbps) audio was offered for this song; it would play {}. Try another song with --song",
                best.describe()
            ),
        ),
        (None, _) => (
            Outcome::Fail,
            "No audio YtFast can play was offered".to_string(),
        ),
    };
    ui::result(outcome, &detail);
    report.step("Premium audio", outcome, detail);
    report.fact("Audio streams offered", found.join(", "));
    (songs, Some(resolved))
}

/// Which of `reported` are newer than everything that was in History
/// before playing. New plays go on top, above the old first entry; a song
/// that was already in History from earlier does not count.
fn new_in_history(before: &[String], after: &[String], reported: &[String]) -> usize {
    let marker = before.iter().find(|id| !reported.contains(id));
    let cutoff = marker
        .and_then(|m| after.iter().position(|id| id == m))
        .unwrap_or(after.len().min(30));
    let mut unique: Vec<&String> = reported.iter().collect();
    unique.sort();
    unique.dedup();
    unique
        .into_iter()
        .filter(|id| after[..cutoff].contains(id))
        .count()
}

fn step_history(
    rt: &Runtime,
    session: &Session,
    reported: &[String],
    before: &[String],
    report: &mut Report,
) {
    if reported.is_empty() {
        ui::result(Outcome::Skip, "Skipped: no song was played and reported.");
        report.step("History", Outcome::Skip, "nothing was played");
        return;
    }
    let mut expected: Vec<&String> = reported.iter().collect();
    expected.sort();
    expected.dedup();
    let expected = expected.len();
    ui::say("Waiting a few seconds for YouTube to update your History...");
    let mut found = 0;
    for wait in [5u64, 15] {
        std::thread::sleep(Duration::from_secs(wait));
        match rt.block_on(session.history()) {
            Ok(page) => {
                let after: Vec<String> = page.tracks.into_iter().map(|t| t.video_id).collect();
                found = new_in_history(before, &after, reported);
                if found == expected {
                    break;
                }
            }
            Err(e) => {
                ui::result(Outcome::Fail, &format!("Could not read your History: {e}"));
                report.step("History", Outcome::Fail, e.to_string());
                return;
            }
        }
    }
    let detail = format!("{found} of {expected} played songs appeared in History");
    if found == expected {
        ui::result(
            Outcome::Ok,
            &format!("{detail}. Your recommendations will keep learning."),
        );
        report.step("History", Outcome::Ok, detail);
    } else {
        ui::result(
            Outcome::Fail,
            &format!(
                "{detail}.\nIf your YouTube History is paused (myactivity.google.com), turn it on and try again."
            ),
        );
        report.step("History", Outcome::Fail, detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_links() {
        assert_eq!(video_id_from("dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(
            video_id_from("https://music.youtube.com/watch?v=3_R4ulvg8OY&list=RDAMVM3").as_deref(),
            Some("3_R4ulvg8OY")
        );
        assert_eq!(
            video_id_from("https://youtu.be/dQw4w9WgXcQ?si=x").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            video_id_from("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            video_id_from("https://music.youtube.com/playlist?list=PL123"),
            None
        );
        assert_eq!(video_id_from("hello"), None);
    }

    #[test]
    fn history_counts_only_new_plays() {
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let before = ids(&["old1", "old2", "song2"]);
        let reported = ids(&["song1", "song2"]);
        // Both plays went on top, above the old first entry.
        let after = ids(&["song2", "song1", "old1", "old2"]);
        assert_eq!(new_in_history(&before, &after, &reported), 2);
        // song2 was only there from before: it is below the old first entry.
        let after = ids(&["song1", "old1", "old2", "song2"]);
        assert_eq!(new_in_history(&before, &after, &reported), 1);
        // An empty History before: anything near the top counts.
        assert_eq!(new_in_history(&[], &ids(&["song1"]), &ids(&["song1"])), 1);
        // A song played twice counts once.
        assert_eq!(new_in_history(&[], &ids(&["a"]), &ids(&["a", "a"])), 1);
    }

    #[test]
    fn arguments_parse() {
        let args = Args::try_parse_from([
            "ytfast-check",
            "--browser",
            "firefox",
            "--song",
            "dQw4w9WgXcQ",
            "--count",
            "2",
        ])
        .unwrap();
        assert_eq!(args.browser.as_deref(), Some("firefox"));
        assert_eq!(args.songs, ["dQw4w9WgXcQ"]);
        assert_eq!(args.count, 2);
    }
}
