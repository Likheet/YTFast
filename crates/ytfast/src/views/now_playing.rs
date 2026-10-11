//! The player page, laid out as YouTube Music's: the playing song's cover
//! large on the left, and on the right a panel with the tabs Up next,
//! Lyrics and Related, on the window's own near-black. Lyrics follow the
//! song in the Even Better Lyrics Plus way: the line being sung lit word
//! by word as it is sung, the rest dimmed, scrolling smoothly; click a
//! line to jump there. Translate shows each line in Latin letters and in
//! English under it.
//!
//! Every look puts its parts in the same places ([`places`]); the Song and
//! Video switch stays at the top, as YouTube Music's, and the cover or the
//! video is centred under it. Premium draws the cover rounder (r 16), the
//! tabs in clear glass, and the lyrics larger.

use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2,
};

use crate::app::{Action, App, NpTab, PlayState};
use crate::lyrics::{Lyrics, State};
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE};
use crate::views::{lyric_lines, page, queue_panel, video, widgets};

pub fn show(app: &App, ui: &mut egui::Ui) {
    if theme::dynamic() {
        return super::dynamic::player_page(app, ui);
    }
    let area = ui.max_rect();
    let Some(entry) = &app.playback.entry else {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            "Play something to see it here.",
            theme::regular(16.0),
            PALETTE.secondary,
        );
        return;
    };

    let places = places(ui.ctx().content_rect().width(), area, app.fullscreen);
    picture(app, ui, &places, entry);
    let panel = places.panel;

    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(panel)
            .layout(Layout::top_down(Align::Min)),
    );
    if theme::premium() {
        // Nothing drawn past the column's edges.
        ui.set_clip_rect(panel);
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    tabs(app, &mut ui, entry);
    match app.np_tab {
        NpTab::UpNext => queue_panel::list(app, &mut ui),
        NpTab::Lyrics => lyrics(app, &mut ui, entry),
        NpTab::Related => related(app, &mut ui, entry),
    }
}

/// The player page's places, the same in every look (the looks differ
/// only in how they draw them).
pub(super) struct Places {
    /// The Song and Video switch's row: at the top of the room left of
    /// the panel, always at the same height, as YouTube Music's.
    pub switch: Rect,
    /// Under it, the room the cover or the video is centred in.
    pub room: Rect,
    /// Up next, Lyrics and Related.
    pub panel: Rect,
}

/// The player page's places in `area` (the window `window` wide): YouTube
/// Music's, or in full screen [`fullscreen_places`].
pub(super) fn places(window: f32, area: Rect, fullscreen: bool) -> Places {
    let (main, panel) = if fullscreen {
        fullscreen_places(area)
    } else {
        page_places(window, area)
    };
    let switch = Rect::from_min_size(main.min, vec2(main.width(), video::SWITCH_HEIGHT));
    let room = Rect::from_min_max(
        pos2(
            main.left(),
            (main.top() + video::SWITCH_ROOM).min(main.bottom()),
        ),
        main.max,
    );
    Places {
        switch,
        room,
        panel,
    }
}

/// YouTube Music's places on the player page, by the window's width: the
/// space above, at the sides, between the cover and the panel, and the
/// panel's share of the rest (at most 800). Returns the room left of the
/// panel (as tall as the space above leaves) and the panel.
fn page_places(window: f32, area: Rect) -> (Rect, Rect) {
    let (top, side, gap, share) = if window >= 1800.0 {
        (64.0, 96.0, 96.0, 0.36)
    } else if window >= 1578.0 {
        (48.0, 64.0, 64.0, 0.36)
    } else if window >= 1364.0 {
        (40.0, 56.0, 56.0, 0.36)
    } else if window >= 1150.0 {
        (32.0, 56.0, 56.0, 0.36)
    } else {
        (24.0, 48.0, 48.0, 0.40)
    };
    let inner = Rect::from_min_max(
        pos2(area.left() + side, area.top() + top),
        pos2(area.right() - side, area.bottom()),
    );
    let panel_width = (inner.width() * share).min(800.0);
    let panel = Rect::from_min_max(pos2(inner.right() - panel_width, inner.top()), inner.max);
    let main = Rect::from_min_max(
        inner.min,
        pos2((panel.left() - gap).max(inner.left()), inner.bottom() - top),
    );
    (main, panel)
}

/// The cover in `room`: square, as large as fits and at most `largest`
/// (YouTube Music's 800; none in full screen), centred.
pub(super) fn cover_in(room: Rect, largest: f32) -> Rect {
    let side = room.width().min(room.height()).clamp(0.0, largest);
    Rect::from_center_size(room.center(), Vec2::splat(side))
}

/// The video in `room`: 16:9, as large as fits, centred.
pub(super) fn video_in(room: Rect) -> Rect {
    let width = room.width().min(room.height() * 16.0 / 9.0).max(0.0);
    Rect::from_center_size(room.center(), vec2(width, width * 9.0 / 16.0))
}

/// The Song and Video switch, and under it the cover (`#606060` while it
/// loads) or, in the video mode, the video in its place with square
/// corners (YouTube Music's `#player` keeps its 800 and its r 8 for the
/// cover: `:not([video-mode])`). Premium: both r 16. A fan's video, which
/// has only its still, shows it whole in the cover's square, 16:9.
fn picture(app: &App, ui: &egui::Ui, places: &Places, entry: &Entry) {
    let premium = theme::premium();
    video::switch(app, ui, places.switch);
    if video::shown(app) {
        let place = video_in(places.room);
        let corners = if premium {
            CornerRadius::same(16)
        } else {
            CornerRadius::ZERO
        };
        video::paint(app, ui, place, corners);
        cover_click(app, ui, place);
        return;
    }
    let largest = if app.fullscreen { f32::INFINITY } else { 800.0 };
    let art = cover_in(places.room, largest);
    let corners = CornerRadius::same(if premium { 16 } else { 8 });
    let thumb = entry.track.thumbnail.as_ref();
    if !widgets::big_picture(app, ui, art, thumb, corners) {
        ui.painter()
            .rect_filled(widgets::shape_in(art, thumb), corners, PALETTE.thumb);
    }
    cover_click(app, ui, art);
    video::note(app, ui, art);
}

/// The player page's places in full screen, in every look: margins of 4%
/// of the screen (48 to 120 at the sides, 32 to 80 above and below), the
/// panel (Up next, Lyrics, Related) 30% of what is inside (400 to 720), 4%
/// between them, and the rest for the cover or the video, as large as it
/// fits there. Returns that room and the panel.
pub(super) fn fullscreen_places(area: Rect) -> (Rect, Rect) {
    let side = (area.width() * 0.04).clamp(48.0, 120.0);
    let top = (area.height() * 0.05).clamp(32.0, 80.0);
    let inner = Rect::from_min_max(
        pos2(area.left() + side, area.top() + top),
        pos2(area.right() - side, area.bottom() - top),
    );
    let gap = side;
    let panel_width = (inner.width() * 0.30)
        .clamp(400.0, 720.0)
        .min((inner.width() - gap) / 2.0)
        .max(0.0);
    let panel = Rect::from_min_max(
        pos2(inner.right() - panel_width, inner.top()),
        pos2(inner.right(), area.bottom()),
    );
    let main = Rect::from_min_max(
        inner.min,
        pos2((panel.left() - gap).max(inner.left()), inner.bottom()),
    );
    (main, panel)
}

/// Clear glass, as an iPhone's, as a capsule over `rect`: white `fill`
/// inside (nearly none) and, when `rim` is more than none, one edge of
/// light in white `rim`.
pub(super) fn glass(painter: &egui::Painter, rect: Rect, fill: f32, rim: f32) {
    let radius = CornerRadius::same((rect.height() / 2.0).round() as u8);
    let white = |alpha: f32| Color32::from_white_alpha((alpha * 255.0) as u8);
    painter.rect_filled(rect, radius, white(fill));
    if rim > 0.0 {
        let edge = egui::Stroke::new(1.0, white(rim));
        painter.rect_stroke(rect, radius, edge, egui::StrokeKind::Inside);
    }
}

/// A click on the cover pauses and plays again, as on YouTube Music, and
/// shows what it did: a 52 circle of black@0.30 with a 36 icon, growing to
/// twice its size as it fades over 0.5 s (`#bezel`).
fn cover_click(app: &App, ui: &egui::Ui, art: Rect) {
    let sounding = app.playback.state == PlayState::Playing && !app.audio_status.paused;
    let id = ui.id().with("cover");
    let response = ui.interact(art, id, Sense::click());
    response.widget_info(|| {
        let name = if sounding { "Pause" } else { "Play" };
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name)
    });
    let now = ui.input(|i| i.time);
    if response.clicked() {
        app.act(Action::TogglePause);
        // What the click does: pause what sounds, or play.
        ui.data_mut(|d| d.insert_temp(id.with("bezel"), (now, sounding)));
    }
    let Some((since, paused)) = ui.data(|d| d.get_temp::<(f64, bool)>(id.with("bezel"))) else {
        return;
    };
    let t = ((now - since) / 0.5) as f32;
    if t >= 1.0 {
        ui.data_mut(|d| d.remove::<(f64, bool)>(id.with("bezel")));
        return;
    }
    ui.ctx().request_repaint();
    let grow = 1.0 + t;
    let fade = 1.0 - t;
    let disc = Rect::from_center_size(art.center(), Vec2::splat(52.0 * grow));
    ui.painter().circle_filled(
        disc.center(),
        26.0 * grow,
        Color32::from_black_alpha((77.0 * fade) as u8),
    );
    let icon = if paused { Icon::Pause } else { Icon::Play };
    theme::paint_icon(
        ui,
        icon,
        disc,
        36.0 * grow,
        Color32::from_white_alpha((255.0 * fade) as u8),
    );
}

/// UP NEXT, LYRICS, RELATED: words in capitals in a row 48 high over a
/// hairline, each tab its words and 12 either side, the chosen one white
/// with a white line 1 thick under it that slides to the tab chosen (0.15
/// s); the others white@0.56 (0.70 seen at 0.8), LYRICS at 0.30 and not to
/// be chosen when the song has none. Premium, in the same places: the tabs
/// on a clear glass capsule 40 high, the chosen one in clear glass with a
/// rim of light, no hairline.
fn tabs(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    let premium = theme::premium();
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 49.0), Sense::hover());
    if !premium {
        ui.painter().hline(
            bar.x_range(),
            bar.top() + 48.5,
            egui::Stroke::new(1.0, PALETTE.outline),
        );
    }
    let no_lyrics = matches!(app.lyrics.get(&entry.track.video_id), Some(State::Missing));
    let names = [
        (NpTab::UpNext, "UP NEXT"),
        (NpTab::Lyrics, "LYRICS"),
        (NpTab::Related, "RELATED"),
    ];
    // Where each tab is, first, so Premium's glass goes under them.
    let mut x = bar.left();
    let places: Vec<(NpTab, &str, Rect)> = names
        .into_iter()
        .map(|(tab, name)| {
            let width = ui
                .painter()
                .layout_no_wrap(name.to_string(), theme::medium(14.0), PALETTE.text)
                .size()
                .x;
            let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(width + 24.0, 48.0));
            x = rect.right();
            (tab, name, rect)
        })
        .collect();
    if premium && let (Some(first), Some(last)) = (places.first(), places.last()) {
        let capsule = Rect::from_min_max(
            pos2(first.2.left(), bar.top() + 4.0),
            pos2(last.2.right(), bar.top() + 44.0),
        );
        glass(ui.painter(), capsule, 0.05, 0.0);
    }
    let mut line = None;
    for (tab, name, rect) in places {
        let chosen = app.np_tab == tab;
        let enabled = !(tab == NpTab::Lyrics && no_lyrics) || chosen;
        let color = match (chosen, enabled, premium) {
            (true, _, _) => PALETTE.text,
            (false, true, true) => PALETTE.secondary,
            (false, true, false) => Color32::from_white_alpha(143),
            (false, false, true) => PALETTE.disabled,
            (false, false, false) => Color32::from_white_alpha(77),
        };
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.id().with(("tab", name)), sense);
        theme::pointing_or_not(ui, &response, enabled);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, chosen, name)
        });
        let glass_place = Rect::from_min_max(
            pos2(rect.left() + 4.0, bar.top() + 8.0),
            pos2(rect.right() - 4.0, bar.top() + 40.0),
        );
        if premium {
            if chosen {
                glass(ui.painter(), glass_place, 0.06, 0.32);
            } else if response.hovered() && enabled {
                glass(ui.painter(), glass_place, 0.05, 0.0);
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    glass_place,
                    CornerRadius::same(16),
                    egui::Stroke::new(1.0, PALETTE.accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let words = ui
            .painter()
            .layout_no_wrap(name.to_string(), theme::medium(14.0), color);
        let at = rect.center() - words.size() / 2.0;
        ui.painter()
            .galley_with_override_text_color(at, words, color);
        if chosen {
            line = Some(rect.x_range());
        }
        if enabled && response.clicked() {
            app.act(Action::NowPlayingTab(tab));
        }
    }
    if let Some(range) = line.filter(|_| !premium) {
        let ctx = ui.ctx();
        let id = ui.id().with("tab-line");
        let left = ctx.animate_value_with_time(id.with("left"), range.min, 0.15);
        let right = ctx.animate_value_with_time(id.with("right"), range.max, 0.15);
        ui.painter().hline(
            left..=right,
            bar.top() + 47.5,
            egui::Stroke::new(1.0, PALETTE.text),
        );
    }
}

fn lyrics(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.lyrics.get(&entry.track.video_id) {
        None | Some(State::Loading) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(PALETTE.secondary));
            });
        }
        Some(State::Missing) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                theme::label(
                    ui,
                    "Lyrics not available",
                    theme::regular(16.0),
                    PALETTE.secondary,
                );
            });
        }
        Some(State::Ready(lyrics)) => {
            lyric_lines::translate_bar(app, ui, &entry.track.video_id, 16.0);
            lines(app, ui, lyrics, &entry.track.video_id);
        }
    }
}

/// How bright a line is: the one being sung, those already sung, and
/// those still to come (and the words of the one being sung not yet sung).
const LIT: f32 = 1.0;
const SUNG: f32 = 0.34;
const COMING: f32 = 0.5;
/// What is under a line (in Latin letters, in English), as a share of the
/// line's brightness.
const UNDER: f32 = 0.7;
/// Premium's sung lines, a little brighter.
const PREMIUM_SUNG: f32 = 0.40;

fn lines(app: &App, ui: &mut egui::Ui, lyrics: &Lyrics, video_id: &str) {
    let id = ui.id().with(("lyrics", video_id));
    let now = ui.input(|i| i.time);
    // Timed to the song: while its video plays, by YouTube's map.
    let clock = app.lyrics_clock();
    let current = clock.and_then(|position| lyrics.current(position));
    let translated = lyric_lines::translation(app, lyrics, video_id);
    let viewport = ui.available_height();
    // Lines that follow the song move on as it plays.
    // Drawn again when they next look different: often only while a word
    // lights.
    if app.audio_status.entry.is_some()
        && !app.audio_status.paused
        && let Some(wait) = clock.and_then(|position| lyrics.next_change(position))
    {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                wait.max(crate::lyrics::WORD_FRAME),
            ));
    }
    // Scrolling by hand pauses following for a few seconds. Read before
    // the scroll area below takes the wheel's movement for itself.
    let scrolled = ui.input(|i| i.smooth_scroll_delta.y != 0.0);
    if scrolled && ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
        ui.data_mut(|d| d.insert_temp(id.with("manual"), now + 4.0));
    }
    // Line positions from the last frame, to scroll the sung line into
    // view (a third of the way down).
    let tops: Vec<f32> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let manual_until: f64 = ui.data(|d| d.get_temp(id.with("manual"))).unwrap_or(0.0);
    let follow = lyrics.synced && now > manual_until;
    let target = match current {
        Some(i) => tops.get(i).map(|top| (top - viewport * 0.3).max(0.0)),
        None => Some(0.0),
    };
    let offset = lyric_lines::follow(ui, id.with("scroll"), target, follow);

    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = offset {
        area = area.vertical_scroll_offset(offset);
    }
    let drawn = area.show(ui, |ui| {
        let origin = ui.min_rect().top();
        let clip = ui.clip_rect();
        ui.add_space(8.0);
        let mut new_tops = Vec::with_capacity(lyrics.lines.len());
        let width = ui.available_width() - 16.0;
        // Lines that follow the song are large and bold; plain ones are
        // set as YouTube Music sets them (14 on lines 19.6 apart).
        // Premium: larger (32 bold, 26 in less room; plain ones 18 on 27).
        let premium = theme::premium();
        let (font, gap, line_height) = match (lyrics.synced, premium) {
            (true, false) => (theme::bold(24.0), 16.0, None),
            (false, false) => (theme::regular(14.0), 0.0, Some(19.6)),
            (true, true) => (
                theme::bold(if width >= 480.0 { 32.0 } else { 26.0 }),
                24.0,
                None,
            ),
            (false, true) => (theme::regular(18.0), 10.0, Some(27.0)),
        };
        // Under a line, smaller: it in Latin letters, and in English.
        let (small, small_height) = match (lyrics.synced, premium) {
            (true, false) => (theme::regular(16.0), None),
            (false, false) => (theme::regular(13.0), Some(18.2)),
            (true, true) => (
                theme::regular(if width >= 480.0 { 19.0 } else { 16.0 }),
                None,
            ),
            (false, true) => (theme::regular(15.0), Some(22.0)),
        };
        // The edge between a word's sung part and the rest: softer when
        // its time is estimated.
        let feather = font.size * if lyrics.timed_words { 0.5 } else { 0.9 };
        let sung = if premium { PREMIUM_SUNG } else { SUNG };
        for (i, line) in lyrics.lines.iter().enumerate() {
            let goal = match current {
                Some(c) if c == i => LIT,
                Some(c) if i < c => sung,
                _ if lyrics.synced => COMING,
                _ => 1.0,
            };
            let bright = ui
                .ctx()
                .animate_value_with_time(id.with(("line", i)), goal, 0.35);
            let text = if line.text.trim().is_empty() {
                if lyrics.synced { "♪" } else { " " }
            } else {
                line.text.as_str()
            };
            // Premium: lines fade out over the last 56 at the panel's top
            // and foot (where they were placed last frame), not cut off.
            let edge = match tops.get(i) {
                Some(&top) if premium => {
                    let middle = origin + top + font.size * 0.6;
                    let from_top = ((middle - clip.top()) / 56.0).clamp(0.0, 1.0);
                    let from_foot = ((clip.bottom() - middle) / 56.0).clamp(0.0, 1.0);
                    from_top.min(from_foot)
                }
                _ => 1.0,
            };
            let shade = |share: f32| Color32::from_white_alpha((share * edge * 255.0) as u8);
            let color = shade(bright);
            let galley = lyric_lines::layout(ui, text, &font, width, line_height);
            let under: Vec<_> = lyric_lines::under(translated, i)
                .map(|words| lyric_lines::layout(ui, words, &small, width, small_height))
                .collect();
            let size = under.iter().fold(galley.size(), |size, line| {
                vec2(
                    size.x.max(line.size().x),
                    size.y + lyric_lines::UNDER_GAP + line.size().y,
                )
            });
            let sense = if line.start.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(size, sense);
            super::lyric_lines::name(&response, text, line.start.is_some());
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                match clock {
                    // The line being sung, word by word.
                    Some(position) if current == Some(i) && !line.words.is_empty() => {
                        let unsung = shade(bright.min(COMING));
                        let lit = lyric_lines::sung(
                            &galley,
                            &line.words,
                            position,
                            color,
                            unsung,
                            feather,
                        );
                        painter.galley(rect.min, lit, color);
                    }
                    _ => painter.galley_with_override_text_color(rect.min, galley.clone(), color),
                }
                let mut y = rect.top() + galley.size().y;
                for line in under {
                    y += lyric_lines::UNDER_GAP;
                    let height = line.size().y;
                    painter.galley_with_override_text_color(
                        pos2(rect.left(), y),
                        line,
                        shade(bright * UNDER),
                    );
                    y += height;
                }
            }
            new_tops.push(rect.top() - origin);
            if let Some(start) = line.start {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    app.act(Action::Seek(app.lyrics_moment(start)));
                    ui.data_mut(|d| d.insert_temp(id.with("manual"), 0.0f64));
                }
            }
            ui.add_space(gap);
        }
        ui.add_space(16.0);
        theme::label(
            ui,
            &format!("Source: {}", lyrics.source),
            theme::regular(14.0),
            PALETTE.dim,
        );
        ui.add_space(if lyrics.synced { viewport * 0.6 } else { 32.0 });
        ui.data_mut(|d| d.insert_temp(id, new_tops));
    });
    lyric_lines::drawn(ui, id.with("scroll"), drawn.state.offset.y);
}

fn related(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.related.get(&entry.track.video_id) {
        Some(crate::app::Loadable::Ready(found)) => {
            // Songs played from here start a queue of their own, not one
            // of the playlist open behind the player page.
            let route = crate::backend::Route::Browse {
                id: format!("related-{}", entry.track.video_id),
                params: None,
            };
            egui::ScrollArea::vertical()
                .id_salt(("related", &entry.track.video_id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    page::sections(app, ui, &route, found, &page::Look::PANEL);
                    ui.add_space(32.0);
                });
        }
        Some(crate::app::Loadable::Failed(message)) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                theme::label(ui, message, theme::regular(16.0), PALETTE.secondary);
            });
        }
        _ => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(PALETTE.secondary));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switch stays at the top of the room left of the panel, at the
    /// same height whatever the picture (a square cover or a 16:9 video, in
    /// a window of any size), and the picture is centred under it, inside
    /// the page and clear of the panel.
    #[test]
    fn the_switch_stays_at_the_top_and_the_picture_is_centred_under_it() {
        for (width, height) in [
            (1040.0, 684.0),
            (1467.0, 883.0),
            (720.0, 476.0),
            (2320.0, 1250.0),
        ] {
            let area = Rect::from_min_size(pos2(240.0, 64.0), vec2(width, height));
            for fullscreen in [false, true] {
                let places = places(width + 240.0, area, fullscreen);
                let cover = cover_in(places.room, 800.0);
                let video = video_in(places.room);
                for shown in [cover, video] {
                    assert!(area.contains_rect(shown), "{width}");
                    assert!(shown.right() + 24.0 <= places.panel.left(), "{width}");
                    assert!(shown.top() >= places.switch.bottom() + 16.0 - 0.01);
                    assert!((shown.center().x - places.switch.center().x).abs() < 0.01);
                    assert!((shown.center().y - places.room.center().y).abs() < 0.01);
                }
                assert!((video.width() / video.height() - 16.0 / 9.0).abs() < 0.01);
                // The same switch for both.
                let under = places.room.top() - video::SWITCH_ROOM;
                assert!((places.switch.top() - under).abs() < 0.01);
            }
        }
        // At 1280 by 820 (the page 1040 by 684 from 240, 64): the switch 32
        // under the top bar, the cover 537.9 where it always was.
        let page = Rect::from_min_size(pos2(240.0, 64.0), vec2(1040.0, 684.0));
        let places = places(1280.0, page, false);
        assert!((places.switch.top() - 96.0).abs() < 0.01);
        let cover = cover_in(places.room, 800.0);
        assert!((cover.width() - 537.92).abs() < 0.01, "{cover:?}");
        assert!((cover.top() - 163.04).abs() < 0.01, "{cover:?}");
    }

    /// In full screen, at a laptop's and larger screens' sizes: the cover
    /// takes most of the height (more than 0.8 of it), the panel keeps 400
    /// to 720, both inside, apart.
    #[test]
    fn full_screen_gives_the_cover_the_room_and_keeps_the_panel() {
        for (width, height) in [
            (1280.0, 748.0),
            (1707.0, 995.0),
            (1920.0, 1008.0),
            (2560.0, 1368.0),
            (3440.0, 1368.0),
        ] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height));
            let (main, panel) = fullscreen_places(area);
            assert!(area.contains_rect(main) && area.contains_rect(panel));
            assert!(main.right() + 48.0 <= panel.left(), "{width}");
            assert!((400.0..=720.0).contains(&panel.width()), "{width}");
            let cover = cover_in(places(width, area, true).room, f32::INFINITY);
            assert!(
                cover.width() > 0.8 * height,
                "{width} by {height}: {cover:?}"
            );
        }
    }
}
