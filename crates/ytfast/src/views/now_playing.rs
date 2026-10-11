//! The player page, laid out as YouTube Music's: the playing song's cover
//! large on the left, and on the right a panel with the tabs Up next,
//! Lyrics and Related, on the window's own near-black. Lyrics follow the
//! song in the Even Better Lyrics Plus way: the line being sung lit word
//! by word as it is sung, the rest dimmed, scrolling smoothly; click a
//! line to jump there. Translate shows each line in Latin letters and in
//! English under it.
//!
//! YouTube Music's look and Dynamic Background put their parts in the same
//! places ([`places`]): the Song and Video switch at the top, at one
//! height, as YouTube Music's, and the cover or the video centred under
//! it.
//!
//! Premium: the cover (at most 480, r 16) with the song's name and artist
//! centred under it, and beside it the tabs in clear glass and what they
//! show, straight on the song's own colours (no box round them), as
//! YouTube Music sets a playlist's songs beside its cover; the lyrics
//! larger. Its switch stays at one height too, over the cover.

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

    let panel = if theme::premium() {
        premium_cover(app, ui, area, entry)
    } else {
        let places = places(ui.ctx().content_rect().width(), area, app.fullscreen);
        picture(app, ui, &places, entry);
        places.panel
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

/// The player page's places in YouTube Music's look and Dynamic
/// Background (they differ only in how they draw them).
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
/// cover: `:not([video-mode])`). A fan's video, which has only its still,
/// shows it whole in the cover's square, 16:9.
fn picture(app: &App, ui: &egui::Ui, places: &Places, entry: &Entry) {
    video::switch(app, ui, places.switch);
    if video::shown(app) {
        let place = video_in(places.room);
        video::paint(app, ui, place, CornerRadius::ZERO);
        cover_click(app, ui, place);
        return;
    }
    let largest = if app.fullscreen { f32::INFINITY } else { 800.0 };
    let art = cover_in(places.room, largest);
    let corners = CornerRadius::same(8);
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

/// Premium's cover (at most 480, r 16; larger on a large screen) and the
/// song's name (26 bold) and artist (16) centred under it, a little larger
/// with a larger cover (at most 34 and 20), over the song's colours
/// (`backdrop::listening`, painted in `views::show`). The Song and Video
/// switch stays at one height over them, over the taller of the cover's
/// place and the video's, whichever shows; a fan's video shows its still
/// whole in the cover's square. Returns where the panel goes.
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
        widgets::big_cover(app, ui, art, entry.track.thumbnail.as_ref(), corners);
        video::note(app, ui, art);
        art
    };
    cover_click(app, ui, shown);
    let highest = if art.top() <= place.top() { art } else { place };
    video::switch(app, ui, video::row_above(highest));
    // The words under what shows: a fan's video's still is wide in the
    // cover's square.
    let visible = if video::shown(app) {
        place
    } else {
        widgets::shape_in(art, entry.track.thumbnail.as_ref())
    };
    let width = art.width().max(visible.width());
    let words = Rect::from_min_size(
        pos2(art.center().x - width / 2.0, visible.bottom() + 24.0),
        vec2(width, 80.0),
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
        theme::repaint_in(
            ui.ctx(),
            std::time::Duration::from_secs_f64(wait.max(crate::lyrics::WORD_FRAME)),
        );
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
            let bright = lyric_lines::ease(ui.ctx(), id.with(("line", i)), goal, 0.35);
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

    /// Premium's Song and Video switch stays at one height whether the
    /// cover or the video shows: over the taller of the two, both centred
    /// on one middle, and inside the page.
    #[test]
    fn premium_switch_stays_at_one_height() {
        for (width, height) in [(1040.0, 652.0), (1487.0, 851.0), (2320.0, 1250.0)] {
            let area = Rect::from_min_size(pos2(220.0, 72.0), vec2(width, height));
            for Listening { art, video, .. } in [listening_layout(area), fullscreen_listening(area)]
            {
                assert!((art.center().y - video.center().y).abs() < 0.01, "{width}");
                assert!((art.center().x - video.center().x).abs() < 0.01, "{width}");
                let highest = art.top().min(video.top());
                assert!(highest - video::SWITCH_ROOM >= area.top() - 0.01, "{width}");
            }
        }
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
