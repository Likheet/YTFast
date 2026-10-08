//! The menu on the left, as YouTube Music's: Home, Explore and Library,
//! then New playlist and the playlists in the library. The button in the
//! top bar closes it to a strip of icons.

use egui::{Align2, CornerRadius, Frame, Rect, Sense, Vec2, pos2, vec2};
use ytfast_core::read::{Item, Target};

use crate::app::{Action, App, Dialog, Loadable};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE};

/// How wide the menu is now.
pub fn width(app: &App) -> f32 {
    if app.settings.mini_guide {
        theme::GUIDE_MINI_WIDTH
    } else {
        theme::GUIDE_WIDTH
    }
}

const PAGES: [(Icon, &str, Route); 3] = [
    (Icon::Home, "Home", Route::Home),
    (Icon::Explore, "Explore", Route::Explore),
    (Icon::Library, "Library", Route::Library),
];

pub fn show(app: &App, ui: &mut egui::Ui) {
    let mini = app.settings.mini_guide;
    egui::Panel::left(if mini { "guide-mini" } else { "guide" })
        .exact_size(width(app))
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |ui| {
            let rect = ui.max_rect();
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(8.0);
            if mini {
                for (icon, text, route) in PAGES {
                    mini_item(app, ui, icon, text, route);
                }
                return;
            }
            // The hairline between the menu and the page.
            ui.painter().vline(
                rect.right() - 0.5,
                rect.y_range(),
                egui::Stroke::new(1.0, PALETTE.outline),
            );
            for (icon, text, route) in PAGES {
                nav_item(app, ui, icon, text, route);
            }
            ui.add_space(24.0);
            ui.painter().hline(
                (rect.left() + 24.0)..=(rect.right() - 24.0),
                ui.cursor().top(),
                egui::Stroke::new(1.0, PALETTE.outline),
            );
            ui.add_space(24.0);
            new_playlist(app, ui);
            ui.add_space(16.0);

            egui::ScrollArea::vertical()
                .id_salt("guide-playlists")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    playlist_item(app, ui, "Liked Music", "Auto playlist", true, Route::Liked);
                    if let Some(Loadable::Ready(page)) = app.pages.get(&Route::Library) {
                        for section in &page.sections {
                            for item in &section.items {
                                let Item::Card(card) = item else { continue };
                                let Some(Target::Browse { id, params, .. }) = &card.open else {
                                    continue;
                                };
                                // Liked Music is already listed first.
                                if id == "VLLM" {
                                    continue;
                                }
                                let route = Route::browse(id.clone(), params.clone());
                                playlist_item(app, ui, &card.title, &card.subtitle, false, route);
                            }
                        }
                    }
                    ui.add_space(16.0);
                });
        });
}

/// Whether `route` is the page showing (not hidden by the player page).
fn showing(app: &App, route: &Route) -> bool {
    // Library stays lit on all its tabs, as on YouTube Music.
    let library = |r: &Route| {
        matches!(
            r,
            Route::Library | Route::LibrarySongs | Route::LibraryAlbums | Route::LibraryArtists
        )
    };
    !app.now_playing && (app.route == *route || (*route == Route::Library && library(&app.route)))
}

/// Home, Explore, Library: an icon and a name, 48 high.
fn nav_item(app: &App, ui: &mut egui::Ui, icon: Icon, text: &str, route: Route) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    let rect = slot.shrink2(vec2(8.0, 0.0));
    let response = ui.interact(rect, ui.id().with(("nav", text)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    if showing(app, &route) || response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
    }
    let icon_rect = Rect::from_min_size(
        pos2(rect.left() + 16.0, rect.center().y - 12.0),
        Vec2::splat(24.0),
    );
    theme::paint_icon(ui, icon, icon_rect, 24.0, PALETTE.text);
    ui.painter().text(
        pos2(rect.left() + 60.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::medium(16.0),
        PALETTE.text,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

/// The same pages with the menu closed: an icon over a small name.
fn mini_item(app: &App, ui: &mut egui::Ui, icon: Icon, text: &str, route: Route) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    let rect = Rect::from_center_size(slot.center(), vec2(56.0, 56.0));
    let response = ui.interact(rect, ui.id().with(("mini", text)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    if showing(app, &route) || response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
    }
    let icon_rect =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 21.0), Vec2::splat(24.0));
    theme::paint_icon(ui, icon, icon_rect, 24.0, PALETTE.text);
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 13.0),
        Align2::CENTER_CENTER,
        text,
        theme::regular(10.0),
        PALETTE.text,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

/// "+ New playlist", a wide pill.
fn new_playlist(app: &App, ui: &mut egui::Ui) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let rect = slot.shrink2(vec2(24.0, 0.0));
    let response = ui.interact(rect, ui.id().with("new-playlist"), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "New playlist"));
    let fill = if response.hovered() {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(18), fill);
    let words = ui.painter().layout_no_wrap(
        "New playlist".to_string(),
        theme::medium(14.0),
        PALETTE.text,
    );
    let left = rect.center().x - (words.size().x + 26.0) / 2.0;
    let plus = Rect::from_min_size(pos2(left, rect.center().y - 10.0), Vec2::splat(20.0));
    theme::paint_icon(ui, Icon::Plus, plus, 20.0, PALETTE.text);
    ui.painter().galley(
        pos2(left + 26.0, rect.center().y - words.size().y / 2.0),
        words,
        PALETTE.text,
    );
    if response.clicked() {
        app.act(Action::OpenDialog(Dialog::NewPlaylist {
            name: String::new(),
            song: None,
        }));
    }
}

/// A playlist: its name over a quieter line (who made it).
fn playlist_item(
    app: &App,
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    pinned: bool,
    route: Route,
) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    if !ui.is_rect_visible(slot) {
        return;
    }
    let rect = slot.shrink2(vec2(8.0, 0.0));
    let response = ui.interact(rect, ui.id().with(("playlist", &route)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    if showing(app, &route) || response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
    }
    let left = rect.left() + 16.0;
    let width = rect.width() - 32.0;
    theme::paint_line(
        ui,
        pos2(left, rect.top() + 7.0),
        title,
        theme::medium(14.0),
        PALETTE.text,
        width,
    );
    let mut below = pos2(left, rect.top() + 27.0);
    let mut below_width = width;
    if pinned {
        let pin = Rect::from_min_size(below + vec2(-1.0, 1.0), Vec2::splat(13.0));
        theme::paint_icon(ui, Icon::Pin, pin, 13.0, PALETTE.secondary);
        below.x += 17.0;
        below_width -= 17.0;
    }
    theme::paint_line(
        ui,
        below,
        subtitle,
        theme::regular(12.0),
        PALETTE.secondary,
        below_width,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}
