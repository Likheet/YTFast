//! The first screen: choosing the browser whose YouTube Music sign-in
//! YtFast borrows, and what happens while it signs in.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Sense, Vec2, vec2};
use ytfast_core::ytdlp::Browser;

use crate::app::{Action, App, Auth};
use crate::theme::{self, PALETTE};

pub fn show(app: &App, ui: &mut egui::Ui) {
    let width = 480.0_f32.min(ui.available_width() - 32.0);
    ui.add_space((ui.available_height() * 0.12).clamp(24.0, 120.0));
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        theme::paint_logo(ui, rect);
        ui.add_space(10.0);
        theme::label(ui, "YTFast", theme::bold(36.0), PALETTE.text);
        theme::label(
            ui,
            "YouTube Music, native and fast.",
            theme::regular(16.0),
            PALETTE.secondary,
        );
        ui.add_space(28.0);

        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            Frame::new()
                .fill(PALETTE.panel)
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    card(app, ui);
                });
        });
    });
}

fn card(app: &App, ui: &mut egui::Ui) {
    theme::label(ui, "Sign in", theme::bold(20.0), PALETTE.text);
    ui.add_space(6.0);
    let explain = "YTFast uses the YouTube Music sign-in from your web browser. \
                   Choose the browser you use music.youtube.com in:";
    ui.label(
        egui::RichText::new(explain)
            .font(theme::regular(14.0))
            .color(PALETTE.secondary),
    );
    ui.add_space(12.0);

    let (chosen, working) = match &app.auth {
        Auth::Choosing { browser } | Auth::Failed { browser, .. } => (*browser, false),
        Auth::Working { browser, .. } => (*browser, true),
        Auth::SignedIn { .. } => return,
    };
    ui.add_enabled_ui(!working, |ui| {
        for browser in Browser::ALL {
            browser_row(app, ui, browser, browser == chosen);
        }
    });
    if cfg!(windows) {
        ui.add_space(6.0);
        let note = "On Windows, Chrome, Edge and Brave lock their sign-in data. \
                    Use Firefox: install it, sign in to music.youtube.com there once, then choose it here.";
        ui.label(
            egui::RichText::new(note)
                .font(theme::regular(12.5))
                .color(PALETTE.dim),
        );
    }
    ui.add_space(16.0);

    match &app.auth {
        Auth::Working { progress, .. } => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(18.0).color(PALETTE.text));
                ui.label(
                    egui::RichText::new(progress)
                        .font(theme::regular(14.0))
                        .color(PALETTE.text),
                );
            });
        }
        Auth::Failed { message, .. } => {
            ui.label(
                egui::RichText::new(message)
                    .font(theme::regular(14.0))
                    .color(PALETTE.danger),
            );
            ui.add_space(10.0);
            if theme::pill_button(ui, "Try again", true).clicked() {
                app.act(Action::SignIn);
            }
        }
        _ => {
            if theme::pill_button(ui, "Continue", true).clicked() {
                app.act(Action::SignIn);
            }
        }
    }
    ui.add_space(16.0);
    let privacy = "Only YouTube's part of the browser's sign-in is read. It stays on this computer \
                   and is only ever sent to YouTube.";
    ui.label(
        egui::RichText::new(privacy)
            .font(theme::regular(12.5))
            .color(PALETTE.dim),
    );
}

fn browser_row(app: &App, ui: &mut egui::Ui, browser: Browser, selected: bool) {
    let problem = browser.problem_here();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let fill = if selected {
        PALETTE.surface_hover
    } else if response.hovered() && problem.is_none() {
        PALETTE.surface
    } else {
        PALETTE.panel
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    // A radio dot.
    let dot = rect.left_center() + vec2(18.0, 0.0);
    ui.painter().circle_stroke(
        dot,
        7.0,
        egui::Stroke::new(
            1.5,
            if problem.is_some() {
                PALETTE.dim
            } else {
                PALETTE.secondary
            },
        ),
    );
    if selected {
        ui.painter().circle_filled(dot, 4.0, PALETTE.accent);
    }
    let color = if problem.is_some() {
        PALETTE.dim
    } else {
        PALETTE.text
    };
    ui.painter().text(
        dot + vec2(18.0, 0.0),
        egui::Align2::LEFT_CENTER,
        browser.label(),
        theme::medium(14.5),
        color,
    );
    if let Some(problem) = problem {
        let note = if cfg!(windows) {
            "does not work on Windows"
        } else {
            "not on this computer"
        };
        ui.painter().text(
            rect.right_center() - vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            note,
            theme::regular(12.5),
            PALETTE.dim,
        );
        response.on_hover_text(problem);
    } else if response.clicked() {
        app.act(Action::ChooseBrowser(browser));
    }
}
