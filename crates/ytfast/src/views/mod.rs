//! What the window draws. Each view reads [`App`] and pushes actions.

mod page;
mod player_bar;
mod queue_panel;
mod sidebar;
mod signin;
mod topbar;
mod widgets;

use egui::{Frame, Margin};

use crate::app::{App, Auth};
use crate::theme::{self, PALETTE};

/// The search box's ID, to focus it from a shortcut.
pub const SEARCH_BOX: &str = "ytfast-search-box";

pub fn show(app: &App, ui: &mut egui::Ui) {
    if !matches!(app.auth, Auth::SignedIn { .. }) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(PALETTE.window))
            .show(ui, |ui| signin::show(app, ui));
        return;
    }
    // Panels first, in the order they take their space; the page fills
    // what is left.
    player_bar::show(app, ui);
    sidebar::show(app, ui);
    if app.show_queue {
        queue_panel::show(app, ui);
    }
    topbar::show(app, ui);
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(PALETTE.window)
                .inner_margin(Margin::symmetric(32, 0)),
        )
        .show(ui, |ui| page::show(app, ui));
    notice(app, ui);
}

/// A short message ("Added to the queue") just above the player bar.
fn notice(app: &App, ui: &egui::Ui) {
    let Some((text, _)) = &app.notice else { return };
    egui::Area::new(egui::Id::new("notice"))
        .order(egui::Order::Foreground)
        .anchor(
            egui::Align2::CENTER_BOTTOM,
            egui::vec2(0.0, -(theme::PLAYER_BAR_HEIGHT + 16.0)),
        )
        .interactable(false)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(PALETTE.surface_hover)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .font(theme::regular(14.0))
                            .color(PALETTE.text),
                    );
                });
        });
}
