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
mod images;
mod lyrics;
mod queue;
mod theme;
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
    // A crash says why in the log, too.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("YtFast stopped: {info}");
        default_hook(info);
    }));
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    start_log(verbose, demo);

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
        .expect("the icon is a PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YTFast")
            .with_icon(std::sync::Arc::new(icon))
            .with_app_id("ytfast")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "YtFast",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, demo)))),
    )
}
