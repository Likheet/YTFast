//! Small windows that ask one thing: a new playlist's name, a new name,
//! or "really delete?".

use crate::app::{Action, App, Dialog};
use crate::backend::Edit;
use crate::theme::{self, PALETTE};

pub fn show(app: &App, ui: &egui::Ui) {
    let mut dialog = app.dialog.borrow_mut();
    let Some(current) = dialog.as_mut() else {
        return;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("dialog"))
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(24, 24, 28, 250))
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(egui::CornerRadius::same(14))
                .inner_margin(egui::Margin::same(22)),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(360.0);
            match current {
                Dialog::NewPlaylist { name, song } => {
                    theme::label(ui, "New playlist", theme::bold(20.0), PALETTE.text);
                    ui.add_space(12.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("Name")
                            .font(theme::regular(15.0))
                            .desired_width(f32::INFINITY),
                    );
                    field.request_focus();
                    ui.add_space(16.0);
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if buttons(ui, "Make it") || (enter && !name.trim().is_empty()) {
                        if !name.trim().is_empty() {
                            app.act(Action::Edit(Edit::CreatePlaylist {
                                title: name.trim().to_string(),
                                video_ids: song.iter().cloned().collect(),
                            }));
                        }
                        close = true;
                    }
                }
                Dialog::Rename { playlist_id, name } => {
                    theme::label(ui, "Rename playlist", theme::bold(20.0), PALETTE.text);
                    ui.add_space(12.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .font(theme::regular(15.0))
                            .desired_width(f32::INFINITY),
                    );
                    field.request_focus();
                    ui.add_space(16.0);
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (buttons(ui, "Rename") || enter) && !name.trim().is_empty() {
                        app.act(Action::RenamePlaylist {
                            playlist_id: playlist_id.clone(),
                            name: name.trim().to_string(),
                        });
                        close = true;
                    }
                }
                Dialog::Delete { playlist_id, title } => {
                    theme::label(ui, "Delete this playlist?", theme::bold(20.0), PALETTE.text);
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("\u{201c}{title}\u{201d} will be deleted from your account. This cannot be undone."))
                            .font(theme::regular(14.0))
                            .color(PALETTE.secondary),
                    );
                    ui.add_space(16.0);
                    if buttons(ui, "Delete") {
                        app.act(Action::DeletePlaylist(playlist_id.clone()));
                        close = true;
                    }
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) || ui.data(|d| d.get_temp::<bool>(egui::Id::new("dialog-cancel"))) == Some(true) {
                ui.data_mut(|d| d.remove::<bool>(egui::Id::new("dialog-cancel")));
                close = true;
            }
        });
    if modal.should_close() || close {
        *dialog = None;
    }
}

/// Cancel and the main button; true when the main one is pressed.
fn buttons(ui: &mut egui::Ui, main: &str) -> bool {
    let mut pressed = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        pressed = theme::pill_button(ui, main, true).clicked();
        if theme::pill_button(ui, "Cancel", false).clicked() {
            ui.data_mut(|d| d.insert_temp(egui::Id::new("dialog-cancel"), true));
        }
    });
    pressed
}
