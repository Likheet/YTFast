//! YTFast's settings: a few switches, the account, and where problems are
//! noted.

use crate::app::{Action, App, Auth, Setting};
use crate::theme::{self, PALETTE};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("settings")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(640.0);
            ui.add_space(20.0);
            theme::label(ui, "Settings", theme::bold(30.0), PALETTE.text);
            ui.add_space(18.0);

            heading(ui, "Playback");
            switch(
                app,
                ui,
                "Start songs the fast way",
                "Songs start in about a second, the way the website does it. Turn this off if songs fail to start: YTFast then always uses yt-dlp, which takes about ten seconds per song.",
                app.settings.fast_way,
                Setting::FastWay,
            );
            switch(
                app,
                ui,
                "Keep playing when the queue ends",
                "Carry on with songs like the last one, as YouTube Music's autoplay does.",
                app.settings.autoplay,
                Setting::Autoplay,
            );
            switch(
                app,
                ui,
                "Even out loudness",
                "Turn loud songs down to the level YouTube Music plays them at, from the next song.",
                app.settings.even_loudness,
                Setting::EvenLoudness,
            );

            ui.add_space(18.0);
            heading(ui, "Account");
            if let Auth::SignedIn { name } = &app.auth {
                let browser = app.settings.browser.as_deref().unwrap_or("your browser");
                text(ui, &format!("Signed in as {name}, with the sign-in from {browser}."));
            }
            ui.add_space(8.0);
            if !app.demo && theme::pill_button(ui, "Sign out", false).clicked() {
                app.act(Action::SignOut);
            }

            ui.add_space(18.0);
            heading(ui, "About");
            text(ui, crate::app::VERSION);
            text(
                ui,
                "Problems are noted in ytfast.log, in YTFast's cache folder. It has no passwords or cookies in it, so it is safe to send.",
            );
            ui.add_space(8.0);
            if theme::pill_button(ui, "Open that folder", false).clicked() {
                app.act(Action::OpenLogFolder);
            }
            ui.add_space(40.0);
        });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    theme::label(ui, text, theme::bold(20.0), PALETTE.text);
    ui.add_space(8.0);
}

fn text(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(14.0))
            .color(PALETTE.secondary),
    );
}

/// A row with a title, an explanation, and an on/off switch.
fn switch(app: &App, ui: &mut egui::Ui, title: &str, about: &str, on: bool, which: Setting) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 70.0).max(0.0));
            theme::label(ui, title, theme::medium(15.5), PALETTE.text);
            ui.label(
                egui::RichText::new(about)
                    .font(theme::regular(13.0))
                    .color(PALETTE.secondary),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // YouTube Music's switch, named for screen readers.
            if theme::toggle(ui, on, title).clicked() {
                app.act(Action::Toggle(which));
            }
        });
    });
    ui.add_space(12.0);
}
