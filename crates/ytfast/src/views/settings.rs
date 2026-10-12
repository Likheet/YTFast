//! YTFast's settings, few and short (touched up at the owner's word, 11
//! October 2026): the look, a few switches, updates, the account, and
//! where problems are noted. YouTube Music's look and Dynamic Background
//! put each setting in the same place; Premium sets each on a card of its
//! own.

use egui::{Align, Layout, Rect, UiBuilder, pos2, vec2};

use crate::app::{Action, App, Auth, Setting};
use crate::theme::{self, PALETTE, Theme};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("settings")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // One column in the middle of the page, as much room either
            // side of it: at most 640 wide (Premium's cards 760).
            let most = if theme::premium() { 760.0 } else { 640.0 };
            let full = ui.max_rect();
            // At least 24 clear of the menu and the window's edge.
            let width = (full.width() - 48.0).clamp(0.0, most);
            let column = Rect::from_min_size(
                pos2(full.center().x - width / 2.0, ui.cursor().top()),
                vec2(width, full.height()),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(column)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_width(width);
                    contents(app, ui);
                },
            );
        });
}

/// Everything Settings shows, from its title down: the look, playback,
/// updates, the account and about, each a short section of one-line
/// settings.
fn contents(app: &App, ui: &mut egui::Ui) {
    // Premium: a larger title, and each setting on a card.
    let (above, size, below) = if theme::premium() {
        (32.0, 36.0, 24.0)
    } else {
        (20.0, 30.0, 18.0)
    };
    ui.add_space(above);
    theme::label(ui, "Settings", theme::bold(size), PALETTE.text);
    ui.add_space(below);

    heading(ui, "Appearance");
    looks(app, ui);

    section(ui, "Playback");
    switch(
        app,
        ui,
        "Even out loudness",
        "Loud songs play at the level YouTube Music plays them at.",
        app.settings.even_loudness,
        Setting::EvenLoudness,
    );
    switch(
        app,
        ui,
        "Skip songs you dislike",
        "Disliking the song playing moves on to the next, as YouTube Music does.",
        app.settings.skip_disliked,
        Setting::SkipDisliked,
    );
    switch(
        app,
        ui,
        "Translate lyrics",
        "Each line in English too, and in Latin letters for other scripts.",
        app.settings.translate_lyrics,
        Setting::TranslateLyrics,
    );
    switch(
        app,
        ui,
        "Keep playing when closed",
        KEEP_RUNNING_ABOUT[usize::from(cfg!(target_os = "macos"))],
        app.settings.keep_running,
        Setting::KeepRunning,
    );
    // Closing stops the music only with the switch above off.
    if !app.settings.keep_running {
        switch(
            app,
            ui,
            "Ask before closing while a song plays",
            "Closing YTFast while a song plays asks first.",
            app.settings.confirm_close,
            Setting::ConfirmClose,
        );
    }
    switch(
        app,
        ui,
        "Start with the computer",
        OPEN_AT_LOGIN_ABOUT[usize::from(cfg!(target_os = "macos"))],
        app.settings.open_at_login,
        Setting::OpenAtLogin,
    );

    if !app.demo {
        section(ui, "Updates");
        updates(app, ui);
    }

    section(ui, "Account");
    let signed_in = match &app.auth {
        Auth::SignedIn { name, .. } => match app.settings.browser.as_deref() {
            Some(browser) => format!("Signed in as {name} ({browser})."),
            None => format!("Signed in as {name}."),
        },
        _ => "Not signed in.".to_string(),
    };
    let sign_out = (!app.demo).then_some(("Sign out", Action::SignOut));
    words_and_button(app, ui, &signed_in, sign_out);

    section(ui, "About");
    words_and_button(app, ui, crate::app::VERSION, None);
    words_and_button(
        app,
        ui,
        LOG_WORDS,
        Some(("Open the log's folder", Action::OpenLogFolder)),
    );
    ui.add_space(40.0);
}

/// Where problems are noted, beside the button that opens its folder.
const LOG_WORDS: &str = "Problems are noted in ytfast.log, free of passwords.";

/// How wide the words beside a button may be: the column less room for
/// the button.
const WORDS_BESIDE_BUTTON: f32 = 640.0 - 200.0;

/// The space before a section, and its heading.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(18.0);
    heading(ui, text);
}

/// A row of words (14, quieter) and at its right, when there is one, a
/// button.
fn words_and_button(app: &App, ui: &mut egui::Ui, words: &str, button: Option<(&str, Action)>) {
    row(ui, |ui| {
        ui.set_min_height(36.0);
        ui.vertical(|ui| {
            ui.set_max_width(WORDS_BESIDE_BUTTON.min(ui.available_width()));
            text(ui, words);
        });
        if let Some((label, action)) = button {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill_button(ui, label, false).clicked() {
                    app.act(action);
                }
            });
        }
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

/// A setting's row; on a glass card in the Premium theme (white@0.05, a
/// white@0.06 edge, r 12, 16 in), so the page's colours show through.
/// Returns its rectangle.
fn row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> egui::Rect {
    if !theme::premium() {
        let rect = ui.horizontal(add).response.rect;
        ui.add_space(12.0);
        return rect;
    }
    let rect = egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(13))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(16)))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| ui.horizontal(add))
        .response
        .rect;
    ui.add_space(8.0);
    rect
}

/// A row with a title, an explanation, and an on/off switch.
fn switch(app: &App, ui: &mut egui::Ui, title: &str, about: &str, on: bool, which: Setting) {
    row(ui, |ui| {
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
}

/// The looks, as three buttons in a row, the one worn lit (each named for
/// screen readers, so the demo can choose one by name); under them, in one
/// line, what the look worn is, or for Dynamic Background its own switch
/// (its colours moving). The line is as tall in every look, so nothing
/// under it moves when the look changes.
fn looks(app: &App, ui: &mut egui::Ui) {
    let worn = app.settings.theme;
    row(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for choice in Theme::ALL {
                    let (name, _) = choice.words();
                    if theme::chip(ui, name, choice == worn, 36.0).clicked() && choice != worn {
                        app.act(Action::SetTheme(choice));
                    }
                }
            });
            ui.add_space(12.0);
            let (line, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
            let mut line_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(line)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if worn == Theme::DynamicBackground {
                line_ui.vertical(|ui| {
                    theme::label(ui, "Moving background", theme::medium(15.5), PALETTE.text);
                    ui.label(
                        egui::RichText::new("The song's colours drift slowly while it plays.")
                            .font(theme::regular(13.0))
                            .color(PALETTE.secondary),
                    );
                });
                line_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::toggle(ui, app.settings.moving_background, "Moving background")
                        .clicked()
                    {
                        app.act(Action::Toggle(Setting::MovingBackground));
                    }
                });
            } else {
                let (_, about) = worn.words();
                text(&mut line_ui, &format!("{about}."));
            }
        });
    });
}

/// What "Keep playing when closed" says, on Windows (its icon by the
/// clock) and on a Mac (in the menu bar).
const KEEP_RUNNING_ABOUT: [&str; 2] = [
    "Closing the window leaves the music on; the icon by the clock reopens it.",
    "Closing the window leaves the music on; the menu bar icon reopens it.",
];

/// What "Start with the computer" says, on Windows and on a Mac.
const OPEN_AT_LOGIN_ABOUT: [&str; 2] = [
    "YTFast opens by the clock when you sign in, without its window.",
    "YTFast opens in the menu bar when you log in, without its window.",
];

/// What "Download updates automatically" says, on Windows and on a Mac
/// (where only a copy in Applications installs its updates).
const UPDATES_ABOUT: [&str; 2] = [
    "New versions download in the background; Restart to update installs them.",
    "New versions download in the background, to a copy in Applications.",
];

/// The updates switch, what the updater is doing, and the button that
/// fits it: look now, install the version found, or restart to install
/// the one downloaded.
fn updates(app: &App, ui: &mut egui::Ui) {
    use crate::update::State;
    switch(
        app,
        ui,
        "Download updates automatically",
        UPDATES_ABOUT[usize::from(cfg!(target_os = "macos"))],
        app.settings.auto_update,
        Setting::AutoUpdate,
    );
    let Some(updates) = &app.updates else {
        return;
    };
    let current = env!("CARGO_PKG_VERSION");
    let check = Some(("Check for updates", false, Action::CheckForUpdates));
    let (words, button) = match updates.state() {
        State::Idle => (format!("This is YTFast {current}."), check),
        State::Checking => ("Looking for a new version...".to_string(), None),
        State::UpToDate => (format!("YTFast {current} is the newest version."), check),
        State::Available {
            version,
            note: Some(note),
        } => (format!("YTFast {version} is out. {note}"), None),
        State::Available {
            version,
            note: None,
        } => (
            format!("YTFast {version} is out."),
            Some(("Install it", true, Action::InstallUpdate)),
        ),
        State::Downloading {
            version,
            received,
            total,
        } => {
            let percent = received.saturating_mul(100).checked_div(total).unwrap_or(0);
            (format!("Downloading YTFast {version}... {percent}%"), None)
        }
        State::Ready { version } => (
            format!("YTFast {version} is ready to install."),
            Some(("Restart to update", true, Action::RestartToUpdate)),
        ),
        State::Restarting => ("Restarting...".to_string(), None),
        State::Failed(message) => (message, check),
    };
    row(ui, |ui| {
        ui.set_min_height(36.0);
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 200.0).max(0.0));
            text(ui, &words);
        });
        if let Some((label, primary, action)) = button {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill_button(ui, label, primary).clicked() {
                    app.act(action);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many lines words take, `size` big in `width`, in Roboto or in
    /// Dynamic Background's Inter, on a screen of `pixels_per_point`.
    fn lines(text: &str, size: f32, width: f32, inter: bool, pixels_per_point: f32) -> usize {
        let ctx = egui::Context::default();
        let mut fonts = theme::install(&ctx);
        fonts.set_inter_first(inter);
        ctx.set_pixels_per_point(pixels_per_point);
        let mut count = 0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            count = ui
                .fonts_mut(|f| {
                    f.layout(
                        text.to_string(),
                        theme::regular(size),
                        PALETTE.secondary,
                        width,
                    )
                })
                .rows
                .len();
        });
        output.textures_delta.clear();
        count
    }

    /// Settings' words take one line in Dynamic Background's font as in
    /// Roboto, the updates' on Windows and on a Mac (whose words differ),
    /// at the owner's screens' scales: so choosing that look moves no
    /// setting (on the Mac the updates' once took a line more).
    #[test]
    fn settings_words_take_one_line_in_every_look() {
        // A switch's explanation: 13, in the 570 the column leaves it.
        let explanations = UPDATES_ABOUT
            .iter()
            .chain(&KEEP_RUNNING_ABOUT)
            .chain(&OPEN_AT_LOGIN_ABOUT)
            .map(|text| (*text, 13.0, 570.0));
        // Words beside a button: 14.
        let beside = [(LOG_WORDS, 14.0, WORDS_BESIDE_BUTTON)];
        for (text, size, width) in explanations.chain(beside) {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for inter in [false, true] {
                    assert_eq!(lines(text, size, width, inter, scale), 1, "{scale}: {text}");
                }
            }
        }
    }
}
