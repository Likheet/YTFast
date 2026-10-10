//! The menu on the left, as YouTube Music's: Home, Explore and Library,
//! then New playlist and the playlists in the library. The button in the
//! top bar closes it to a strip of icons.

use egui::{Align2, CornerRadius, Frame, Rect, Sense, Vec2, pos2, vec2};
use ytfast_core::read::{Item, Target, Thumb};

use crate::app::{Action, App, Dialog, Loadable};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE};
use crate::views::widgets;

/// How wide the menu is now.
pub fn width(app: &App) -> f32 {
    if app.settings.mini_guide {
        theme::GUIDE_MINI_WIDTH
    } else {
        theme::guide_width()
    }
}

const PAGES: [(Icon, &str, Route); 3] = [
    (Icon::Home, "Home", Route::Home),
    (Icon::Explore, "Explore", Route::Explore),
    (Icon::Library, "Library", Route::LibraryRecent),
];

pub fn show(app: &App, ui: &mut egui::Ui) {
    let mini = app.settings.mini_guide;
    // The open menu is solid `#030303` (an album's background stays behind
    // it); the strip is see-through until the page scrolls.
    // Premium: the player page's colours show through it.
    let solid = app.page_scrolled.get() || (app.now_playing && !theme::premium());
    let shown = ui
        .ctx()
        .animate_bool_with_time(egui::Id::new("guide-solid"), solid, 0.2);
    let fill = if mini {
        PALETTE.window.gamma_multiply(shown)
    } else if theme::premium() {
        // Premium: see-through, one background behind the menu and page.
        egui::Color32::TRANSPARENT
    } else {
        PALETTE.window
    };
    egui::Panel::left(if mini { "guide-mini" } else { "guide" })
        .exact_size(width(app))
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(fill))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(8.0);
            if mini {
                // Once the page scrolls, a white@0.10 line at the strip's
                // right edge (`#mini-guide-background`).
                if shown > 0.0 {
                    ui.painter().vline(
                        rect.right() - 0.5,
                        rect.y_range(),
                        egui::Stroke::new(1.0, PALETTE.outline.gamma_multiply(shown)),
                    );
                }
                for (icon, text, route) in PAGES {
                    mini_item(app, ui, icon, text, route);
                }
                return;
            }
            // The hairline between the menu and the page.
            ui.painter().vline(
                rect.right() - 0.5,
                rect.y_range(),
                egui::Stroke::new(1.0, PALETTE.divider),
            );
            for (icon, text, route) in PAGES {
                nav_item(app, ui, icon, text, route);
            }
            ui.add_space(24.0);
            ui.painter().hline(
                (rect.left() + 24.0)..=(rect.right() - 24.0),
                ui.cursor().top() + 0.5,
                egui::Stroke::new(1.0, PALETTE.divider),
            );
            // The divider is a border, a point high, then 24.
            if theme::premium() {
                // Premium: "YOUR LIBRARY" over New playlist.
                ui.add_space(20.0);
                let (heading, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                ui.painter().text(
                    pos2(heading.left() + 24.0, heading.center().y),
                    Align2::LEFT_CENTER,
                    "YOUR LIBRARY",
                    theme::medium(11.0),
                    PALETTE.dim,
                );
            } else {
                ui.add_space(25.0);
            }
            new_playlist(app, ui);
            ui.add_space(16.0);

            egui::ScrollArea::vertical()
                .id_salt("guide-playlists")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    playlist_item(app, ui, "Liked Music", "Auto playlist", None, Route::Liked);
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
                                playlist_item(
                                    app,
                                    ui,
                                    &card.title,
                                    &card.subtitle,
                                    card.thumbnail.as_ref(),
                                    route,
                                );
                            }
                        }
                    }
                    ui.add_space(8.0);
                });
        });
}

/// An entry's fill: white@0.10 for the page showing, white@0.20 under the
/// pointer (`--ytmusic-guide-hover`).
fn entry_fill(ui: &egui::Ui, rect: Rect, lit: bool, hovered: bool) {
    let fill = if lit {
        PALETTE.surface
    } else if hovered {
        PALETTE.surface_hover
    } else {
        return;
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
}

/// Whether an entry takes the fill under the pointer: also when the
/// keyboard is on it, in the Premium theme.
fn lit_by(response: &egui::Response) -> bool {
    response.hovered() || (theme::premium() && response.has_focus())
}

/// Whether `route` is the page showing (not hidden by the player page).
fn showing(app: &App, route: &Route) -> bool {
    // Library stays lit on all its tabs, as on YouTube Music; the page's
    // entry stays lit under the player page, which covers only the page.
    let library = |r: &Route| r.library_tab().is_some();
    app.route == *route || (*route == Route::LibraryRecent && library(&app.route))
}

/// Home, Explore, Library: an icon and a name, 48 high.
fn nav_item(app: &App, ui: &mut egui::Ui, icon: Icon, text: &str, route: Route) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    let premium = theme::premium();
    let rect = slot.shrink2(if premium {
        vec2(12.0, 2.0)
    } else {
        vec2(8.0, 0.0)
    });
    let response = ui.interact(rect, ui.id().with(("nav", text)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    let lit = showing(app, &route);
    entry_fill(ui, rect, lit, lit_by(&response));
    let icon_rect = Rect::from_min_size(
        pos2(rect.left() + 16.0, rect.center().y - 12.0),
        Vec2::splat(24.0),
    );
    theme::paint_icon(ui, icon, icon_rect, 24.0, PALETTE.text);
    // 16/400, and 500 for the page showing (Premium: 15, the others
    // quieter).
    let size = if premium { 15.0 } else { 16.0 };
    let font = if lit {
        theme::medium(size)
    } else {
        theme::regular(size)
    };
    let color = if premium && !lit {
        PALETTE.secondary
    } else {
        PALETTE.text
    };
    ui.painter().text(
        pos2(rect.left() + 60.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

/// The same pages with the menu closed: entries 56×65 one under another,
/// the icon 24 from 12 in, 5 under it the name 10/500.
fn mini_item(app: &App, ui: &mut egui::Ui, icon: Icon, text: &str, route: Route) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 65.0), Sense::hover());
    let rect = slot.shrink2(vec2(8.0, 0.0));
    let response = ui.interact(rect, ui.id().with(("mini", text)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    entry_fill(ui, rect, showing(app, &route), lit_by(&response));
    let icon_rect = Rect::from_min_size(
        pos2(rect.center().x - 12.0, rect.top() + 12.0),
        Vec2::splat(24.0),
    );
    theme::paint_icon(ui, icon, icon_rect, 24.0, PALETTE.text);
    ui.painter().text(
        pos2(rect.center().x, rect.top() + 47.0),
        Align2::CENTER_CENTER,
        text,
        theme::medium(10.0),
        PALETTE.text,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

/// "+ New playlist", a wide pill: 200 wide, 20 in (measured signed in).
fn new_playlist(app: &App, ui: &mut egui::Ui) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let rect = slot.shrink2(vec2(20.0, 0.0));
    let response = ui.interact(rect, ui.id().with("new-playlist"), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "New playlist"));
    let fill = if response.hovered() {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    let corners = if theme::premium() { 10 } else { 18 };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(corners), fill);
    // YouTube's tonal button: its icon 24 with 6 after it, words 14/500
    // `#f1f1f1`, together centred.
    let words = ui.painter().layout_no_wrap(
        "New playlist".to_string(),
        theme::medium(14.0),
        PALETTE.button,
    );
    let left = rect.center().x - (words.size().x + 24.0) / 2.0;
    let plus = Rect::from_min_size(pos2(left - 6.0, rect.center().y - 12.0), Vec2::splat(24.0));
    theme::paint_icon(ui, Icon::Plus, plus, 24.0, PALETTE.button);
    ui.painter().galley(
        pos2(left + 24.0, rect.center().y - words.size().y / 2.0),
        words,
        PALETTE.button,
    );
    if response.clicked() {
        app.act(Action::OpenDialog(Dialog::NewPlaylist {
            name: String::new(),
            description: String::new(),
            privacy: Default::default(),
            songs: Vec::new(),
        }));
    }
}

/// A playlist: its name over a quieter line (who made it), 56 high
/// (measured signed in: padding 4 16, the two lines centred). Premium: its
/// cover and its name, 52 high, who made it under the pointer.
fn playlist_item(
    app: &App,
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    thumbnail: Option<&Thumb>,
    route: Route,
) {
    let premium = theme::premium();
    let height = if premium { 52.0 } else { 56.0 };
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if !ui.is_rect_visible(slot) {
        return;
    }
    let rect = slot.shrink2(vec2(8.0, 0.0));
    let response = ui.interact(rect, ui.id().with(("playlist", &route)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let hovered = ui.rect_contains_pointer(rect);
    entry_fill(
        ui,
        rect,
        showing(app, &route),
        hovered || (premium && response.has_focus()),
    );
    // Under the pointer, a white disc 24 with a play icon 16 at the right
    // (16 in), and the words stop 50 from the right.
    let playlist = match &route {
        Route::Liked => Some("LM".to_string()),
        Route::Browse { id, .. } => id.strip_prefix("VL").map(str::to_string),
        _ => None,
    };
    let mut pressed = false;
    if hovered && let Some(playlist) = &playlist {
        let disc = Rect::from_center_size(
            pos2(rect.right() - 16.0 - 12.0, rect.center().y),
            Vec2::splat(24.0),
        );
        let play = ui.interact(
            disc,
            ui.id().with(("play-playlist", &route)),
            Sense::click(),
        );
        play.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Play {title}"))
        });
        ui.painter()
            .circle_filled(disc.center(), 12.0, PALETTE.text);
        theme::paint_icon(ui, Icon::Play, disc, 16.0, PALETTE.window);
        if play.clicked() {
            pressed = true;
            app.act(Action::QueuePlaylist(
                playlist.clone(),
                crate::app::QueueMode::Play,
            ));
        }
    }
    if premium {
        let art = Rect::from_min_size(
            pos2(rect.left() + 12.0, rect.center().y - 17.0),
            Vec2::splat(34.0),
        );
        if route == Route::Liked {
            // Liked Music has no cover: a glass tile with its thumb.
            ui.painter().rect_filled(
                art,
                CornerRadius::same(7),
                egui::Color32::from_white_alpha(20),
            );
            theme::paint_icon(ui, Icon::ThumbsUpFilled, art, 18.0, PALETTE.accent);
        } else {
            widgets::cover_with(app, ui, art, thumbnail, CornerRadius::same(7));
        }
        let left = art.right() + 12.0;
        let width = (rect.right() - left - if hovered { 50.0 } else { 12.0 }).max(0.0);
        theme::paint_line(
            ui,
            pos2(left, rect.center().y - 8.4),
            title,
            theme::medium(14.0),
            PALETTE.text,
            width,
        );
        if response.clicked() && !pressed {
            app.act(Action::Navigate(route));
        }
        response.on_hover_text(subtitle);
        return;
    }
    let left = rect.left() + 16.0;
    let width = rect.width() - 16.0 - if hovered { 50.0 } else { 16.0 };
    // Its name 14/500 on an 18 line at 10.5, 3 under it the line under,
    // 12/400 on a 16 line (measured: 10.5 and 30.5 from the top).
    theme::paint_line(
        ui,
        pos2(left, rect.top() + 11.1),
        title,
        theme::medium(14.0),
        PALETTE.text,
        width,
    );
    let mut below = pos2(left, rect.top() + 31.3);
    let mut below_width = width;
    if route == Route::Liked {
        let pin = Rect::from_min_size(below + vec2(0.0, 1.2), Vec2::splat(12.0));
        theme::paint_icon(ui, Icon::Pin, pin, 12.0, PALETTE.secondary);
        below.x += 16.0;
        below_width -= 16.0;
    }
    theme::paint_line(
        ui,
        below,
        subtitle,
        theme::regular(12.0),
        PALETTE.secondary,
        below_width,
    );
    if response.clicked() && !pressed {
        app.act(Action::Navigate(route));
    }
}
