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
    let frame = if matches!(current, Dialog::SaveToPlaylist { .. }) {
        // `ytmusic-add-to-playlist-renderer`.
        egui::Frame::new()
            .fill(PALETTE.panel)
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(2))
    } else {
        egui::Frame::new()
            .fill(egui::Color32::from_rgba_premultiplied(24, 24, 28, 250))
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(22))
    };
    let modal = egui::Modal::new(egui::Id::new("dialog"))
        .frame(frame)
        // YouTube Music dims the window behind a dialog by half.
        .backdrop_color(egui::Color32::from_black_alpha(128))
        .show(ui.ctx(), |ui| {
            if let Dialog::SaveToPlaylist { video_id } = current {
                close = save_to_playlist(app, ui, video_id);
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                return;
            }
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
                    // Asked before the field takes the focus back, which
                    // would hide that Enter just took it away.
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    field.request_focus();
                    ui.add_space(16.0);
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
                    // As above: asked before the field takes the focus back.
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    field.request_focus();
                    ui.add_space(16.0);
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
                Dialog::SaveToPlaylist { .. } => {}
                // The whole description (its look is not measured yet).
                Dialog::Description { title, text } => {
                    theme::label(ui, title, theme::bold(20.0), PALETTE.text);
                    ui.add_space(12.0);
                    egui::ScrollArea::vertical()
                        .max_height(ui.ctx().content_rect().height() * 0.6)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(text.as_str())
                                    .font(theme::regular(14.0))
                                    .color(PALETTE.secondary),
                            );
                        });
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::pill_button(ui, "Close", false).clicked() {
                            ui.data_mut(|d| d.insert_temp(egui::Id::new("dialog-cancel"), true));
                        }
                    });
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

/// "Save to playlist", as YouTube Music's (`ytmusic-add-to-playlist-renderer`):
/// a bar with the title and ×, then "All playlists" and the account's
/// playlists, which scroll, and "New playlist" at the bottom right. True
/// when it is done.
fn save_to_playlist(app: &App, ui: &mut egui::Ui, video_id: &str) -> bool {
    use egui::{Align2, Rect, Sense, pos2, vec2};
    let width = 382.0; // 384 with the border
    ui.set_width(width);
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let mut done = false;

    // The bar: at least 64 high, the title 24 in, × 17 from the right.
    let (bar, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
    ui.painter().text(
        pos2(bar.left() + 24.0, bar.center().y),
        Align2::LEFT_CENTER,
        "Save to playlist",
        theme::bold(24.0),
        PALETTE.text,
    );
    let close = Rect::from_center_size(
        pos2(bar.right() - 17.0 - 20.0, bar.center().y),
        egui::Vec2::splat(40.0),
    );
    let mut corner = ui.new_child(egui::UiBuilder::new().max_rect(close));
    if theme::round_button(
        &mut corner,
        theme::Icon::Close,
        40.0,
        24.0,
        theme::Round::Plain,
        PALETTE.dim,
        "Close",
    )
    .clicked()
    {
        done = true;
    }
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        egui::Stroke::new(1.0, PALETTE.outline),
    );

    // The list: "All playlists" (20 above it, 7 under), the rows, and 64
    // of room for "New playlist"; at most three quarters of the window
    // high with the bar, then it scrolls.
    let playlists = app.own_playlist_cards();
    let content = 20.0 + 16.8 + 7.0 + 64.0 * playlists.len() as f32 + 64.0;
    let tallest = (ui.ctx().content_rect().height() * 0.75 - 64.0).max(128.0);
    let (list, _) = ui.allocate_exact_size(vec2(width, content.min(tallest)), Sense::hover());
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("save-to-playlist")
        .max_height(list.height())
        .auto_shrink([false, false])
        .show(&mut list_ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.add_space(20.0);
            let (heading, _) = ui.allocate_exact_size(vec2(width, 16.8), Sense::hover());
            ui.painter().text(
                pos2(heading.left() + 24.0, heading.center().y),
                Align2::LEFT_CENTER,
                "All playlists",
                theme::medium(14.0),
                PALETTE.text,
            );
            ui.add_space(7.0);
            for (playlist_id, card) in playlists {
                if playlist_row(app, ui, &card) {
                    app.act(Action::AddToPlaylist {
                        playlist_id,
                        title: card.title.clone(),
                        video_id: video_id.to_string(),
                    });
                    done = true;
                }
            }
            // Room for "New playlist" over the list's end.
            ui.add_space(64.0);
        });

    // "New playlist", 17 in from the bottom right, over the list.
    let place = Rect::from_min_max(
        pos2(list.left(), list.bottom() - 17.0 - 36.0),
        pos2(list.right() - 17.0, list.bottom() - 17.0),
    );
    let mut corner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(place)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if theme::pill(
        &mut corner,
        Some(theme::Icon::Plus),
        "New playlist",
        theme::Pill::Filled,
    )
    .clicked()
    {
        app.act(Action::OpenDialog(Dialog::NewPlaylist {
            name: String::new(),
            song: Some(video_id.to_string()),
        }));
        done = true;
    }
    done
}

/// One playlist in "Save to playlist": 64 high, its cover 44 (r 2) 24 in,
/// its name and the line under it 16 after; white@0.05 under the pointer.
/// True when clicked.
fn playlist_row(app: &App, ui: &mut egui::Ui, card: &ytfast_core::read::Card) -> bool {
    use egui::{Rect, Sense, pos2, vec2};
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_white_alpha(13));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let cover = Rect::from_min_size(
            pos2(rect.left() + 24.0, rect.center().y - 22.0),
            egui::Vec2::splat(44.0),
        );
        crate::views::widgets::cover_with(
            app,
            ui,
            cover,
            card.thumbnail.as_ref(),
            egui::CornerRadius::same(2),
        );
        let left = cover.right() + 16.0;
        let room = (rect.right() - 24.0 - left).max(0.0);
        // Two lines of 16.8, 4 apart, centred.
        let top = rect.center().y - 18.8;
        let title = theme::fit(ui, &card.title, theme::medium(14.0), PALETTE.text, room, 1);
        let under = theme::fit(
            ui,
            &card.subtitle,
            theme::regular(14.0),
            PALETTE.secondary,
            room,
            1,
        );
        ui.painter().galley(
            pos2(left, top + (16.8 - title.size().y) / 2.0),
            title,
            PALETTE.text,
        );
        ui.painter().galley(
            pos2(left, top + 20.8 + (16.8 - under.size().y) / 2.0),
            under,
            PALETTE.secondary,
        );
    }
    response.clicked()
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
