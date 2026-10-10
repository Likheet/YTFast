//! The player page, laid out as YouTube Music's: the playing song's cover
//! large on the left, and on the right a panel with the tabs Up next,
//! Lyrics and Related, on the window's own near-black. Lyrics follow the
//! song in the Even Better Lyrics Plus way: the line being sung lit, the
//! rest dimmed, scrolling smoothly; click a line to jump there.
//!
//! Premium: the cover (at most 480, r 16) with the song's name and artist
//! centred under it, and beside it the tabs in clear glass and what they
//! show, straight on the song's own colours (no box round them), as
//! YouTube Music sets a playlist's songs beside its cover; the lyrics
//! larger.
//!
//! Over the cover, in every look, the Song and Video switch; in the video
//! mode the music video takes the cover's place (`views::video`).

use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2,
};

use crate::app::{Action, App, NpTab, PlayState};
use crate::lyrics::{Lyrics, State};
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE};
use crate::views::{page, queue_panel, video, widgets};

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

    let panel = if theme::premium() {
        premium_cover(app, ui, area, entry)
    } else {
        youtube_music_cover(app, ui, area, entry)
    };

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
    if theme::premium() {
        premium_tabs(app, &mut ui, entry);
    } else {
        tabs(app, &mut ui, entry);
    }
    match app.np_tab {
        NpTab::UpNext => queue_panel::list(app, &mut ui),
        NpTab::Lyrics => lyrics(app, &mut ui, entry),
        NpTab::Related => related(app, &mut ui, entry),
    }
}

/// YouTube Music's cover, large on the left. Returns where the panel goes.
fn youtube_music_cover(app: &App, ui: &egui::Ui, area: Rect, entry: &Entry) -> Rect {
    if app.fullscreen {
        let (main, panel) = fullscreen_places(area);
        return youtube_music_picture(app, ui, main, entry, f32::INFINITY, panel);
    }
    // YouTube Music's spacing, by the window's width: above, at the sides,
    // between the cover and the panel, and the panel's share of the rest
    // (at most 800).
    let window = ui.ctx().content_rect().width();
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
    // The Song and Video switch over the cover (square, at most 800), the
    // two centred in what is left (with the space above kept under it
    // too); `#606060` while the cover loads. In the video mode the video
    // takes its place, 16:9, as large as the room allows and with square
    // corners (YouTube Music's `#player` keeps its 800 and its r 8 for the
    // cover: `:not([video-mode])`).
    let main = Rect::from_min_max(inner.min, pos2(panel.left() - gap, inner.bottom() - top));
    youtube_music_picture(app, ui, main, entry, 800.0, panel)
}

/// YouTube Music's cover (square, at most `largest`) or video in `main`,
/// with the Song and Video switch over it. Returns `panel`.
fn youtube_music_picture(
    app: &App,
    ui: &egui::Ui,
    main: Rect,
    entry: &Entry,
    largest: f32,
    panel: Rect,
) -> Rect {
    let corners = CornerRadius::same(8);
    if video::shown(app) {
        let size = video::size_in(main, f32::INFINITY, f32::INFINITY);
        let (row, place) = video::stack(main, size);
        video::switch(app, ui, row);
        video::paint(app, ui, place, CornerRadius::ZERO);
        cover_click(app, ui, place);
        return panel;
    }
    let length = main
        .width()
        .min(main.height() - video::SWITCH_ROOM)
        .clamp(0.0, largest);
    let (row, art) = video::stack(main, Vec2::splat(length));
    video::switch(app, ui, row);
    ui.painter().rect_filled(art, corners, PALETTE.thumb);
    widgets::picture(app, ui, art, entry.track.thumbnail.as_ref(), corners);
    cover_click(app, ui, art);
    video::note(app, ui, art);
    panel
}

/// Premium's cover (at most 480, r 16; larger on a large screen) and the
/// song's name (26 bold) and artist (16) centred under it, a little larger
/// with a larger cover (at most 34 and 20), over the song's colours
/// (`backdrop::listening`, painted in `views::show`). Returns where the
/// panel goes.
fn premium_cover(app: &App, ui: &mut egui::Ui, area: Rect, entry: &Entry) -> Rect {
    let Listening {
        art,
        video: place,
        panel,
    } = if app.fullscreen {
        fullscreen_listening(area)
    } else {
        listening_layout(area)
    };
    let grow = ((art.width() - 480.0) / 420.0).clamp(0.0, 1.0);
    let corners = CornerRadius::same(16);
    let shown = if video::shown(app) {
        video::paint(app, ui, place, corners);
        place
    } else {
        widgets::cover_with(app, ui, art, entry.track.thumbnail.as_ref(), corners);
        video::note(app, ui, art);
        art
    };
    cover_click(app, ui, shown);
    video::switch(app, ui, video::row_above(shown));
    let words = Rect::from_min_size(
        pos2(shown.left(), shown.bottom() + 24.0),
        vec2(shown.width(), 80.0),
    );
    let mut details = ui.new_child(
        UiBuilder::new()
            .max_rect(words)
            .layout(Layout::top_down(Align::Center)),
    );
    theme::label(
        &mut details,
        &entry.track.title,
        theme::bold(26.0 + 8.0 * grow),
        PALETTE.text,
    );
    details.add_space(6.0);
    theme::label(
        &mut details,
        &entry.track.artists,
        theme::regular(16.0 + 4.0 * grow),
        PALETTE.secondary,
    );
    panel
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

/// Premium in full screen: [`fullscreen_places`], the cover or video as
/// large as the room allows with the switch over it and the song's words
/// under it.
fn fullscreen_listening(area: Rect) -> Listening {
    let (main, panel) = fullscreen_places(area);
    // Above, the switch; below, 88 of words (and 16 to spare).
    let room = (main.height() - 104.0 - video::SWITCH_ROOM).max(0.0);
    let side = main.width().min(room);
    let wide = main.width().min(room * 16.0 / 9.0);
    let centred = |size: Vec2| {
        Rect::from_min_size(
            pos2(
                main.center().x - size.x / 2.0,
                main.center().y - (size.y + 88.0 + video::SWITCH_ROOM) / 2.0 + video::SWITCH_ROOM,
            ),
            size,
        )
    };
    Listening {
        art: centred(Vec2::splat(side)),
        video: centred(vec2(wide, wide * 9.0 / 16.0)),
        panel,
    }
}

/// Premium's places on the player page.
struct Listening {
    /// The cover, and the video in its place in the video mode.
    art: Rect,
    video: Rect,
    panel: Rect,
}

/// Premium's places, 32 in at the sides and 24 above and below: the panel
/// the whole height at the right (52% of the room, at most 680), and the
/// cover (at most 480) with the Song and Video switch over it and its
/// words under it, centred in the rest, 48 before it (32 in less room),
/// so the two share a middle line. The video, in the cover's place, is
/// 16:9 and as wide as that room (at most 1280).
///
/// Up to a page 1520 wide (a laptop's) the room is at most 1280, centred.
/// A larger page (a large screen's) keeps margins of 120 and gives the
/// rest to what it shows, which would otherwise huddle in the middle: for
/// each point past 1280, the panel grows a quarter (to at most 960), the
/// cover a quarter (to at most 900, as the height allows) and the space
/// between them 0.15 (to at most 160).
fn listening_layout(area: Rect) -> Listening {
    let margin = ((area.width() - 1280.0) / 2.0).clamp(32.0, 120.0);
    let width = (area.width() - 2.0 * margin).max(0.0);
    let extra = (width - 1280.0).max(0.0);
    let inner = Rect::from_center_size(area.center(), vec2(width, (area.height() - 48.0).max(0.0)));
    let gap = if width >= 900.0 {
        (48.0 + extra * 0.15).min(160.0)
    } else {
        32.0
    };
    let panel_width = (width * 0.52).min((680.0 + extra * 0.25).min(960.0));
    let panel = Rect::from_min_max(pos2(inner.right() - panel_width, inner.top()), inner.max);
    let main = Rect::from_min_max(
        inner.min,
        pos2((panel.left() - gap).max(inner.left()), inner.bottom()),
    );
    // Above, the switch; below, 88 of words (and 16 to spare).
    let room = (main.height() - 104.0 - video::SWITCH_ROOM).max(0.0);
    let side = main
        .width()
        .min(room)
        .min((480.0 + extra * 0.25).min(900.0));
    let wide = main.width().min(room * 16.0 / 9.0).min(1280.0);
    let centred = |size: Vec2| {
        Rect::from_min_size(
            pos2(
                main.center().x - size.x / 2.0,
                main.center().y - (size.y + 88.0 + video::SWITCH_ROOM) / 2.0 + video::SWITCH_ROOM,
            ),
            size,
        )
    };
    Listening {
        art: centred(Vec2::splat(side)),
        video: centred(vec2(wide, wide * 9.0 / 16.0)),
        panel,
    }
}

/// Premium's tabs, Up next, Lyrics and Related, in clear glass as an
/// iPhone draws it: a capsule 44 high (480 wide, or 60% of a wider panel's
/// width up to 560, centred over the list) barely filled and with no edge;
/// the chosen tab a clear capsule inside it with a single rim of light;
/// the one under the pointer barely filled.
fn premium_tabs(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let wide = (row.width() * 0.6).clamp(480.0, 560.0);
    let bar = Rect::from_center_size(row.center(), vec2(row.width().min(wide), 44.0));
    glass(ui.painter(), bar, 0.05, 0.0);
    let no_lyrics = matches!(app.lyrics.get(&entry.track.video_id), Some(State::Missing));
    let tabs = [
        (NpTab::UpNext, "UP NEXT", "Up next"),
        (NpTab::Lyrics, "LYRICS", "Lyrics"),
        (NpTab::Related, "RELATED", "Related"),
    ];
    let width = (bar.width() - 8.0) / 3.0;
    for (index, (tab, name, label)) in tabs.into_iter().enumerate() {
        let chosen = app.np_tab == tab;
        let enabled = !(tab == NpTab::Lyrics && no_lyrics) || chosen;
        let color = if chosen {
            PALETTE.text
        } else if enabled {
            PALETTE.secondary
        } else {
            PALETTE.disabled
        };
        let rect = Rect::from_min_size(
            pos2(bar.left() + 4.0 + index as f32 * width, bar.top() + 4.0),
            vec2(width, 36.0),
        );
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
        if chosen {
            glass(ui.painter(), rect, 0.06, 0.32);
        } else if response.hovered() && enabled {
            glass(ui.painter(), rect, 0.05, 0.0);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(18),
                egui::Stroke::new(1.0, PALETTE.accent),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::medium(14.0),
            color,
        );
        if enabled && response.clicked() {
            app.act(Action::NowPlayingTab(tab));
        }
    }
    ui.add_space(16.0);
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
/// hairline, the chosen one white with a white line 1 thick under it that
/// slides to the tab chosen (0.15 s); the others white@0.56 (0.70 seen at
/// 0.8), LYRICS at 0.30 and not to be chosen when the song has none.
fn tabs(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 49.0), Sense::hover());
    ui.painter().hline(
        bar.x_range(),
        bar.top() + 48.5,
        egui::Stroke::new(1.0, PALETTE.outline),
    );
    let no_lyrics = matches!(app.lyrics.get(&entry.track.video_id), Some(State::Missing));
    let mut x = bar.left();
    let mut line = None;
    for (tab, name) in [
        (NpTab::UpNext, "UP NEXT"),
        (NpTab::Lyrics, "LYRICS"),
        (NpTab::Related, "RELATED"),
    ] {
        let chosen = app.np_tab == tab;
        let enabled = !(tab == NpTab::Lyrics && no_lyrics) || chosen;
        let color = if chosen {
            PALETTE.text
        } else if enabled {
            Color32::from_white_alpha(143)
        } else {
            Color32::from_white_alpha(77)
        };
        let words = ui
            .painter()
            .layout_no_wrap(name.to_string(), theme::medium(14.0), color);
        let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(words.size().x + 24.0, 48.0));
        x = rect.right();
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
    if let Some(range) = line {
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
        Some(State::Ready(lyrics)) => lines(app, ui, lyrics, &entry.track.video_id),
    }
}

/// How bright a line is: the one being sung, those already sung, and
/// those still to come.
const LIT: f32 = 1.0;
const SUNG: f32 = 0.34;
const COMING: f32 = 0.5;
/// Premium's sung lines, a little brighter.
const PREMIUM_SUNG: f32 = 0.40;

fn lines(app: &App, ui: &mut egui::Ui, lyrics: &Lyrics, video_id: &str) {
    let id = ui.id().with(("lyrics", video_id));
    let now = ui.input(|i| i.time);
    // Timed to the song: while its video plays, by YouTube's map.
    let current = app
        .lyrics_clock()
        .and_then(|position| lyrics.current(position));
    let viewport = ui.available_height();
    // Lines that follow the song move on as it plays.
    if lyrics.synced && app.audio_status.entry.is_some() && !app.audio_status.paused {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
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
    let target = current
        .and_then(|i| tops.get(i))
        .map_or(0.0, |top| (top - viewport * 0.3).max(0.0));
    let offset = ui
        .ctx()
        .animate_value_with_time(id.with("scroll"), target, 0.55);

    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if follow {
        area = area.vertical_scroll_offset(offset);
    }
    area.show(ui, |ui| {
        let origin = ui.min_rect().top();
        let clip = ui.clip_rect();
        ui.add_space(24.0);
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
            let color = Color32::from_white_alpha((bright * edge * 255.0) as u8);
            ui.set_max_width(width);
            let response = ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .font(font.clone())
                        .color(color)
                        .line_height(line_height),
                )
                .wrap()
                .selectable(false)
                .sense(if line.start.is_some() {
                    Sense::click()
                } else {
                    Sense::hover()
                }),
            );
            new_tops.push(response.rect.top() - origin);
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
                    // Premium: the songs in as many columns as fit.
                    if theme::premium() {
                        let grid =
                            theme::Grid::new(ui.ctx().content_rect().width(), ui.available_width());
                        let look = page::Look {
                            column: grid.song_column(ui.available_width()),
                            column_gap: grid.song_column_gap(),
                            ..page::Look::PANEL
                        };
                        page::sections(app, ui, &route, found, &look);
                    } else {
                        page::sections(app, ui, &route, found, &page::Look::PANEL);
                    }
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

    #[test]
    fn premium_player_page_keeps_cover_and_words_inside_the_window() {
        for (width, height) in [
            (700.0, 400.0),
            (1040.0, 652.0),
            (1487.0, 851.0),
            (1680.0, 880.0),
            (2320.0, 1250.0),
            (3200.0, 1250.0),
            (3600.0, 2000.0),
        ] {
            let area = Rect::from_min_size(pos2(220.0, 72.0), vec2(width, height));
            let Listening { art, video, panel } = listening_layout(area);
            assert!(area.contains_rect(art));
            assert!(area.contains_rect(panel));
            assert!(art.right() + 24.0 <= panel.left());
            assert!(art.width() <= 900.0);
            assert!(art.bottom() + 80.0 <= area.bottom());
            assert!(panel.width() >= art.width());
            assert!(panel.width() <= 960.0);
            // The switch over them, the words under them.
            for shown in [art, video] {
                assert!(shown.top() - video::SWITCH_ROOM >= area.top(), "{width}");
                assert!(shown.bottom() + 80.0 <= area.bottom(), "{width}");
            }
            assert!(video.right() + 24.0 <= panel.left());
            assert!((video.width() / video.height().max(0.01) - 16.0 / 9.0).abs() < 0.01);
        }
    }

    /// In full screen, at a laptop's and larger screens' sizes: the cover
    /// takes most of the height (more than 0.8 of it; Premium's, with its
    /// words under it, 0.65), the panel keeps 400 to 720, both inside,
    /// apart.
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
            let side = main.width().min(main.height() - video::SWITCH_ROOM);
            assert!(side > 0.8 * height, "{width} by {height}: {side}");
            // Premium's, with its words under the cover, inside too.
            let Listening { art, video, panel } = fullscreen_listening(area);
            assert!(art.top() - video::SWITCH_ROOM >= area.top());
            assert!(art.bottom() + 80.0 <= area.bottom());
            assert!(video.right() <= panel.left() - 48.0);
            // (Its words under it take some of the height.)
            assert!(art.width() > 0.65 * height, "{width} by {height}");
        }
    }

    /// A laptop's page is laid out as before: the room at most 1280,
    /// centred, the cover 480 and the panel 52% of the room.
    #[test]
    fn premium_player_page_is_unchanged_on_a_laptop() {
        for width in [1344.0, 1467.0, 1520.0] {
            let area = Rect::from_min_size(pos2(240.0, 64.0), vec2(width, 851.0));
            let Listening { art, panel, .. } = listening_layout(area);
            let margin = (width - 1280.0) / 2.0;
            assert!(
                (area.right() - panel.right() - margin).abs() < 0.01,
                "{width}"
            );
            assert!((panel.width() - 1280.0 * 0.52).abs() < 0.01, "{width}");
            assert_eq!(art.width(), 480.0, "{width}");
        }
    }

    /// On a large screen what the page shows spreads out instead of
    /// keeping to the middle: margins of 120, a larger cover and panel, and
    /// more room between them.
    #[test]
    fn premium_player_page_spreads_out_on_a_large_screen() {
        // A 2560 wide screen, the menu open.
        let area = Rect::from_min_size(pos2(240.0, 64.0), vec2(2320.0, 1250.0));
        let Listening { art, panel, .. } = listening_layout(area);
        assert!((area.right() - panel.right() - 120.0).abs() < 0.01);
        assert!((panel.width() - 880.0).abs() < 0.01);
        assert!((art.width() - 680.0).abs() < 0.01);
        // The cover centred in the room left of the panel, 160 before it.
        let main_right = panel.left() - 160.0;
        let main_left = area.left() + 120.0;
        assert!(((art.center().x - (main_left + main_right) / 2.0).abs()) < 0.01);
    }
}
