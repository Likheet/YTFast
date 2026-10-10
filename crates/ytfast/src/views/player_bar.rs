//! The bar across the bottom, as YouTube Music's (`ytmusic-player-bar`,
//! measured at 1280 wide): on the left previous, play and next and the
//! time; in the middle the song (its cover, title and artist, like,
//! dislike and its menu), centred; on the right the volume, repeat,
//! shuffle and the arrow that opens the player page. The progress line
//! runs along its top edge.
//!
//! Premium: the bar is a glass panel floating over the page (corners 16,
//! inset 12 and 8, a clear edge, a light along its top, a shadow); its
//! middle column (36% of it, 28% under 1150 wide; 240 to 560) holds
//! previous, play on a white disc and next, and under them the song's line
//! in the accent between the time played and the length; the song at its
//! left, the other buttons at its right, all quieter than play.

use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{Action, App, LikeState, PlayState, Repeat};
use crate::backend::Route;
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE};
use crate::views::widgets;

/// The right group's width (a song: the volume slider 100, three buttons
/// of 48 with their margins, the arrow 36 and 4 after it).
const RIGHT_WIDTH: f32 = 284.0;

/// Below this window width, volume, repeat and shuffle move into a strip
/// that a "…" button opens (YouTube Music's `@media (max-width:1149px)`,
/// `ytmusic-player-expanding-menu`).
const NARROW: f32 = 1150.0;

/// The right group's width then: the "…" button 36 and 8, the arrow 36
/// and 4 after it.
const NARROW_RIGHT_WIDTH: f32 = 36.0 + 8.0 + 36.0 + 4.0 + 16.0;

/// `#f1f1f1`: like, dislike and the menu.
const SOFT: Color32 = Color32::from_rgb(0xf1, 0xf1, 0xf1);

/// Premium: the room either side of the song's line for the times.
const TIMES: f32 = 52.0;

/// Like, dislike and the menu: `#f1f1f1` and 24 (Premium: the quieter
/// grey and 22, so play stands out).
fn soft() -> (Color32, f32) {
    if theme::premium() {
        (PALETTE.secondary, 22.0)
    } else {
        (SOFT, 24.0)
    }
}

pub fn show(app: &App, ui: &mut egui::Ui) {
    let premium = theme::premium();
    // Premium: only the floating bar, whatever is behind showing round it.
    let fill = if premium {
        Color32::TRANSPARENT
    } else {
        PALETTE.panel
    };
    egui::Panel::bottom("player-bar")
        .exact_size(theme::player_bar_height())
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(fill))
        .show(ui, |ui| {
            let bar = if premium { floating(ui) } else { ui.max_rect() };
            let Some(entry) = &app.playback.entry else {
                if premium {
                    ui.painter().text(
                        pos2(bar.left() + 24.0, bar.center().y),
                        Align2::LEFT_CENTER,
                        "Choose something to play",
                        theme::regular(14.0),
                        PALETTE.secondary,
                    );
                }
                return;
            };
            // A click on the bar's empty parts opens (or closes) the
            // player page; the buttons, drawn after, take their own.
            let open = ui.interact(bar, ui.id().with("open-player"), Sense::click());
            if open.clicked() {
                app.act(Action::ToggleNowPlaying);
            }
            // A right-click on it opens the playing song's menu where the
            // pointer is, as its ⋮ does (and a song's row on a right-click).
            theme::context_menu(&open)
                .show(|ui| widgets::song_menu(app, ui, &entry.track, widgets::Place::Playing));

            let narrow = ui.ctx().content_rect().width() < NARROW;
            // Premium: the song's line, in the middle column under the
            // buttons.
            let mut track = None;
            let expand = if premium {
                // Narrower in a narrow window, so the song keeps its name.
                let share = if narrow { 0.28 } else { 0.36 };
                let column = (bar.width() * share).clamp(240.0, 560.0);
                let middle = Rect::from_center_size(bar.center(), vec2(column, bar.height()));
                let transport = Rect::from_center_size(
                    pos2(bar.center().x, bar.top() + 30.0),
                    vec2(160.0, 40.0),
                );
                left_group(app, ui, transport);
                track = Some(Rect::from_min_max(
                    pos2(middle.left() + TIMES, bar.bottom() - 18.0),
                    pos2(middle.right() - TIMES, bar.bottom() - 18.0),
                ));
                let expand = right_group(app, ui, bar, narrow);
                let (from, to) = (bar.left() + 12.0, middle.left() - 16.0);
                middle_group(app, ui, bar, from, to, entry);
                expand
            } else {
                let left_end = left_group(app, ui, bar);
                let right_start = bar.right()
                    - if narrow {
                        NARROW_RIGHT_WIDTH
                    } else {
                        RIGHT_WIDTH
                    };
                let expand = right_group(app, ui, bar, narrow);
                middle_group(app, ui, bar, left_end, right_start, entry);
                expand
            };
            // Over the song's words, which it covers while open.
            if let Some(expand) = expand {
                expanding_menu(app, ui, bar, expand);
            }
            // Last, so it is over the bar's own things.
            match track {
                Some(track) => {
                    if !premium_times(app, ui, track) {
                        progress_line(app, ui, track);
                    }
                }
                None => progress_line(app, ui, bar),
            }
        });
}

/// Premium's floating bar, a glass panel: inset 12 and 8, corners 16,
/// `#202024` at 90% (what is behind shows a little), a white@0.12 edge, a
/// white@0.07 light along its top and a shadow under it. Returns its
/// rectangle.
fn floating(ui: &egui::Ui) -> Rect {
    let bar = ui.max_rect().shrink2(vec2(12.0, 8.0));
    let corners = CornerRadius::same(16);
    let shadow = egui::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };
    ui.painter().add(shadow.as_shape(bar, corners));
    ui.painter().rect_filled(
        bar,
        corners,
        Color32::from_rgba_unmultiplied(0x20, 0x20, 0x24, 230),
    );
    ui.painter().hline(
        (bar.left() + 16.0)..=(bar.right() - 16.0),
        bar.top() + 1.5,
        egui::Stroke::new(1.0, Color32::from_white_alpha(18)),
    );
    ui.painter().rect_stroke(
        bar,
        corners,
        egui::Stroke::new(1.0, Color32::from_white_alpha(31)),
        egui::StrokeKind::Inside,
    );
    bar
}

/// A round button `size` across with its icon `icon_size` and no disc at
/// rest; white at 20% under the pointer, `disc` across.
fn bar_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    icon_size: f32,
    disc: f32,
    color: Color32,
    name: &str,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(("bar-button", name)), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    if response.hovered() && ui.is_enabled() {
        ui.painter()
            .circle_filled(rect.center(), disc / 2.0, PALETTE.surface_hover);
    }
    theme::pointing_or_not(ui, &response, ui.is_enabled());
    let color = if ui.is_enabled() {
        color
    } else {
        PALETTE.faint
    };
    if theme::premium() && response.has_focus() {
        ui.painter().circle_stroke(
            rect.center(),
            disc / 2.0,
            egui::Stroke::new(1.0, PALETTE.accent),
        );
    }
    theme::paint_icon(ui, icon, rect, icon_size, color);
    response.on_hover_text(name)
}

/// Previous (36, at 8), play or pause (a 40 spot at 60, its icon 40, a 52
/// disc under the pointer), next (36, at 120), then the time 8 after it.
/// Returns where the group ends (16 after the time). Premium: `bar` is the
/// buttons' own room, 160 wide; the time goes with the song's line.
fn left_group(app: &App, ui: &mut egui::Ui, bar: Rect) -> f32 {
    let premium = theme::premium();
    let y = bar.center().y;
    let previous = Rect::from_min_size(pos2(bar.left() + 8.0, y - 18.0), Vec2::splat(36.0));
    if bar_button(
        ui,
        previous,
        Icon::SkipBack,
        24.0,
        36.0,
        PALETTE.text,
        "Previous",
    )
    .clicked()
    {
        app.act(Action::Previous);
    }
    let spot = Rect::from_min_size(pos2(bar.left() + 60.0, y - 20.0), Vec2::splat(40.0));
    if app.playback.state == PlayState::Preparing {
        let mut inner = ui.new_child(UiBuilder::new().max_rect(spot));
        inner.put(
            Rect::from_center_size(spot.center(), Vec2::splat(16.0)),
            egui::Spinner::new().size(16.0).color(PALETTE.dim),
        );
        inner
            .interact(spot, inner.id().with("getting-ready"), Sense::hover())
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Getting the song ready")
            });
    } else {
        let playing = app.playback.state == PlayState::Playing && !app.audio_status.paused;
        let (glyph, name) = if playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        let clicked = if premium {
            ui.painter()
                .circle_filled(spot.center(), 21.0, PALETTE.text);
            bar_button(ui, spot, glyph, 26.0, 42.0, PALETTE.window, name).clicked()
        } else {
            bar_button(ui, spot, glyph, 40.0, 52.0, PALETTE.text, name).clicked()
        };
        if clicked {
            app.act(Action::TogglePause);
        }
    }
    let next = Rect::from_min_size(pos2(bar.left() + 120.0, y - 18.0), Vec2::splat(36.0));
    if bar_button(
        ui,
        next,
        Icon::SkipForward,
        24.0,
        36.0,
        PALETTE.text,
        "Next",
    )
    .clicked()
    {
        app.act(Action::Next);
    }

    if premium {
        return bar.right();
    }
    // "0:00 / 4:38", 12/400 #aaa, 8 after next (and its margin of 4).
    let mut x = next.right() + 4.0 + 8.0;
    if app.audio_status.length > 0.0 {
        let time = format!(
            "{} / {}",
            theme::clock(app.shown_position()),
            theme::clock(app.audio_status.length)
        );
        let at = ui.painter().text(
            pos2(x, y),
            Align2::LEFT_CENTER,
            time,
            theme::regular(12.0),
            PALETTE.dim,
        );
        x = at.right();
    }
    // YTFast's own words, which YouTube Music has no place for.
    if app.demo {
        let at = ui.painter().text(
            pos2(x + 8.0, y),
            Align2::LEFT_CENTER,
            "Demo",
            theme::regular(12.0),
            PALETTE.faint,
        );
        x = at.right();
    }
    if let Some(problem) = &app.audio_status.problem {
        let galley = theme::fit(ui, problem, theme::regular(12.0), PALETTE.danger, 200.0, 1);
        let at = pos2(x + 8.0, y - galley.size().y / 2.0);
        let rect = Rect::from_min_size(at, galley.size());
        ui.painter().galley(at, galley, PALETTE.danger);
        ui.interact(rect, ui.id().with("sound-problem"), Sense::hover())
            .on_hover_text(problem);
        x = rect.right();
    }
    x + 16.0
}

/// Premium: the time played at the left of the song's line `track` and the
/// length (and "Demo") at its right, 12 in the quieter grey, 10 from its
/// ends; or, when the sound has a problem, that instead of the line, in
/// red. True when the problem shows.
fn premium_times(app: &App, ui: &mut egui::Ui, track: Rect) -> bool {
    let y = track.center().y;
    if let Some(problem) = &app.audio_status.problem {
        let width = track.width() + 2.0 * TIMES;
        let words = theme::fit(ui, problem, theme::regular(11.0), PALETTE.danger, width, 1);
        let rect = Rect::from_center_size(pos2(track.center().x, y), words.size());
        ui.painter().galley(rect.min, words, PALETTE.danger);
        ui.interact(rect, ui.id().with("sound-problem"), Sense::hover())
            .on_hover_text(problem);
        return true;
    }
    let length = app.audio_status.length;
    let (played, mut total) = if length > 0.0 {
        (theme::clock(app.shown_position()), theme::clock(length))
    } else {
        ("-:--".to_string(), "-:--".to_string())
    };
    if app.demo {
        total.push_str(" \u{00b7} Demo");
    }
    let font = theme::regular(12.0);
    ui.painter().text(
        pos2(track.left() - 10.0, y),
        Align2::RIGHT_CENTER,
        played,
        font.clone(),
        PALETTE.secondary,
    );
    ui.painter().text(
        pos2(track.right() + 10.0, y),
        Align2::LEFT_CENTER,
        total,
        font,
        PALETTE.secondary,
    );
    false
}

/// The song: its cover 40 (r 2), 16 after it its title (14/500 white, 16
/// from a window 1364 wide) and "Artist • Album" (14/400 white@0.70, the
/// same 16, each a link), 8 after them like
/// and dislike (36 each), then the menu (36), 16 after; the group centred
/// between the left and right groups, the words as wide as they need up
/// to what is left.
fn middle_group(app: &App, ui: &mut egui::Ui, bar: Rect, from: f32, to: f32, entry: &Entry) {
    let track = &entry.track;
    let y = bar.center().y;
    // Cover 40, 16, the words, 8, like 36, 2.5, dislike 36, 16, menu 36, 16
    // (Premium: cover 48 r 8 and 12 after it, 8 before and after the menu,
    // at the left of its room).
    let premium = theme::premium();
    let (art_size, art_gap, menu_gap) = if premium {
        (48.0, 12.0, 8.0)
    } else {
        (40.0, 16.0, 16.0)
    };
    let fixed = art_size + art_gap + 8.0 + 36.0 + 2.5 + 36.0 + menu_gap + 36.0 + menu_gap;
    let room = (to - from - fixed).max(0.0);
    let (line, line_color) = match &app.playback.state {
        PlayState::Preparing => ("Getting the song ready...".to_string(), PALETTE.secondary),
        PlayState::Failed(message) => (message.clone(), PALETTE.danger),
        _ => (String::new(), PALETTE.secondary),
    };
    // Lines 1.2 times the words' size.
    let size = theme::text_size(ui);
    let line_height = size * 1.2;
    let title = theme::fit(ui, &track.title, theme::medium(size), PALETTE.text, room, 1);
    let parts = byline_parts(track);
    let under_width = if line.is_empty() {
        let joined = parts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join(" \u{2022} ");
        ui.fonts_mut(|f| {
            f.layout_no_wrap(joined, theme::regular(size), PALETTE.secondary)
                .size()
                .x
        })
    } else {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(line.clone(), theme::regular(size), line_color)
                .size()
                .x
        })
    };
    let words = title.size().x.max(under_width).min(room);
    let group = fixed + words;
    let left = if premium {
        from
    } else {
        from + ((to - from - group) / 2.0).max(0.0)
    };

    let art = Rect::from_min_size(pos2(left, y - art_size / 2.0), Vec2::splat(art_size));
    widgets::cover_with(
        app,
        ui,
        art,
        track.thumbnail.as_ref(),
        CornerRadius::same(if theme::dynamic() {
            crate::dynamic::RADIUS_ART
        } else if premium {
            8
        } else {
            2
        }),
    );
    let text_left = art.right() + art_gap;
    // Two lines, together centred down.
    let top = y - line_height;
    ui.painter().galley(
        pos2(text_left, top + (line_height - title.size().y) / 2.0),
        title,
        PALETTE.text,
    );
    let under_top = top + line_height;
    if line.is_empty() {
        byline(app, ui, pos2(text_left, under_top), &parts, words, size);
    } else {
        let galley = theme::fit(ui, &line, theme::regular(size), line_color, words, 1);
        ui.painter().galley(
            pos2(text_left, under_top + (line_height - galley.size().y) / 2.0),
            galley,
            line_color,
        );
    }
    // How the song was found, for whoever rests the pointer on the words.
    if !app.playback.format.is_empty() {
        let words_rect = Rect::from_min_size(pos2(text_left, top), vec2(words, 2.0 * line_height));
        ui.interact(words_rect, ui.id().with("how-found"), Sense::hover())
            .on_hover_text(&app.playback.format);
    }

    let mut x = text_left + words + 8.0;
    likes(app, ui, &mut x, y, track);
    x += menu_gap;
    let menu_rect = Rect::from_min_size(pos2(x, y - 18.0), Vec2::splat(36.0));
    let (color, size) = soft();
    let more = bar_button(ui, menu_rect, Icon::MoreVertical, size, 36.0, color, "More");
    theme::menu_popup(&more).show(|ui| widgets::song_menu(app, ui, track, widgets::Place::Playing));
}

/// The byline's parts: the artists (a link to the artist) and the album
/// (a link to it), as each is known.
fn byline_parts(track: &ytfast_core::read::Track) -> Vec<(String, Option<Route>)> {
    let mut parts = Vec::new();
    if !track.artists.is_empty() {
        let to = track.artist_id.clone().map(|id| Route::browse(id, None));
        parts.push((track.artists.clone(), to));
    }
    if let Some(album) = track.album.as_ref().filter(|a| !a.is_empty()) {
        let to = track.album_id.clone().map(|id| Route::browse(id, None));
        parts.push((album.clone(), to));
    }
    parts
}

/// "Artist • Album", each name a link (underlined under the pointer), cut
/// at `width`, in words `size` on a line 1.2 times as tall.
fn byline(
    app: &App,
    ui: &mut egui::Ui,
    at: egui::Pos2,
    parts: &[(String, Option<Route>)],
    width: f32,
    size: f32,
) {
    let font = theme::regular(size);
    let line_height = size * 1.2;
    let mut x = at.x;
    let end = at.x + width;
    for (index, (text, to)) in parts.iter().enumerate() {
        if index > 0 {
            let dot = ui.painter().text(
                pos2(x, at.y + line_height / 2.0),
                Align2::LEFT_CENTER,
                " \u{2022} ",
                font.clone(),
                PALETTE.secondary,
            );
            x = dot.right();
        }
        let room = end - x;
        if room <= 0.0 {
            break;
        }
        let galley = theme::fit(ui, text, font.clone(), PALETTE.secondary, room, 1);
        let rect = Rect::from_min_size(
            pos2(x, at.y + (line_height - galley.size().y) / 2.0),
            galley.size(),
        );
        ui.painter().galley(rect.min, galley, PALETTE.secondary);
        if let Some(route) = to {
            let link = ui.interact(rect, ui.id().with(("byline", index)), Sense::click());
            link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, text));
            if link.hovered() {
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 1.0,
                    egui::Stroke::new(1.0, PALETTE.secondary),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if link.clicked() {
                app.act(Action::Navigate(route.clone()));
            }
        }
        x = rect.right();
    }
}

/// Like, then dislike (2.5 apart), 36 each, their icons 24 `#f1f1f1`
/// (filled when chosen). Moves `x` past them.
fn likes(app: &App, ui: &mut egui::Ui, x: &mut f32, y: f32, track: &ytfast_core::read::Track) {
    let state = app
        .likes
        .get(&track.video_id)
        .copied()
        .unwrap_or(LikeState::Neutral);
    let liked = state == LikeState::Liked;
    let disliked = state == LikeState::Disliked;
    let like = Rect::from_min_size(pos2(*x, y - 18.0), Vec2::splat(36.0));
    let dislike = Rect::from_min_size(pos2(like.right() + 2.5, y - 18.0), Vec2::splat(36.0));
    *x = dislike.right();

    let icon = if liked {
        Icon::ThumbsUpFilled
    } else {
        Icon::ThumbsUp
    };
    let (color, size) = soft();
    // Chosen, it is white.
    let tint = if liked { PALETTE.text } else { color };
    let response = bar_button(ui, like, icon, size, 36.0, tint, "Like");
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, liked, "Like"));
    if response.clicked() {
        let next = if liked {
            LikeState::Neutral
        } else {
            LikeState::Liked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
    let icon = if disliked {
        Icon::ThumbsDownFilled
    } else {
        Icon::ThumbsDown
    };
    let tint = if disliked { PALETTE.text } else { color };
    let response = bar_button(ui, dislike, icon, size, 36.0, tint, "Dislike");
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, disliked, "Dislike")
    });
    if response.clicked() {
        let next = if disliked {
            LikeState::Neutral
        } else {
            LikeState::Disliked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
}

/// From the right edge: the arrow (36, 4 in), shuffle, repeat and volume
/// (36 each, 48 apart, grey `#909090`, white when on), and the volume's
/// slider (100) left of them.
/// The player page's arrow at the right end, and before it volume, repeat
/// and shuffle; in a narrow window, the "…" button in their place, whose
/// spot is returned.
fn right_group(app: &App, ui: &mut egui::Ui, bar: Rect, narrow: bool) -> Option<Rect> {
    let y = bar.center().y;
    let arrow = Rect::from_min_size(pos2(bar.right() - 4.0 - 36.0, y - 18.0), Vec2::splat(36.0));
    page_arrow(app, ui, arrow);
    let next_to_arrow =
        Rect::from_min_size(pos2(arrow.left() - 8.0 - 36.0, y - 18.0), Vec2::splat(36.0));
    if narrow {
        return Some(next_to_arrow);
    }
    controls(app, ui, next_to_arrow);
    None
}

/// Shuffle in `shuffle`, then repeat and volume (with its slider) 48 and
/// 96 before it.
fn controls(app: &App, ui: &mut egui::Ui, shuffle: Rect) {
    let repeat = shuffle.translate(vec2(-48.0, 0.0));
    let volume_button = repeat.translate(vec2(-48.0, 0.0));

    // Shuffle is a mode, white while on.
    let (color, name) = if app.settings.shuffle {
        (PALETTE.text, "Shuffle is on")
    } else {
        (PALETTE.quiet, "Shuffle is off")
    };
    if bar_button(ui, shuffle, Icon::Shuffle, 24.0, 36.0, color, name).clicked() {
        app.act(Action::ShuffleQueue);
    }
    let (glyph, color, name) = match app.settings.repeat {
        Repeat::Off => (Icon::Repeat, PALETTE.quiet, "Repeat is off"),
        Repeat::All => (Icon::Repeat, PALETTE.text, "Repeating the queue"),
        Repeat::One => (Icon::RepeatOne, PALETTE.text, "Repeating this song"),
    };
    if bar_button(ui, repeat, glyph, 24.0, 36.0, color, name).clicked() {
        app.act(Action::CycleRepeat);
    }
    volume(app, ui, volume_button);
}

/// A narrow window's "…" button (YouTube Music's expand button, ⋮ turned
/// across) and the strip it opens to its left: `#212121`, padding 0 8,
/// fading in over 0.2 s, holding volume (with its slider), repeat and
/// shuffle. A click elsewhere closes it.
fn expanding_menu(app: &App, ui: &mut egui::Ui, bar: Rect, button: Rect) {
    let id = ui.id().with("expanding-menu");
    let mut open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let response = ui.interact(button, id.with("button"), Sense::click());
    let name = if open {
        "Close the volume, repeat and shuffle"
    } else {
        "Volume, repeat and shuffle"
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if response.hovered() {
        ui.painter()
            .circle_filled(button.center(), 18.0, PALETTE.surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Three dots across, 4 apart.
    for dx in [-6.0, 0.0, 6.0] {
        ui.painter()
            .circle_filled(button.center() + vec2(dx, 0.0), 2.0, SOFT);
    }
    // The strip: volume's slider 100, 4, its button 36, 12, repeat 36, 12,
    // shuffle 36, with 8 at each end.
    let width = 8.0 + 100.0 + 4.0 + 36.0 + 12.0 + 36.0 + 12.0 + 36.0 + 8.0;
    let strip = Rect::from_min_max(
        pos2(button.left() - 8.0 - width, bar.top()),
        pos2(button.left() - 8.0, bar.bottom()),
    );
    if response.clicked() {
        open = !open;
    } else if open && ui.input(|i| i.pointer.primary_clicked()) && !ui.rect_contains_pointer(strip)
    {
        open = false;
    }
    let shown = ui.ctx().animate_bool_with_time(id.with("shown"), open, 0.2);
    if shown > 0.0 {
        // Over the song's words: the strip takes the pointer there.
        ui.interact(strip, id.with("strip"), Sense::click());
        if theme::dynamic() {
            // Dynamic Background: glass over the song's words.
            let mut faded = ui.painter().clone();
            faded.set_opacity(shown);
            crate::dynamic::glass(
                &faded,
                strip.shrink2(vec2(0.0, 10.0)),
                CornerRadius::same(crate::dynamic::RADIUS_PANEL),
            );
        } else {
            ui.painter()
                .rect_filled(strip, 0.0, PALETTE.panel.gamma_multiply(shown));
        }
        if open {
            let shuffle = Rect::from_min_size(
                pos2(strip.right() - 8.0 - 36.0, bar.center().y - 18.0),
                Vec2::splat(36.0),
            );
            controls(app, ui, shuffle);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, open));
}

/// The triangle that opens the player page (pointing up) and closes it
/// (pointing down), as YouTube Music draws it; it turns over 0.3 s.
fn page_arrow(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let name = if app.now_playing {
        "Close the player page"
    } else {
        "Open the player page"
    };
    let response = ui.interact(rect, ui.id().with("page-arrow"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if response.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 18.0, PALETTE.surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // 0 when the page is open (pointing down), 1 when closed (up).
    let turn = ui
        .ctx()
        .animate_bool_with_time(response.id.with("turn"), !app.now_playing, 0.3);
    let angle = std::f32::consts::PI * turn;
    let (sin, cos) = angle.sin_cos();
    if theme::premium() {
        // Premium: a chevron drawn in lines 2 thick, as its other icons,
        // not a solid triangle; turned the same way.
        let centre = rect.center();
        let point = |x: f32, y: f32| centre + vec2(x * cos - y * sin, x * sin + y * cos);
        ui.painter().add(egui::Shape::line(
            vec![point(-6.0, -3.0), point(0.0, 3.0), point(6.0, -3.0)],
            egui::Stroke::new(2.0, PALETTE.text),
        ));
    } else {
        // YouTube Music's triangle, in a 24 box: from (4, 7) and (20, 7) to
        // (12, 19), turned half a circle when closed.
        let icon = Rect::from_center_size(rect.center(), Vec2::splat(24.0));
        let point = |x: f32, y: f32| {
            let (dx, dy) = (x - 12.0, y - 13.0);
            icon.center() + vec2(dx * cos - dy * sin, dx * sin + dy * cos)
        };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![point(4.0, 7.0), point(20.0, 7.0), point(12.0, 19.0)],
            PALETTE.text,
            egui::Stroke::NONE,
        ));
    }
    if response.clicked() {
        app.act(Action::ToggleNowPlaying);
    }
    response.on_hover_text(name);
}

/// The volume button (click to mute) and its slider: 100 wide to its left,
/// its room always kept, shown (fading over 0.2 s) while the pointer is on
/// either. The slider's rail is 68 by 2, `#909090`, white up to the
/// volume, with a white knob 12 across. Scrolling over either turns the
/// volume.
fn volume(app: &App, ui: &mut egui::Ui, button: Rect) {
    let id = ui.id().with("volume");
    let slot = Rect::from_min_max(
        pos2(button.left() - 4.0 - 100.0, button.top()),
        pos2(button.left() - 4.0, button.bottom()),
    );
    let region = slot.union(button);
    let was_open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let shown = ui
        .ctx()
        .animate_bool_with_time(id.with("open"), was_open, 0.2);

    let mut volume = app.settings.volume;
    let muted = volume <= 0.001;
    let (icon, name) = if muted {
        (Icon::Muted, "Unmute")
    } else {
        (Icon::Volume, "Mute")
    };
    let color = if was_open {
        PALETTE.text
    } else {
        PALETTE.quiet
    };
    if bar_button(ui, button, icon, 24.0, 36.0, color, name).clicked() {
        app.act(Action::ToggleMute);
    }

    let mut dragging = false;
    if shown > 0.0 {
        let mut response = ui.interact(slot, id.with("slider"), Sense::click_and_drag());
        dragging = response.dragged();
        let rail = Rect::from_center_size(slot.center(), vec2(68.0, 2.0));
        if let Some(pointer) = response.interact_pointer_pos()
            && (response.is_pointer_button_down_on() || response.clicked())
        {
            let value = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
            if value != volume {
                volume = value;
                response.mark_changed();
            }
        }
        let value = volume;
        response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(value), "Volume"));
        let fade = |c: Color32| c.gamma_multiply(shown);
        let x = rail.left() + rail.width() * volume;
        ui.painter().rect_filled(rail, 0.0, fade(PALETTE.quiet));
        ui.painter().rect_filled(
            Rect::from_min_max(rail.min, pos2(x, rail.bottom())),
            0.0,
            fade(PALETTE.text),
        );
        ui.painter()
            .circle_filled(pos2(x, rail.center().y), 6.0, fade(PALETTE.text));
        if response.changed() {
            app.act(Action::SetVolume(volume));
        }
    }
    let over = ui.rect_contains_pointer(region) || dragging;
    if over != was_open {
        ui.data_mut(|d| d.insert_temp(id, over));
        ui.ctx().request_repaint();
    }
    if over {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let next = (volume + scroll * 0.002).clamp(0.0, 1.0);
            app.act(Action::SetVolume(next));
        }
    }
}

/// The line along the top edge (`#progress-bar`): 2 high, white@0.10,
/// played in red (`#ff0033`, its last fifth turning to `#ff2791`); 4 high
/// while the pointer is on it. Its knob (12, `#ff0033`) shows whenever the
/// pointer is over the bar, and the time under the pointer shows above
/// it. Click or drag (anywhere 15 above to 17 below its centre) to move in
/// the song.
fn progress_line(app: &App, ui: &mut egui::Ui, bar: Rect) {
    // Premium: `bar` is the line itself, in the accent, 4 thick (6 under
    // the pointer) and rounded, on white@0.16; its knob shows only under
    // the pointer.
    let premium = theme::premium();
    let (red, pink) = if premium {
        (PALETTE.accent, PALETTE.accent)
    } else {
        (
            Color32::from_rgb(0xff, 0x00, 0x33),
            Color32::from_rgb(0xff, 0x27, 0x91),
        )
    };
    // Dynamic Background: the accent, white.
    let (red, pink) = if theme::dynamic() {
        (PALETTE.accent, PALETTE.accent)
    } else {
        (red, pink)
    };
    let length = app.audio_status.length.max(0.0);
    let fraction = if length > 0.0 {
        (app.shown_position() / length).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // The line's centre is 1 below the bar's top; its pointer area runs 15
    // above to 17 below, over the page's foot.
    let centre = if premium {
        bar.center().y
    } else {
        bar.top() + 1.0
    };
    let (above, below) = if premium { (10.0, 10.0) } else { (15.0, 17.0) };
    let area_rect = Rect::from_min_max(
        pos2(bar.left(), centre - above),
        pos2(bar.right(), centre + below),
    );
    let layer = egui::LayerId::new(egui::Order::Middle, ui.id().with("progress-line"));
    // An area of its own, so the part above the bar takes the pointer too.
    let area = egui::Area::new(ui.id().with("progress-area"))
        .order(egui::Order::Middle)
        .fixed_pos(area_rect.min)
        .show(ui.ctx(), |ui| {
            ui.allocate_exact_size(area_rect.size(), Sense::click_and_drag())
                .1
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    area.widget_info(|| {
        egui::WidgetInfo::slider(true, f64::from(fraction), "Position in the song")
    });
    let hovered = area.hovered() || area.dragged();
    // While dragging, the line follows the pointer; the song moves when
    // the button is let go.
    let shown = match area.interact_pointer_pos() {
        Some(pointer) if area.dragged() => ((pointer.x - bar.left()) / bar.width()).clamp(0.0, 1.0),
        _ => fraction,
    };
    let painter = ui.ctx().layer_painter(layer);
    let half = match (premium, hovered) {
        (true, true) => 3.0,
        (true, false) => 2.0,
        (false, true) => 2.0,
        (false, false) => 1.0,
    };
    let line = Rect::from_min_max(
        pos2(bar.left(), centre - half),
        pos2(bar.right(), centre + half),
    );
    if premium {
        // A track that can be seen: white@0.18.
        painter.rect_filled(line, CornerRadius::same(3), Color32::from_white_alpha(46));
    } else {
        painter.rect_filled(line, 0.0, PALETTE.surface);
    }
    let end = line.left() + line.width() * shown;
    if premium {
        if end > line.left() {
            let played = Rect::from_min_max(line.min, pos2(end, line.bottom()));
            painter.rect_filled(played, CornerRadius::same(3), red);
        }
        if hovered {
            painter.circle_filled(pos2(end, centre), 6.0, PALETTE.text);
        }
    } else if end > line.left() {
        // Red to 80% of the played part, then turning pink to its end.
        let turn = line.left() + (end - line.left()) * 0.8;
        painter.rect_filled(
            Rect::from_min_max(line.min, pos2(turn, line.bottom())),
            0.0,
            red,
        );
        let mut mesh = egui::Mesh::default();
        let fade = Rect::from_min_max(pos2(turn, line.top()), pos2(end, line.bottom()));
        mesh.colored_vertex(fade.left_top(), red);
        mesh.colored_vertex(fade.right_top(), pink);
        mesh.colored_vertex(fade.right_bottom(), pink);
        mesh.colored_vertex(fade.left_bottom(), red);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
    let over_bar = ui.rect_contains_pointer(bar) || hovered;
    if over_bar && !premium {
        let knob = if area.dragged() {
            21.0
        } else if hovered {
            14.0
        } else {
            12.0
        };
        painter.circle_filled(pos2(end, centre), knob / 2.0, red);
    }
    // The time under the pointer, 8 above the line.
    if hovered
        && length > 0.0
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
    {
        let at = ((pointer.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64 * length;
        let galley = painter.layout_no_wrap(theme::clock(at), theme::regular(12.0), PALETTE.text);
        let size = galley.size() + vec2(16.0, 8.0);
        let label =
            Rect::from_min_size(pos2(pointer.x - size.x / 2.0, centre - 8.0 - size.y), size);
        if theme::dynamic() {
            // Dynamic Background: glass, corners 12.
            crate::dynamic::glass(
                &painter,
                label,
                CornerRadius::same(crate::dynamic::RADIUS_PANEL),
            );
        } else {
            painter.rect_filled(label, CornerRadius::same(2), PALETTE.panel);
        }
        painter.galley(label.min + vec2(8.0, 4.0), galley, PALETTE.text);
    }
    if length > 0.0
        && (area.clicked() || area.drag_stopped())
        && let Some(pointer) = area.interact_pointer_pos()
    {
        let at = ((pointer.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64 * length;
        app.act(Action::Seek(at));
    }
}
