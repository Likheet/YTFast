//! What the window draws. Each view reads [`App`] and pushes actions.

mod backdrop;
mod dialogs;
mod now_playing;
mod page;
mod player_bar;
mod queue_panel;
mod settings;
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
    backdrop::paint(app, ui, ui.max_rect());
    if !matches!(app.auth, Auth::SignedIn { .. }) {
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| signin::show(app, ui));
        return;
    }
    // Panels first, in the order they take their space; the page fills
    // what is left.
    player_bar::show(app, ui);
    if app.now_playing {
        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin::symmetric(24, 8)))
            .show(ui, |ui| now_playing::show(app, ui));
        notice(app, ui);
        dialogs::show(app, ui);
        return;
    }
    sidebar::show(app, ui);
    if app.show_queue {
        queue_panel::show(app, ui);
    }
    topbar::show(app, ui);
    egui::CentralPanel::default()
        .frame(Frame::new().inner_margin(Margin::symmetric(28, 0)))
        .show(ui, |ui| page::show(app, ui));
    notice(app, ui);
    dialogs::show(app, ui);
}

/// "Add to playlist": the account's own playlists, from the Library.
pub fn playlists_menu(app: &App, ui: &mut egui::Ui, track: &ytfast_core::read::Track) {
    let own: Vec<(String, String)> = app.own_playlists();
    if own.is_empty() {
        return;
    }
    ui.menu_button("Add to playlist", |ui| {
        ui.set_min_width(200.0);
        for (id, title) in own {
            if ui.button(&title).clicked() {
                app.act(crate::app::Action::AddToPlaylist {
                    playlist_id: id,
                    title,
                    video_id: track.video_id.clone(),
                });
                ui.close();
            }
        }
    });
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
                .fill(egui::Color32::from_rgba_premultiplied(30, 30, 34, 235))
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(egui::CornerRadius::same(10))
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
