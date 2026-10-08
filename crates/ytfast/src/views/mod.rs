//! What the window draws, laid out as YouTube Music lays its page out: a
//! bar across the top, the menu on the left, the page, and the player bar
//! across the bottom once something plays. Each view reads [`App`] and
//! pushes actions.

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
    backdrop::paint(ui, ui.max_rect());
    if !matches!(app.auth, Auth::SignedIn { .. }) {
        // Signed out by YouTube while a song plays: it can still be paused.
        if app.playback.entry.is_some() {
            player_bar::show(app, ui);
        }
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| signin::show(app, ui));
        return;
    }
    // Panels first, in the order they take their space: the two bars span
    // the window, the menu sits between them, and the page fills the rest.
    if app.playback.entry.is_some() {
        player_bar::show(app, ui);
    }
    topbar::show(app, ui);
    sidebar::show(app, ui);
    egui::CentralPanel::default()
        .frame(Frame::new())
        .show(ui, |ui| {
            if app.now_playing {
                now_playing::show(app, ui);
            } else {
                page::show(app, ui);
            }
        });
    notice(app, ui);
    dialogs::show(app, ui);
}

/// "Add to playlist": a new playlist, then the account's own playlists,
/// from the Library (a list that scrolls when long).
pub fn playlists_menu(app: &App, ui: &mut egui::Ui, track: &ytfast_core::read::Track) {
    use crate::app::{Action, Dialog};
    ui.menu_button("Add to playlist", |ui| {
        theme::menu(ui);
        if ui.button("New playlist").clicked() {
            app.act(Action::OpenDialog(Dialog::NewPlaylist {
                name: String::new(),
                song: Some(track.video_id.clone()),
            }));
            ui.close();
        }
        let own = app.own_playlists();
        if own.is_empty() {
            return;
        }
        ui.separator();
        // About a dozen rows, then it scrolls.
        egui::ScrollArea::vertical()
            .id_salt("add-to-playlist")
            .max_height(480.0)
            .show(ui, |ui| {
                for (id, title) in own {
                    if ui.button(&title).clicked() {
                        app.act(Action::AddToPlaylist {
                            playlist_id: id,
                            title,
                            video_id: track.video_id.clone(),
                        });
                        ui.close();
                    }
                }
            });
    });
}

/// A short message ("Added to the queue") at the bottom left, just above
/// the player bar, as YouTube Music shows its own.
fn notice(app: &App, ui: &egui::Ui) {
    let Some((text, _)) = &app.notice else { return };
    let bar = if app.playback.entry.is_some() {
        theme::PLAYER_BAR_HEIGHT
    } else {
        0.0
    };
    egui::Area::new(egui::Id::new("notice"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(24.0, -(bar + 24.0)))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(PALETTE.panel)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(Margin::symmetric(24, 14))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .font(theme::regular(14.0))
                            .color(PALETTE.text),
                    );
                });
        });
}
