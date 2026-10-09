//! The bar that shows while songs are ticked on a page, as YouTube Music's
//! (`ytmusic-multi-select-menu-bar`, measured signed in): centred at the
//! window's foot (above the player bar), `#212121`, r 2, padding 15 20,
//! 396 wide by 70. × (clears the ticks, `#aaa`), "N selected" (16 white),
//! then Save to playlist, Play next and ⋮ (Add to queue, and Remove from
//! playlist on the account's own playlist).

use egui::{Align2, CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{Action, App, Dialog};
use crate::theme::{self, Icon, PALETTE};

pub fn show(app: &App, ui: &egui::Ui) {
    if app.selected.is_empty() {
        return;
    }
    let window = ui.ctx().content_rect();
    let foot = if app.playback.entry.is_some() {
        theme::player_bar_height() + 16.0
    } else {
        33.0
    };
    let size = vec2(396.0, 70.0);
    let rect = Rect::from_min_size(
        pos2(
            window.center().x - size.x / 2.0,
            window.bottom() - foot - size.y,
        ),
        size,
    );
    egui::Area::new(egui::Id::new("selection-bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ui.ctx(), |ui| {
            let (bar, _) = ui.allocate_exact_size(size, Sense::click());
            if crate::theme::dynamic() {
                // Dynamic Background: glass, corners 24, the theme's shadow.
                let corners = CornerRadius::same(crate::dynamic::RADIUS_PANEL_LG);
                ui.painter()
                    .add(crate::dynamic::SHADOW.as_shape(bar, corners));
                crate::dynamic::glass(ui.painter(), bar, corners);
            } else {
                ui.painter()
                    .rect_filled(bar, CornerRadius::same(2), PALETTE.panel);
            }
            let y = bar.center().y;
            let mut x = bar.left() + 20.0;
            let spot = |x: f32| Rect::from_min_size(pos2(x, y - 20.0), Vec2::splat(40.0));
            if button(ui, spot(x), Icon::Close, "Clear the ticks") {
                app.act(Action::ClearSelected);
            }
            x += 40.0 + 10.0;
            let count = app.selected.len();
            ui.painter().text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                format!("{count} selected"),
                theme::regular(16.0),
                PALETTE.text,
            );
            x += 89.0 + 70.0 - 40.0;
            let video_ids: Vec<String> = app.selected.iter().map(|t| t.video_id.clone()).collect();
            if button(ui, spot(x), Icon::SaveToPlaylist, "Save to playlist") {
                app.act(Action::OpenDialog(Dialog::SaveToPlaylist { video_ids }));
                app.act(Action::ClearSelected);
            }
            x += 40.0;
            if button(ui, spot(x), Icon::PlayNext, "Play next") {
                app.act(Action::PlayNextAll(app.selected.clone()));
            }
            x += 40.0;
            let more = ui.interact(spot(x), ui.id().with("more"), Sense::click());
            more.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions")
            });
            paint_button(ui, &more, Icon::MoreVertical);
            theme::menu_popup(&more).show(|ui| {
                theme::menu(ui);
                if theme::menu_item(ui, Icon::AddToQueue, "Add to queue").clicked() {
                    app.act(Action::AddToQueueAll(app.selected.clone()));
                    ui.close();
                }
                // Only on the account's own playlist's page.
                if let Some(playlist_id) = app.editable_playlist() {
                    let songs: Vec<(String, String)> = app
                        .selected
                        .iter()
                        .filter_map(|t| Some((t.video_id.clone(), t.set_video_id.clone()?)))
                        .collect();
                    if !songs.is_empty()
                        && theme::menu_item(ui, Icon::RemoveFromPlaylist, "Remove from playlist")
                            .clicked()
                    {
                        app.act(Action::RemoveFromPlaylist { playlist_id, songs });
                        app.act(Action::ClearSelected);
                        ui.close();
                    }
                }
            });
        });
}

/// One of the bar's buttons: a 40 spot, its icon 24 `#aaa`, white@0.20
/// under the pointer. True when clicked.
fn button(ui: &egui::Ui, spot: Rect, icon: Icon, name: &str) -> bool {
    let response = ui.interact(spot, ui.id().with(name), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    paint_button(ui, &response, icon);
    response.clicked()
}

fn paint_button(ui: &egui::Ui, response: &egui::Response, icon: Icon) {
    if response.hovered() {
        ui.painter()
            .circle_filled(response.rect.center(), 20.0, PALETTE.surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::paint_icon(ui, icon, response.rect, 24.0, PALETTE.dim);
}
