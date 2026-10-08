//! The bar across the top, as YouTube Music's (`ytmusic-nav-bar`, measured
//! at 1280 wide): the menu button and the app's mark, the search box over
//! the page's own column, and the account at the content's right edge.
//! Back and forward (a web browser has its own; this window has not) sit
//! just left of the account. Transparent at the top of a page, the bar
//! turns solid with a hairline under it once the page scrolls.

use std::time::Duration;

use egui::{Align2, CornerRadius, Frame, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use ytfast_core::read::{Item, Target, Track, TrackKind};

use crate::app::{Action, App, Auth};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE, Round};
use crate::views::{sidebar, widgets};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(theme::TOP_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |ui| {
            let bar = ui.max_rect();
            let guide = sidebar::width(app);
            // (An album's background, painted behind everything, shows
            // through the bar while the page is at its top.)
            if !app.settings.mini_guide {
                // The open menu is solid from the window's top, behind the
                // logo too (`#guide-wrapper`).
                let corner = Rect::from_min_max(bar.min, pos2(bar.left() + guide, bar.bottom()));
                ui.painter().rect_filled(corner, 0.0, PALETTE.window);
            }
            // Once the page scrolls (or the player page is open), the bar
            // turns `#030303` with a white@0.15 line under it, fading over
            // 0.2 s (`#nav-bar-background`).
            let solid = app.page_scrolled.get() || app.now_playing;
            let shown = ui
                .ctx()
                .animate_bool_with_time(ui.id().with("solid"), solid, 0.2);
            if shown > 0.0 {
                ui.painter()
                    .rect_filled(bar, 0.0, PALETTE.window.gamma_multiply(shown));
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

            // The page's grid, so the search box and the account line up
            // with what is under them (on search, its narrower column).
            let window = ui.ctx().content_rect().width();
            let area = bar.width() - guide;
            let grid = theme::Grid::new(window, area);
            let column = if matches!(app.route, Route::Search(_) | Route::SearchOnly(..)) {
                theme::Grid::search(window, area)
            } else {
                grid
            };
            let search_left = (bar.left() + guide + column.left).max(name.right() + 12.0);
            let account_right = bar.left() + guide + grid.left + grid.width;

            // The account: 26 round, its right edge at the content's.
            let avatar = Rect::from_center_size(
                pos2(account_right - 13.0, bar.center().y),
                Vec2::splat(26.0),
            );
            account(app, ui, avatar);

            // Back and forward, 8 before the account (where YouTube
            // Music's cast button stands).
            let arrows = Rect::from_min_max(
                pos2(avatar.left() - 8.0 - 80.0, bar.center().y - 20.0),
                pos2(avatar.left() - 8.0, bar.center().y + 20.0),
            );
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

            // The search box: at most 480, 42 high from y 11.
            let width = (arrows.left() - 16.0 - search_left).clamp(120.0, 480.0);
            let field = Rect::from_min_size(pos2(search_left, bar.top() + 11.0), vec2(width, 42.0));
            search_box(app, ui, field);
        });
}

/// The account's round button, and its menu: History, Settings, signing
/// out.
fn account(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let Auth::SignedIn { name } = &app.auth else {
        return;
    };
    let response = ui.interact(rect, ui.id().with("account"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Account"));
    // YouTube Music shows the account's photo on `#909090`; until the
    // photo is read, the name's first letter.
    ui.painter()
        .circle_filled(rect.center(), rect.width() / 2.0, PALETTE.quiet);
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        theme::medium(13.0),
        PALETTE.text,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::menu_popup(&response).show(|ui| {
        theme::menu(ui);
        // The account's name, in line with the menu's words.
        let (who, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        let under = if app.demo { "Demo" } else { "YouTube Music" };
        theme::paint_line(
            ui,
            who.left_top() + vec2(16.0, 7.0),
            name,
            theme::medium(16.0),
            PALETTE.text,
            who.width() - 32.0,
        );
        theme::paint_line(
            ui,
            who.left_top() + vec2(16.0, 28.0),
            under,
            theme::regular(13.0),
            PALETTE.dim,
            who.width() - 32.0,
        );
        ui.separator();
        let go = |ui: &mut egui::Ui, icon: Icon, text: &str, route: Route| {
            if theme::menu_item(ui, icon, text).clicked() {
                app.act(Action::Navigate(route));
                ui.close();
            }
        };
        go(ui, Icon::History, "History", Route::History);
        go(ui, Icon::Settings, "Settings", Route::Settings);
        if !app.demo {
            ui.separator();
            if theme::menu_item(ui, Icon::LogOut, "Sign out").clicked() {
                app.act(Action::SignOut);
                ui.close();
            }
        }
    });
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
    let fill = if focused || open {
        PALETTE.window
    } else {
        PALETTE.field
    };
    let corners = if open {
        CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        }
    } else {
        CornerRadius::same(8)
    };
    ui.painter().rect(
        rect,
        corners,
        fill,
        egui::Stroke::new(1.0, PALETTE.divider),
        egui::StrokeKind::Inside,
    );
    let tint = if focused { PALETTE.text } else { PALETTE.hint };
    let icon = Rect::from_center_size(pos2(rect.left() + 27.0, rect.center().y), Vec2::splat(18.0));
    theme::paint_icon(ui, Icon::Search, icon, 18.0, tint);

    let field_rect = Rect::from_min_max(
        pos2(rect.left() + 53.0, rect.top() + 10.0),
        pos2(rect.right() - 56.0, rect.bottom() - 10.0),
    );
    let hint = egui::RichText::new("Search songs, albums, artists, podcasts").color(PALETTE.hint);
    let edit = egui::TextEdit::singleline(&mut text)
        .id(super::SEARCH_BOX.into())
        .hint_text(hint)
        .font(theme::regular(16.0))
        .frame(Frame::NONE)
        .text_color(PALETTE.text);
    let response = ui.put(field_rect, edit);

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

/// What YouTube Music suggests for the words typed so far, as its list
/// under the box (`#suggestion-list`): right under it, as wide, `#030303`
/// with a white@0.15 border and its lower corners rounded 8, padding 8;
/// up to 6 rows of words 48 high (the search icon 18 `#909090` centred 25
/// in, the words 14 at 55 in, white@0.50, the typed part in 500 weight),
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
    // Asked for once typing pauses for a moment.
    let asked: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if typed != asked {
        let (pending, since): (String, f64) = ui
            .data(|d| d.get_temp(id.with("pending")))
            .unwrap_or_default();
        let waited = now - since;
        if typed.is_empty() {
            // Nothing to ask about.
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
        && !typed.is_empty()
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
    let words: Vec<&String> = found.words.iter().take(6).collect();
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
    if let Some(m) = marked
        && field.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        match words.get(m) {
            Some(words) => chosen = Some((*words).clone()),
            None => picked = items.get(m - words.len()).copied(),
        }
    }

    // A row's fill under the pointer, or marked by the keys (with a blue
    // edge).
    let fill = |ui: &egui::Ui, row: Rect, hovered: bool, is_marked: bool| {
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
    let area = egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_bottom() - vec2(0.0, 1.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(PALETTE.window)
                .stroke(egui::Stroke::new(1.0, PALETTE.divider))
                .corner_radius(CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                })
                .inner_margin(Margin::symmetric(0, 8))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 10,
                    spread: 1,
                    color: egui::Color32::from_black_alpha(36),
                })
                .show(ui, |ui| {
                    ui.set_width(rect.width() - 2.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, suggestion) in words.iter().enumerate() {
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                suggestion.as_str(),
                            )
                        });
                        fill(ui, row, response.hovered(), marked == Some(index));
                        let icon = Rect::from_center_size(
                            pos2(row.left() + 25.0, row.center().y),
                            Vec2::splat(18.0),
                        );
                        theme::paint_icon(ui, Icon::Search, icon, 18.0, PALETTE.quiet);
                        let words =
                            suggestion_words(ui, suggestion, &typed, row.width() - 55.0 - 16.0);
                        let at = pos2(row.left() + 55.0, row.center().y - words.size().y / 2.0);
                        ui.painter().galley(at, words, PALETTE.hint);
                        if response.clicked() {
                            chosen = Some((*suggestion).clone());
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
                        let top = row.center().y - (16.8 * 2.0 + 3.0) / 2.0;
                        theme::paint_line(
                            ui,
                            pos2(left, top),
                            title,
                            theme::medium(14.0),
                            PALETTE.text,
                            width,
                        );
                        theme::paint_line(
                            ui,
                            pos2(left, top + 16.8 + 3.0),
                            &line,
                            theme::regular(14.0),
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
/// rest in 400, all white@0.50, cut at `width`.
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
    let format = |font| {
        let mut format = TextFormat::simple(font, PALETTE.hint);
        format.line_height = Some(16.8);
        format
    };
    job.append(&suggestion[..lead], 0.0, format(theme::medium(14.0)));
    job.append(&suggestion[lead..], 0.0, format(theme::regular(14.0)));
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}
