//! YtFast: YouTube Music, native and fast.
//!
//! `ytfast --demo` opens the window with made-up music, no account and no
//! network: for trying the interface and for screenshots.

// A window, not a console, on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_thread;
mod backend;
mod colors;
mod demo;
mod discord;
mod dynamic;
mod images;
mod instance;
mod login;
mod lyrics;
mod queue;
mod radar_files;
mod store;
mod swipe;
mod theme;
mod tray;
mod update;
mod update_mac;
mod views;

/// Problems go to `ytfast.log` in YtFast's cache folder (made new each
/// run), with web addresses cut to their site, so the file is safe to
/// send. On Windows there is no console to show them. The demo keeps a
/// file of its own, so it can be open beside YTFast in real use.
fn start_log(verbose: bool, demo: bool) {
    use std::io::Write;
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(if verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Warn
        })
        // Its notes name part of Musixmatch's session token.
        .filter_module("musixmatch_inofficial", log::LevelFilter::Warn)
        .format(|out, record| {
            let message = ytfast_core::redact::urls(&record.args().to_string());
            writeln!(
                out,
                "{} {:5} {message}",
                out.timestamp_seconds(),
                record.level()
            )
        });
    let name = if demo {
        "ytfast-demo.log"
    } else {
        "ytfast.log"
    };
    let file = directories::ProjectDirs::from("", "", "YtFast").and_then(|dirs| {
        std::fs::create_dir_all(dirs.cache_dir()).ok()?;
        std::fs::File::create(dirs.cache_dir().join(name)).ok()
    });
    if let Some(file) = file {
        builder.target(env_logger::Target::Pipe(Box::new(file)));
    }
    builder.init();
    // Which build wrote the log, whatever else it holds.
    log::warn!("{} started", app::VERSION);
    // A crash says why in the log, too.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("YtFast stopped: {info}");
        default_hook(info);
    }));
}

fn main() -> eframe::Result<()> {
    // First of all: a run that is the updater's helper installs the new
    // version and ends here; the update flags come off the command line.
    let launch = fastframe_update::intercept(&update::CONFIG);
    let args: Vec<String> = launch
        .arguments
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    // The updater asks a downloaded copy its version before installing it
    // (before the log, which this run must not make new).
    if args.iter().any(|a| a == "--version") {
        println!("ytfast {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--update-dry-run") {
        std::process::exit(update::dry_run());
    }
    // On a Mac, the helper that puts a new version in place once YTFast
    // has closed (`update_mac`).
    if args.first().map(String::as_str) == Some("--finish-mac-update") {
        std::process::exit(update_mac::finish(&args[1..]));
    }
    // And the one that moves YTFast from its disk image to Applications.
    if args.first().map(String::as_str) == Some("--finish-mac-move") {
        std::process::exit(update_mac::finish_move(&args[1..]));
    }
    let demo = args.iter().any(|a| a == "--demo");
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    // The YTFast running (the demo, with `--demo`) quits: for scripts.
    if args.iter().any(|a| a == "--quit") {
        instance::quit_running(demo);
        return Ok(());
    }
    // Started with the computer: no window, only the icon by the clock.
    let hidden = args.iter().any(|a| a == login::HIDDEN);
    // One YTFast at a time: a second launch shows the first one's window
    // and ends here, before its log (which would empty the first one's).
    let waker = fastframe_shell::Waker::default();
    let asked = std::sync::Arc::new(instance::Asked::default());
    let request = if hidden {
        instance::request::START
    } else {
        instance::request::SHOW
    };
    let Some(_running) = instance::claim(demo, request, &asked, &waker) else {
        return Ok(());
    };
    start_log(verbose, demo);
    if let Some(error) = &launch.error {
        log::warn!("updates: {error}");
    }
    // A Mac update just installed (or not): said once.
    let mac_update = if demo {
        None
    } else {
        update_mac::take_result()
    };
    let startup = app::Startup {
        update_error: launch.error.or(mac_update),
        receipt: launch.receipt,
    };

    // The settings and the queue, as eframe kept them; the demo keeps its
    // window size and the like in a file of its own: it may be open beside
    // YTFast in real use, and the real app's file holds its settings,
    // which the demo would write back as they were when it opened.
    let store = store::Store::for_app(demo);
    let persistence_path = store
        .as_ref()
        .filter(|_| demo)
        .map(|store| store.path().to_path_buf());
    let app = app::App::new(&waker, demo, startup, store, asked);
    // Each window is made by `run_native`; closed with "Keep playing when
    // closed" on, YTFast runs on without one until a window is wanted.
    let shell = fastframe_shell::Shell::new(app, &waker).start_hidden(hidden);
    // A Mac's menu bar item answers only while AppKit runs. Elsewhere the
    // shell sleeps, waking when the player, a media key or the icon wakes it.
    #[cfg(target_os = "macos")]
    let shell = shell.idle(fastframe_tray::idle);
    let result = shell.run(|lease| {
        eframe::run_native(
            "YtFast",
            native_options(persistence_path.clone()),
            Box::new(move |cc| {
                let mut app = lease.take(&cc.egui_ctx);
                app.attach(&cc.egui_ctx);
                Ok(Box::new(app::AppWindow::new(app)))
            }),
        )
    });
    // On Windows there is no console to see this on.
    if let Err(e) = &result {
        log::error!("the window stopped: {e}");
    }
    log::info!("closed");
    result
}

/// How each window is made.
fn native_options(persistence_path: Option<std::path::PathBuf>) -> eframe::NativeOptions {
    // On the Mac the app's own icon (YTFast.app's, with the margins every
    // Mac icon has) stays in the Dock: eframe would put this one, which
    // fills its square, in its place while YTFast runs. The default icon
    // tells it to leave the Dock alone.
    let icon = if cfg!(target_os = "macos") {
        egui::IconData::default()
    } else {
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
            .expect("the icon is a PNG")
    };
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YTFast")
            .with_icon(std::sync::Arc::new(icon))
            .with_app_id(store::APP_ID)
            .with_inner_size(app::WINDOW_SIZE)
            .with_min_inner_size(app::MIN_WINDOW_SIZE)
            // On Windows, YTFast's top bar is the title bar, with buttons of
            // its own (`views::window_frame`).
            .with_decorations(!cfg!(windows)),
        persistence_path,
        ..Default::default()
    }
}
