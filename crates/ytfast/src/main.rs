//! YtFast: YouTube Music, native and fast.
//!
//! `ytfast --demo` opens the window with made-up music, no account and no
//! network: for trying the interface and for screenshots.

// A window, not a console, on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_thread;
mod backend;
mod demo;
mod images;
mod queue;
mod theme;
mod views;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    env_logger::Builder::new()
        .filter_level(if verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Warn
        })
        .init();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
        .expect("the icon is a PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YtFast")
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
