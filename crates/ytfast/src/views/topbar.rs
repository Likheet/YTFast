//! The bar across the top, as YouTube Music's (`ytmusic-nav-bar`, measured
//! at 1280 wide): the menu button and the app's mark, the search box over
//! the page's own column, and the account at the content's right edge.
//! Back and forward (a web browser has its own; this window has not) sit
//! just left of the account. Transparent at the top of a page, the bar
//! turns solid with a hairline under it once the page scrolls.

use std::time::Duration;

use egui::{Align2, CornerRadius, Frame, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use ytfast_core::read::{Item, SuggestedWords, Target, Track, TrackKind};

use crate::app::{Action, App, Auth};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE, Round};
use crate::views::{sidebar, widgets, window_frame};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(theme::top_bar_height())
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |ui| {
            let bar = ui.max_rect();
            // The bar is the window's title bar too (on Windows): what is
            // drawn on it below takes the pointer first.
            window_frame::drag_area(ui, bar);
            let guide = sidebar::width(app);
            // (An album's background, painted behind everything, shows
            // through the bar while the page is at its top.)
            let premium = theme::premium();
            if !app.settings.mini_guide && !premium {
                // The open menu is solid from the window's top, behind the
                // logo too (`#guide-wrapper`). (Premium's is see-through.)
                let corner = Rect::from_min_max(bar.min, pos2(bar.left() + guide, bar.bottom()));
                ui.painter().rect_filled(corner, 0.0, PALETTE.window);
            }
            // Once the page scrolls (or the player page is open), the bar
            // turns `#030303` with a white@0.15 line under it, fading over
            // 0.2 s (`#nav-bar-background`). Premium: the player page's
            // colours show through it.
            let solid = app.page_scrolled.get() || (app.now_playing && !premium);
            let shown = ui
                .ctx()
                .animate_bool_with_time(ui.id().with("solid"), solid, 0.2);
            if shown > 0.0 {
                // Beside the open menu (its corner is already solid).
                // Premium's stays see-through, its background showing: only
                // the hairline marks the scroll.
                let left = if app.settings.mini_guide {
                    bar.left()
                } else {
                    bar.left() + guide
                };
                let content = Rect::from_min_max(pos2(left, bar.top()), bar.max);
                if !premium {
                    ui.painter()
                        .rect_filled(content, 0.0, PALETTE.window.gamma_multiply(shown));
                }
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom() - 0.5,
                    egui::Stroke::new(1.0, PALETTE.divider.gamma_multiply(shown)),
                );
            }
            if !app.settings.mini_guide {
                // The hairline between the menu and the page carries on
                // through the bar.
                ui.painter().vline(
                    bar.left() + guide - 0.5,
                    bar.y_range(),
                    egui::Stroke::new(1.0, PALETTE.divider),
                );
            }

            // The menu button (40, from 16 in) and the mark (24, at 68).
            let button =
                Rect::from_center_size(pos2(bar.left() + 36.0, bar.center().y), Vec2::splat(40.0));
            let mut left = ui.new_child(UiBuilder::new().max_rect(button));
            if theme::round_button(
                &mut left,
                Icon::Menu,
                40.0,
                24.0,
                Round::Plain,
                PALETTE.text,
                "Menu",
            )
            .clicked()
            {
                app.act(Action::ToggleGuide);
            }
            let mark = Rect::from_min_size(
                pos2(bar.left() + 68.0, bar.center().y - 12.0),
                Vec2::splat(24.0),
            );
            theme::paint_logo(ui, mark);
            let name = ui.painter().text(
                pos2(mark.right() + 5.0, bar.center().y),
                Align2::LEFT_CENTER,
                "YTFast",
                theme::bold(19.0),
                PALETTE.text,
            );
            let home = ui.interact(mark.union(name), ui.id().with("home"), Sense::click());
            home.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "YTFast home")
            });
            if home.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if home.clicked() {
                app.act(Action::Navigate(Route::Home));
            }

            // A newer version: the update badge, as Spotifast's, until it
            // is installed.
            let badge = app
                .update_found
                .as_ref()
                .map(|_| badge_width(ui))
                .unwrap_or(0.0);
            let (avatar, arrows, field) = if theme::premium() {
                premium_places(bar, guide, name.right(), badge)
            } else {
                youtube_music_places(app, ui, bar, guide, name.right(), badge)
            };
            if let Some(version) = &app.update_found {
                // Before the arrows (on the right) or the account.
                let end = if arrows.left() > field.right() {
                    arrows.left() - 8.0
                } else {
                    avatar.left() - 12.0
                };
                let rect = Rect::from_min_max(
                    pos2(end - badge, bar.center().y - 16.0),
                    pos2(end, bar.center().y + 16.0),
                );
                update_badge(app, ui, rect, version);
            }
            account(app, ui, avatar);
            let mut nav = ui.new_child(
                UiBuilder::new()
                    .max_rect(arrows)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            nav.spacing_mut().item_spacing.x = 0.0;
            for (icon, name, enabled, action) in [
                (Icon::NavBack, "Back", app.can_go_back(), Action::Back),
                (
                    Icon::NavForward,
                    "Forward",
                    app.can_go_forward(),
                    Action::Forward,
                ),
            ] {
                nav.add_enabled_ui(enabled, |ui| {
                    if theme::round_button(ui, icon, 40.0, 24.0, Round::Plain, PALETTE.text, name)
                        .clicked()
                    {
                        app.act(action);
                    }
                });
            }
            search_box(app, ui, field);
            window_frame::buttons(ui, bar);
        });
}

/// Where YouTube Music puts the account, back and forward, and the search
/// box (`avatar`, `arrows`, `field`): the search box over the page's own
/// column (on search, its narrower one), the account at the content's
/// right edge (16 before the window's buttons when they are there), back
/// and forward just left of it. `name_right` is where the app's name ends.
fn youtube_music_places(
    app: &App,
    ui: &egui::Ui,
    bar: Rect,
    guide: f32,
    name_right: f32,
    badge: f32,
) -> (Rect, Rect, Rect) {
    let window = ui.ctx().content_rect().width();
    let area = bar.width() - guide;
    let grid = theme::Grid::new(window, area);
    let column = if matches!(app.route, Route::Search(_) | Route::SearchOnly(..)) {
        theme::Grid::search(window, area)
    } else {
        grid
    };
    let search_left = (bar.left() + guide + column.left).max(name_right + 12.0);
    let account_right = (bar.left() + guide + grid.left + grid.width)
        .min(bar.right() - window_frame::buttons_width() - 16.0);
    // The account: 26 round, its right edge at the content's.
    let avatar = Rect::from_center_size(
        pos2(account_right - 13.0, bar.center().y),
        Vec2::splat(26.0),
    );
    // Back and forward, 8 before the account (where YouTube Music's cast
    // button stands).
    let arrows = Rect::from_min_max(
        pos2(avatar.left() - 8.0 - 80.0, bar.center().y - 20.0),
        pos2(avatar.left() - 8.0, bar.center().y + 20.0),
    );
    // The search box: at most 480, 42 high, in the bar's middle (from y 11).
    let room = if badge > 0.0 { badge + 8.0 } else { 0.0 };
    let width = (arrows.left() - room - 16.0 - search_left).clamp(120.0, 480.0);
    let field = Rect::from_min_size(pos2(search_left, bar.center().y - 21.0), vec2(width, 42.0));
    (avatar, arrows, field)
}

/// Premium's places, balanced across the bar: back and forward just past
/// the menu, the search box (at most 560) in the middle of the page, kept
/// 24 clear of both, and the account 16 before the window's buttons; all on
/// the bar's middle line, as the window's buttons are.
fn premium_places(bar: Rect, guide: f32, name_right: f32, badge: f32) -> (Rect, Rect, Rect) {
    let y = bar.center().y;
    let page_left = bar.left() + guide;
    let arrows_left = (page_left + 16.0).max(name_right + 16.0);
    let arrows = Rect::from_min_max(
        pos2(arrows_left, y - 20.0),
        pos2(arrows_left + 80.0, y + 20.0),
    );
    let account_right = bar.right() - window_frame::buttons_width() - 16.0;
    let avatar = Rect::from_center_size(pos2(account_right - 13.0, y), Vec2::splat(26.0));
    let room = if badge > 0.0 { badge + 12.0 } else { 0.0 };
    let (room_left, room_right) = (arrows.right() + 24.0, avatar.left() - room - 24.0);
    let width = (room_right - room_left).clamp(120.0, 560.0);
    let middle = (page_left + bar.right()) / 2.0;
    let left = (middle - width / 2.0).clamp(room_left, (room_right - width).max(room_left));
    let field = Rect::from_min_size(pos2(left, y - 21.0), vec2(width, 42.0));
    (avatar, arrows, field)
}

/// The update badge's width: the icon 18 and the word, 12 in at each end.
fn badge_width(ui: &egui::Ui) -> f32 {
    let words = ui
        .painter()
        .layout_no_wrap("Update".into(), theme::medium(13.0), PALETTE.text);
    12.0 + 18.0 + 6.0 + words.size().x + 14.0
}

/// The update badge, as Spotifast's: a pill 32 high with a "new" mark (in
/// the switches' colour) and "Update", that stays while a newer version is
/// known and opens the update window. Its words for the pointer and
/// screen readers say which version.
fn update_badge(app: &App, ui: &egui::Ui, rect: Rect, version: &str) {
    let response = ui.interact(rect, ui.id().with("update-badge"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Update available")
    });
    theme::pointing(ui, &response);
    let fill = if response.hovered() {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
    let mark = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 9.0),
        Vec2::splat(18.0),
    );
    theme::paint_icon(ui, Icon::NewReleases, mark, 18.0, PALETTE.switch);
    ui.painter().text(
        pos2(mark.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Update",
        theme::medium(13.0),
        PALETTE.text,
    );
    if response
        .on_hover_text(format!("YTFast {version} is available."))
        .clicked()
    {
        app.act(Action::ShowUpdate);
    }
}

/// The account's round button (its photo 26 on `#909090`; the name's first
/// letter until the photo arrives), and its menu as YouTube Music's
/// (measured signed in): 300 wide, `#282828`, r 12; a header with the
/// photo 40 and the name and handle (16/400 on 22 lines), a white@0.20
/// line, then entries 40 high (an icon 18 at 16, the words 14/400 at 50):
/// History, Settings and Sign out, what YTFast does itself.
fn account(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let Auth::SignedIn {
        name,
        handle,
        photo,
    } = &app.auth
    else {
        return;
    };
    let response = ui.interact(rect, ui.id().with("account"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Account"));
    face(app, ui, rect, name, photo.as_deref());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let menu = egui::Frame::new()
        .fill(egui::Color32::from_rgb(0x28, 0x28, 0x28))
        .corner_radius(CornerRadius::same(12))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 32,
            spread: 0,
            color: egui::Color32::from_black_alpha(26),
        });
    // Dynamic Background: glass, corners 24.
    let menu = if theme::dynamic() {
        crate::dynamic::dialog_frame()
    } else {
        menu
    };
    egui::Popup::menu(&response)
        .frame(menu)
        .width(300.0)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(8.0)
        .show(|ui| {
            if theme::dynamic() {
                crate::dynamic::glass_behind(ui, crate::dynamic::RADIUS_PANEL_LG);
            }
            ui.set_width(300.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // The header: 16 around, the photo 40 and 16 after it, the name
            // and handle (16, on 22 lines) beside it.
            let lines = if handle.is_some() { 2.0 } else { 1.0 };
            let height = (16.0 + 22.0 * lines + 16.0_f32).max(72.0);
            let (top, _) = ui.allocate_exact_size(vec2(300.0, height), Sense::hover());
            let picture = Rect::from_min_size(
                pos2(top.left() + 16.0, top.center().y - 20.0),
                Vec2::splat(40.0),
            );
            face(app, ui, picture, name, photo.as_deref());
            let left = picture.right() + 16.0;
            let room = top.right() - 16.0 - left;
            let mut y = top.center().y - 11.0 * lines;
            for words in std::iter::once(name.as_str()).chain(handle.as_deref()) {
                theme::paint_line(
                    ui,
                    pos2(left, y + 1.6),
                    words,
                    theme::regular(16.0),
                    PALETTE.text,
                    room,
                );
                y += 22.0;
            }
            let (rule, _) = ui.allocate_exact_size(vec2(300.0, 1.0), Sense::hover());
            ui.painter().hline(
                rule.x_range(),
                rule.center().y,
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(51)),
            );
            let entry = |ui: &mut egui::Ui, icon: Icon, words: &str| -> egui::Response {
                let (row, response) = ui.allocate_exact_size(vec2(300.0, 40.0), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, words)
                });
                if response.hovered() {
                    ui.painter()
                        .rect_filled(row, 0.0, egui::Color32::from_white_alpha(26));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let mark = Rect::from_min_size(
                    pos2(row.left() + 16.0, row.center().y - 9.0),
                    Vec2::splat(18.0),
                );
                theme::paint_icon(ui, icon, mark, 18.0, PALETTE.text);
                theme::paint_line(
                    ui,
                    pos2(row.left() + 50.0, row.center().y - 8.4),
                    words,
                    theme::regular(14.0),
                    PALETTE.text,
                    300.0 - 50.0 - 36.0,
                );
                response
            };
            // Only what YTFast does itself: the account's web pages (its
            // channel, memberships, Google's help and policies) are left
            // out, as this window opens no web pages.
            ui.add_space(8.0);
            for (icon, words, route) in [
                (Icon::History, "History", Route::History),
                (Icon::Settings, "Settings", Route::Settings),
            ] {
                if entry(ui, icon, words).clicked() {
                    app.act(Action::Navigate(route));
                    ui.close();
                }
            }
            if !app.demo && entry(ui, Icon::LogOut, "Sign out").clicked() {
                app.act(Action::SignOut);
                ui.close();
            }
            ui.add_space(8.0);
        });
}

/// The account's photo in `rect`, round on `#909090`; the name's first
/// letter while it has none.
fn face(app: &App, ui: &egui::Ui, rect: Rect, name: &str, photo: Option<&str>) {
    ui.painter()
        .circle_filled(rect.center(), rect.width() / 2.0, PALETTE.quiet);
    let texture = photo.and_then(|url| app.picture(url));
    if let Some(texture) = texture {
        egui::Image::new((texture.id(), rect.size()))
            .corner_radius(CornerRadius::same((rect.width() / 2.0) as u8))
            .paint_at(ui, rect);
        return;
    }
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        theme::medium(rect.width() / 2.0),
        PALETTE.text,
    );
}

/// The search box, as YouTube Music's (`ytmusic-search-box`): white@0.15
/// with a 1 point border of the same, r 8; `#030303` while it has the
/// keyboard. Its icon 18 centred 27 in; the words (16) from 53 in to 56
/// before its right end, where an × clears them.
fn search_box(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let id = ui.id().with("search");
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let focused = ui.memory(|m| m.has_focus(super::SEARCH_BOX.into()));
    // Whether the suggestions showed last frame: then the box's lower
    // corners are square, and it carries a shadow.
    let open = ui
        .data(|d| d.get_temp::<Rect>(ui.id().with("suggest").with("rect")))
        .is_some();
    // Premium: glass, as its other buttons (white@0.07, more under the
    // pointer and while typing; solid while the suggestions hang under
    // it), corners 12, outlined in the accent while typing.
    let premium = theme::premium();
    let fill = if premium {
        if open {
            PALETTE.menu
        } else if focused || ui.rect_contains_pointer(rect) {
            egui::Color32::from_white_alpha(26)
        } else {
            egui::Color32::from_white_alpha(18)
        }
    } else if focused || open {
        PALETTE.window
    } else {
        PALETTE.field
    };
    let round = if premium { 12 } else { 8 };
    let corners = if open {
        CornerRadius {
            nw: round,
            ne: round,
            sw: 0,
            se: 0,
        }
    } else {
        CornerRadius::same(round)
    };
    let edge = match (premium, focused) {
        (true, true) => PALETTE.accent,
        (true, false) => PALETTE.outline,
        (false, _) => PALETTE.divider,
    };
    // Dynamic Background: white@0.10 and corners 12 always; its
    // suggestions float apart, below.
    let (fill, corners) = if theme::dynamic() {
        (PALETTE.field, CornerRadius::same(12))
    } else {
        (fill, corners)
    };
    ui.painter().rect(
        rect,
        corners,
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let tint = if focused { PALETTE.text } else { PALETTE.hint };
    let icon = Rect::from_center_size(pos2(rect.left() + 27.0, rect.center().y), Vec2::splat(18.0));
    theme::paint_icon(ui, Icon::Search, icon, 18.0, tint);

    // The words (and the placeholder, the same 16) on one line centred in
    // the box, as the page's input centres its line.
    let font = theme::regular(16.0);
    let line = ui.fonts_mut(|f| f.row_height(&font));
    let field_rect = Rect::from_min_max(
        pos2(rect.left() + 53.0, rect.center().y - line / 2.0),
        pos2(rect.right() - 56.0, rect.center().y + line / 2.0),
    );
    let hint = egui::RichText::new("Search songs, albums, artists, podcasts")
        .font(font.clone())
        .color(PALETTE.hint);
    let edit = egui::TextEdit::singleline(&mut text)
        .id(super::SEARCH_BOX.into())
        .hint_text(hint)
        .font(font)
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .text_color(PALETTE.text);
    let response = ui.put(field_rect, edit);
    // As YouTube Music's: a hand over the box until it is typed in, the
    // text cursor while it is.
    if ui.rect_contains_pointer(rect) {
        let icon = if focused {
            egui::CursorIcon::Text
        } else {
            egui::CursorIcon::PointingHand
        };
        ui.ctx().set_cursor_icon(icon);
    }

    // ×: 32 across, 11 in from the right, once something is typed.
    if !text.is_empty() {
        let clear = Rect::from_center_size(
            pos2(rect.right() - 11.0 - 16.0, rect.center().y),
            Vec2::splat(32.0),
        );
        let mut corner = ui.new_child(UiBuilder::new().max_rect(clear));
        if theme::round_button(
            &mut corner,
            Icon::Close,
            32.0,
            18.0,
            Round::Plain,
            tint,
            "Clear search",
        )
        .clicked()
        {
            text.clear();
            response.request_focus();
        }
    }

    match suggestions(app, ui, &response, rect, &mut text) {
        Some(Chosen::Words(words)) => app.act(Action::Search(words)),
        Some(Chosen::Done) => {}
        None if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) => {
            app.act(Action::Search(text.clone()));
        }
        None => {}
    }
    ui.data_mut(|d| d.insert_temp(id, text));
}

/// What was chosen from the suggestions.
enum Chosen {
    /// Words to search for.
    Words(String),
    /// A song now playing or a page opened: nothing more to do.
    Done,
}

/// How long typing pauses before suggestions are asked for, in seconds.
const TYPING_PAUSE: f64 = 0.25;

/// What YouTube Music suggests for the words typed so far (with nothing
/// typed, the account's past searches), as its list under the box
/// (`#suggestion-list`): right under it, as wide, `#030303` with a
/// white@0.15 border and its lower corners rounded 8, padding 8; rows of
/// words 48 high (the search icon 18 white@0.50 centred 25 in, the words
/// 14 at 55 in, white@0.50, the typed part in 500 weight; a past search
/// has a clock for its icon and a bin, 36 across and 4 from the right,
/// that removes it from the history),
/// then the songs, artists and albums it suggests, 56 high (picture 32 at
/// 12 in, round for an artist; 16 after it the title 14/500 and 3 under
/// it what it is, 14 white@0.70). The arrow keys move through them all
/// (marked white@0.10 with a blue edge); Enter or a click searches for
/// words, or plays the song or opens the page.
fn suggestions(
    app: &App,
    ui: &mut egui::Ui,
    field: &egui::Response,
    rect: Rect,
    text: &mut String,
) -> Option<Chosen> {
    let id = ui.id().with("suggest");
    let now = ui.input(|i| i.time);
    let typed = text.trim().to_string();
    // Asked for once typing pauses for a moment; with nothing typed, the
    // past searches, each time the box is entered (a search since then
    // is among them).
    let asked: Option<String> = ui.data(|d| d.get_temp(id));
    let entered = typed.is_empty() && field.gained_focus();
    if entered || asked.as_deref() != Some(typed.as_str()) {
        let (pending, since): (String, f64) = ui
            .data(|d| d.get_temp(id.with("pending")))
            .unwrap_or_default();
        let waited = now - since;
        if typed.is_empty() {
            // Asked for when the box is entered, or emptied in it.
            if field.has_focus() {
                app.act(Action::Suggest(String::new()));
            }
            ui.data_mut(|d| d.insert_temp(id, typed.clone()));
        } else if pending != typed {
            ui.data_mut(|d| d.insert_temp(id.with("pending"), (typed.clone(), now)));
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(TYPING_PAUSE + 0.01));
        } else if waited >= TYPING_PAUSE {
            app.act(Action::Suggest(typed.clone()));
            ui.data_mut(|d| d.insert_temp(id, typed.clone()));
        } else {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(TYPING_PAUSE - waited + 0.01));
        }
    }
    let (for_text, found) = &app.suggestions;
    let popup_rect: Option<Rect> = ui.data(|d| d.get_temp(id.with("rect")));
    let over_popup = popup_rect.is_some_and(|r| ui.rect_contains_pointer(r));
    // A song played or a page opened from the list closes it, until the
    // words change.
    let closed: Option<String> = ui.data(|d| d.get_temp(id.with("closed")));
    // (Enter takes the focus from the box in the same frame it is read.)
    let show = (field.has_focus() || field.lost_focus() || over_popup)
        && *for_text == typed
        && closed.as_deref() != Some(typed.as_str())
        && !(found.words.is_empty() && found.items.is_empty());
    if !show {
        ui.data_mut(|d| {
            d.remove::<Rect>(id.with("rect"));
            d.remove::<usize>(id.with("marked"));
        });
        return None;
    }
    // As many as YouTube gives (6 as a rule, 7 past searches signed in).
    let words: Vec<&SuggestedWords> = found.words.iter().take(10).collect();
    let items: Vec<&Item> = found.items.iter().take(5).collect();
    let total = words.len() + items.len();
    // The row the arrow keys have marked, if any.
    let mut marked: Option<usize> = ui.data(|d| d.get_temp(id.with("marked")));
    if field.has_focus() {
        let (down, up) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            )
        });
        if down {
            marked = Some(marked.map_or(0, |m| (m + 1).min(total - 1)));
        }
        if up {
            marked = marked.and_then(|m| m.checked_sub(1));
        }
    }
    let mut chosen = None;
    let mut picked: Option<&Item> = None;
    // A past search to remove from the history: its words and token.
    let mut forget: Option<(String, String)> = None;
    if let Some(m) = marked
        && field.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        match words.get(m) {
            Some(words) => chosen = Some(words.text.clone()),
            None => picked = items.get(m - words.len()).copied(),
        }
    }

    // A row's fill under the pointer, or marked by the keys (with a blue
    // edge).
    let dynamic = theme::dynamic();
    let fill = |ui: &egui::Ui, row: Rect, hovered: bool, is_marked: bool| {
        if dynamic {
            // Dynamic Background: white@0.10, corners 12, inset 8.
            if hovered || is_marked {
                ui.painter().rect_filled(
                    row.shrink2(vec2(8.0, 0.0)),
                    CornerRadius::same(crate::dynamic::RADIUS_PANEL),
                    PALETTE.surface,
                );
            }
            return;
        }
        if hovered || is_marked {
            ui.painter().rect_filled(row, 0.0, PALETTE.surface);
        }
        if is_marked {
            ui.painter().rect_filled(
                Rect::from_min_size(row.min, vec2(2.0, row.height())),
                0.0,
                PALETTE.switch,
            );
        }
    };
    // Dynamic Background: the list floats 16 below the box, as glass with
    // corners 12 (`glass_behind`; its frame's colours are clear).
    let rect = if dynamic {
        rect.translate(vec2(0.0, 17.0))
    } else {
        rect
    };
    let area = egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_bottom() - vec2(0.0, 1.0))
        .show(ui.ctx(), |ui| {
            // Premium: the menu's colour, its foot rounded 12.
            let (backing, round) = if theme::premium() {
                (PALETTE.menu, 12)
            } else {
                (PALETTE.window, 8)
            };
            Frame::new()
                .fill(backing)
                .stroke(egui::Stroke::new(1.0, PALETTE.divider))
                .corner_radius(CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: round,
                    se: round,
                })
                .inner_margin(Margin::symmetric(0, 8))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 10,
                    spread: 1,
                    color: egui::Color32::from_black_alpha(36),
                })
                .show(ui, |ui| {
                    if dynamic {
                        crate::dynamic::glass_behind(ui, crate::dynamic::RADIUS_PANEL);
                    }
                    ui.set_width(rect.width() - 2.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, suggestion) in words.iter().enumerate() {
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                &suggestion.text,
                            )
                        });
                        // A past search's bin: 36 across, 4 from the right,
                        // its icon 18. It takes the click, not the row.
                        let bin = suggestion.forget.as_ref().map(|token| {
                            let spot = Rect::from_min_size(
                                pos2(row.right() - 40.0, row.center().y - 18.0),
                                Vec2::splat(36.0),
                            );
                            let bin = ui.interact(spot, response.id.with("bin"), Sense::click());
                            bin.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Remove {}", suggestion.text),
                                )
                            });
                            (spot, bin, token)
                        });
                        let bin_hovered = bin.as_ref().is_some_and(|(_, b, _)| b.hovered());
                        fill(
                            ui,
                            row,
                            response.hovered() || bin_hovered,
                            marked == Some(index),
                        );
                        let mark = if bin.is_some() {
                            Icon::History
                        } else {
                            Icon::Search
                        };
                        let icon = Rect::from_center_size(
                            pos2(row.left() + 25.0, row.center().y),
                            Vec2::splat(18.0),
                        );
                        theme::paint_icon(ui, mark, icon, 18.0, PALETTE.hint);
                        let room = if bin.is_some() { 40.0 } else { 16.0 };
                        let words = suggestion_words(
                            ui,
                            &suggestion.text,
                            &typed,
                            row.width() - 55.0 - room,
                        );
                        let at = pos2(row.left() + 55.0, row.center().y - words.size().y / 2.0);
                        ui.painter().galley(at, words, PALETTE.hint);
                        if let Some((spot, bin, token)) = bin {
                            if bin.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            let icon = Rect::from_center_size(spot.center(), Vec2::splat(18.0));
                            theme::paint_icon(ui, Icon::Delete, icon, 18.0, PALETTE.hint);
                            if bin.clicked() {
                                forget = Some((suggestion.text.clone(), token.clone()));
                            }
                        }
                        if response.clicked() {
                            chosen = Some(suggestion.text.clone());
                        }
                    }
                    for (k, item) in items.iter().enumerate() {
                        let index = words.len() + k;
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
                        let (title, line, thumb, round) = match item {
                            Item::Track(track) => {
                                (&track.title, track_line(track), &track.thumbnail, false)
                            }
                            Item::Card(card) => (
                                &card.title,
                                card.subtitle.clone(),
                                &card.thumbnail,
                                card.round,
                            ),
                        };
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title)
                        });
                        fill(ui, row, response.hovered(), marked == Some(index));
                        let picture = Rect::from_min_size(
                            pos2(row.left() + 12.0, row.center().y - 16.0),
                            Vec2::splat(32.0),
                        );
                        if round {
                            widgets::cover(app, ui, picture, thumb.as_ref(), true);
                        } else {
                            widgets::cover_with(
                                app,
                                ui,
                                picture,
                                thumb.as_ref(),
                                CornerRadius::same(4),
                            );
                        }
                        let left = picture.right() + 16.0;
                        let width = (row.right() - 12.0 - left).max(0.0);
                        let size = theme::text_size(ui);
                        let line_height = size * 1.2;
                        let top = row.center().y - (line_height * 2.0 + 3.0) / 2.0;
                        theme::paint_line(
                            ui,
                            pos2(left, top),
                            title,
                            theme::medium(size),
                            PALETTE.text,
                            width,
                        );
                        theme::paint_line(
                            ui,
                            pos2(left, top + line_height + 3.0),
                            &line,
                            theme::regular(size),
                            PALETTE.secondary,
                            width,
                        );
                        if response.clicked() {
                            picked = Some(*item);
                        }
                    }
                });
        });
    if let Some(words) = &chosen {
        *text = words.clone();
        ui.memory_mut(|m| m.surrender_focus(field.id));
    }
    if let Some((words, token)) = forget {
        app.act(Action::ForgetSearch { words, token });
        // The box keeps the keyboard, and the list stays open.
        field.request_focus();
    }
    let done = picked.is_some();
    if let Some(item) = picked {
        match item {
            // A song plays, then songs like it (as a search result does).
            Item::Track(track) => {
                let radio = Target::Watch {
                    video_id: Some(track.video_id.clone()),
                    playlist_id: None,
                };
                app.act(Action::Play(radio, Some(track.clone())));
            }
            Item::Card(card) => {
                if let Some(target) = card.open.clone().or_else(|| card.play.clone()) {
                    app.act(Action::Open(target, card.song()));
                }
            }
        }
        ui.memory_mut(|m| m.surrender_focus(field.id));
        ui.data_mut(|d| d.insert_temp(id.with("closed"), typed.clone()));
    }
    ui.data_mut(|d| {
        d.insert_temp(id.with("rect"), area.response.rect);
        match marked {
            Some(m) => {
                d.insert_temp(id.with("marked"), m);
            }
            None => d.remove::<usize>(id.with("marked")),
        }
    });
    match chosen {
        Some(words) => Some(Chosen::Words(words)),
        None if done => Some(Chosen::Done),
        None => None,
    }
}

/// What a suggested song is, as YouTube Music says it: "Song • Coldplay •
/// 2.4B plays • Parachutes".
fn track_line(track: &Track) -> String {
    let kind = match track.kind {
        TrackKind::Song => "Song",
        TrackKind::MusicVideo => "Video",
        _ => "",
    };
    [
        kind,
        track.artists.as_str(),
        track.count().unwrap_or_default(),
        track.album.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" \u{2022} ")
}

/// A suggestion's words: the part that begins as typed in 500 weight, the
/// rest in 400, all white@0.50, 14 (16 from a window 1364 wide), cut at
/// `width`.
fn suggestion_words(
    ui: &egui::Ui,
    suggestion: &str,
    typed: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    use egui::text::{LayoutJob, TextFormat};
    let lead = if suggestion.to_lowercase().starts_with(&typed.to_lowercase()) {
        suggestion
            .char_indices()
            .nth(typed.chars().count())
            .map_or(suggestion.len(), |(at, _)| at)
    } else {
        0
    };
    let mut job = LayoutJob::default();
    let size = theme::text_size(ui);
    let format = |font| {
        let mut format = TextFormat::simple(font, PALETTE.hint);
        format.line_height = Some(size * 1.2);
        format
    };
    job.append(&suggestion[..lead], 0.0, format(theme::medium(size)));
    job.append(&suggestion[lead..], 0.0, format(theme::regular(size)));
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}
