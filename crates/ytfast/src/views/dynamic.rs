//! The Dynamic Background theme's own screens and sizes (see
//! `crate::dynamic` for the background, the glass and the buttons): the
//! player page with Better Lyrics' lyrics as the theme sets them, toasts,
//! and the rows, covers and buttons that differ from YouTube Music's.
//! Everything is measured from music.youtube.com wearing the theme, 1280
//! wide.

use egui::{
    Align2, Color32, CornerRadius, Layout, Rect, Sense, Shape, TextureId, UiBuilder, Vec2, pos2,
    vec2,
};

use crate::app::{Action, App, NpTab, PlayState};
use crate::dynamic::{self, SHADOW};
use crate::lyrics::{Lyrics, State};
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE};
use crate::views::widgets::Row;
use crate::views::{page, queue_panel, video, widgets};

/// The theme's song rows: an album's and a playlist's 56 high, the cover
/// (or the number) 38 with 24 after it; an artist's top songs 54 high;
/// Up next 58 high and 8 apart, covers 42; no hairlines between rows
/// (`--ytmusic-list-item-height: 54px`, measured with the theme on).
pub fn row(row: Row) -> Row {
    if row == Row::LIST || row == Row::PLAYLIST {
        Row {
            height: 56.0,
            gap: 0.0,
            art: 38.0,
            corner: 4,
            art_gap: 24.0,
            pad: 8.0,
            text_gap: 4.0,
            ..row
        }
    } else if row == Row::SHELF {
        Row {
            height: 54.0,
            gap: 0.0,
            art: 38.0,
            line: false,
            ..row
        }
    } else if row == Row::QUEUE {
        Row {
            height: 58.0,
            gap: 8.0,
            art: 42.0,
            corner: 4,
            line: false,
            ..row
        }
    } else if row == Row::SEARCH_ONLY {
        Row { line: false, ..row }
    } else {
        row
    }
}

/// A cover's corners by its width: cards and large covers 20
/// (`--radius-art-lg`), middling ones 12, small ones 4; artists round.
pub fn cover_radius(width: f32, round: bool) -> CornerRadius {
    if round {
        CornerRadius::same(255)
    } else if width > 200.0 {
        CornerRadius::same(dynamic::RADIUS_ART_LG)
    } else if width > 64.0 {
        CornerRadius::same(12)
    } else {
        CornerRadius::same(4)
    }
}

/// The shade over a card's cover under the pointer (and on the album or
/// playlist playing): black at 50% fading to nothing 80% down
/// (`linear-gradient(rgba(0,0,0,.5), transparent 80%)`), its top corners
/// rounded as the cover's are, so nothing shows past them.
pub fn card_shade(ui: &egui::Ui, art: Rect, shade: f32) {
    let radius = f32::from(dynamic::RADIUS_ART_LG).min(art.width() / 2.0);
    let bottom = art.top() + art.height() * 0.8;
    let dark = |y: f32| {
        let left = 1.0 - ((y - art.top()) / (bottom - art.top())).clamp(0.0, 1.0);
        Color32::from_black_alpha((128.0 * shade * left) as u8)
    };
    // Its outline, clockwise from the foot of the left side: round the top
    // left corner, across, round the top right corner, down.
    let corner = |centre: egui::Pos2, from: f32| {
        (0..=8).map(move |i| {
            let angle = from + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
            centre + vec2(angle.cos(), angle.sin()) * radius
        })
    };
    let mut outline = vec![pos2(art.left(), bottom)];
    outline.extend(corner(
        pos2(art.left() + radius, art.top() + radius),
        std::f32::consts::PI,
    ));
    outline.extend(corner(
        pos2(art.right() - radius, art.top() + radius),
        1.5 * std::f32::consts::PI,
    ));
    outline.push(pos2(art.right(), bottom));
    // A fan from its middle: the shade changes only down, so each
    // triangle's corners give it exactly.
    let middle = pos2(art.center().x, (art.top() + bottom) / 2.0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(middle, dark(middle.y));
    for point in &outline {
        mesh.colored_vertex(*point, dark(point.y));
    }
    let last = outline.len() as u32;
    for i in 1..last {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh.add_triangle(0, last, 1);
    ui.painter().add(Shape::mesh(mesh));
}

/// A square button with × (back to all results, or all of the library):
/// white@0.20 glass, corners 12, a white ×.
pub fn close_square(ui: &egui::Ui, rect: Rect, hovered: bool) {
    let fill = if hovered {
        Color32::from_white_alpha(77)
    } else {
        PALETTE.surface_hover
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(dynamic::RADIUS_BTN), fill);
    theme::paint_icon(ui, Icon::Close, rect, 24.0, PALETTE.text);
}

/// A shelf's arrow: white@0.10, corners 16 (white@0.20 under the pointer);
/// one that cannot turn at 40%.
pub fn shelf_arrow(ui: &egui::Ui, rect: Rect, icon: Icon, hovered: bool, enabled: bool) {
    let fill = if hovered {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    let dim = if enabled { 1.0 } else { 0.4 };
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(dynamic::RADIUS_BTN_ALT),
        fill.gamma_multiply(dim),
    );
    theme::paint_icon(ui, icon, rect, 18.7, PALETTE.text.gamma_multiply(dim));
}

/// Explore's big buttons (New releases, Charts, Moods & genres): white@0.10
/// glass, corners 16, white@0.20 under the pointer, the icon white.
pub fn big_button_look(ui: &egui::Ui, rect: Rect, hover: f32) -> Color32 {
    let fill = dynamic::lerp(PALETTE.surface, PALETTE.surface_hover, hover);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(dynamic::RADIUS_BTN_ALT), fill);
    PALETTE.text
}

/// A mood's button: white@0.10 glass (white@0.20 under the pointer),
/// corners 12, the mood's colour a stripe 6 wide at its left.
pub fn mood_button_look(ui: &egui::Ui, rect: Rect, stripe: Color32, hovered: bool) -> Rect {
    let radius = dynamic::RADIUS_BTN;
    let fill = if hovered {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), fill);
    let band = Rect::from_min_max(rect.min, pos2(rect.left() + 6.0, rect.bottom()));
    ui.painter().rect_filled(
        band,
        CornerRadius {
            nw: radius,
            sw: radius,
            ne: 0,
            se: 0,
        },
        stripe,
    );
    Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), rect.max)
}

/// A row's tick box: white@0.10, corners 4; ticked, white@0.20 with a
/// white tick.
pub fn tick_box_look(ui: &egui::Ui, square: Rect, ticked: bool) {
    let fill = if ticked {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    ui.painter()
        .rect_filled(square, CornerRadius::same(4), fill);
    if ticked {
        theme::paint_icon(ui, Icon::Check, square, 16.0, PALETTE.text);
    }
}

/// Search's top result: two panes of glass, corners 24, 8 apart (the card
/// and its songs); its picture corners 16.
pub fn top_result_panes(ui: &egui::Ui, card: Rect, songs: Option<Rect>) {
    let corners = CornerRadius::same(dynamic::RADIUS_PANEL_LG);
    ui.painter().rect_filled(card, corners, PALETTE.surface);
    if let Some(songs) = songs {
        let songs = Rect::from_min_max(pos2(songs.left() + 8.0, songs.top()), songs.max);
        ui.painter()
            .rect_filled(songs, corners, Color32::from_white_alpha(13));
    }
}

/// Behind an artist's header: the picture at 80% (and a little darker),
/// fading out over its last 40% into the background behind
/// (`mask-image: linear-gradient(black 60%, transparent)`); no fade to
/// black under it.
pub fn artist_picture(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    frame: Rect,
    texture: Option<(TextureId, [usize; 2])>,
) {
    let mut shapes = Vec::new();
    if let Some((texture, [w, h])) = texture {
        let picture = w as f32 / h.max(1) as f32;
        let shape = frame.width() / frame.height().max(1.0);
        let uv = if picture > shape {
            let keep = shape / picture;
            Rect::from_min_max(pos2((1.0 - keep) / 2.0, 0.0), pos2((1.0 + keep) / 2.0, 1.0))
        } else {
            Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, picture / shape))
        };
        // Four rows of corners: solid to 60% down, then fading out.
        let mut mesh = egui::Mesh::with_texture(texture);
        let tone = |alpha: f32| {
            let a = (alpha * 0.8 * 255.0) as u8;
            // `brightness(0.95)` with the opacity, premultiplied.
            let c = (f32::from(a) * 0.95) as u8;
            Color32::from_rgba_premultiplied(c, c, c, a)
        };
        for (row, (down, alpha)) in [(0.0, 1.0), (0.6, 1.0), (1.0, 0.0)].into_iter().enumerate() {
            let y = frame.top() + frame.height() * down;
            let v = uv.top() + uv.height() * down;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(frame.left(), y),
                uv: pos2(uv.left(), v),
                color: tone(alpha),
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(frame.right(), y),
                uv: pos2(uv.right(), v),
                color: tone(alpha),
            });
            if row > 0 {
                let base = (row as u32 - 1) * 2;
                mesh.add_triangle(base, base + 1, base + 3);
                mesh.add_triangle(base, base + 3, base + 2);
            }
        }
        shapes.push(Shape::mesh(mesh));
    }
    ui.ctx()
        .layer_painter(egui::LayerId::background())
        .set(slot, Shape::Vec(shapes));
}

/// A short message ("Added to the queue") at the bottom left, just above
/// the player bar: glass, corners 12, the theme's shadow, white words
/// (`tp-yt-paper-toast` in the theme), coming and going as YouTube Music's.
pub fn notice(app: &App, ui: &egui::Ui) {
    let Some((text, at)) = &app.notice else {
        return;
    };
    let elapsed = at.elapsed().as_secs_f32();
    let coming = (elapsed / 0.3).min(1.0);
    let going = ((elapsed - 3.0) / 0.3).clamp(0.0, 1.0);
    if elapsed < 0.3 || elapsed > 3.0 {
        ui.ctx().request_repaint();
    } else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(3.0 - elapsed));
    }
    let shown = theme::bezier(0.25, 0.1, 0.25, 1.0, coming) * (1.0 - going);
    if shown <= 0.0 {
        return;
    }
    let lift = if app.playback.entry.is_some() {
        theme::player_bar_height()
    } else {
        12.0
    };
    let drop = 100.0 * (1.0 - shown);
    egui::Area::new(egui::Id::new("notice"))
        .order(egui::Order::Foreground)
        .anchor(Align2::LEFT_BOTTOM, vec2(12.0, -lift + drop))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            ui.set_opacity(shown);
            let widest = (ui.ctx().content_rect().width() - 24.0 - 48.0).max(240.0);
            let words = theme::fit(ui, text, theme::regular(14.0), PALETTE.text, widest, 3);
            let width = words.size().x.max(240.0);
            let size = vec2(width + 48.0, words.size().y.max(16.8) + 32.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let corners = CornerRadius::same(dynamic::RADIUS_PANEL);
            ui.painter().add(SHADOW.as_shape(rect, corners));
            dynamic::glass(ui.painter(), rect, corners);
            ui.painter()
                .galley(rect.min + vec2(24.0, 16.0), words, PALETTE.text);
        });
}

/// The player page: the background again (it rises over the page without
/// a seam), the cover at most 400 (corners 20, the theme's shadow) on the
/// left, and the tabs Up next, Lyrics and Related on the right, with Better
/// Lyrics' lyrics as the theme sets them.
pub fn player_page(app: &App, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    dynamic::paint_again(ui.painter(), area);
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
    // The page fades in as it rises (`open-playerpage`).
    let open =
        ui.ctx()
            .animate_bool_with_time(egui::Id::new("player-page-open"), app.now_playing, 0.3);
    ui.set_opacity(open.max(0.0));

    // Full screen: the shared places, and no limit but the room's.
    let (main, panel) = if app.fullscreen {
        super::now_playing::fullscreen_places(area)
    } else {
        layout(ui.ctx().content_rect().width(), area)
    };
    let largest = if app.fullscreen { f32::INFINITY } else { 400.0 };
    let corners = CornerRadius::same(dynamic::RADIUS_ART_LG);
    // The Song and Video switch over the cover, or in the video mode over
    // the video in its place (16:9, at most 400 high).
    if video::shown(app) {
        let (row, place) = video::stack(main, video::size_in(main, f32::INFINITY, largest));
        video::switch(app, ui, row);
        ui.painter().add(SHADOW.as_shape(place, corners));
        video::paint(app, ui, place, corners);
        cover_click(app, ui, place);
    } else {
        let (row, art) = cover_in(main, largest);
        video::switch(app, ui, row);
        ui.painter().add(SHADOW.as_shape(art, corners));
        ui.painter().rect_filled(art, corners, PALETTE.thumb);
        widgets::picture(app, ui, art, entry.track.thumbnail.as_ref(), corners);
        cover_click(app, ui, art);
        video::note(app, ui, art);
    }

    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(panel)
            .layout(Layout::top_down(egui::Align::Min)),
    );
    ui.spacing_mut().item_spacing.y = 0.0;
    tabs(app, &mut ui, entry);
    match app.np_tab {
        NpTab::UpNext => queue_panel::list(app, &mut ui),
        NpTab::Lyrics => lyrics(app, &mut ui, entry),
        NpTab::Related => related(app, &mut ui, entry),
    }
}

/// YouTube Music's places on the player page, by the window's width (the
/// space above, at the sides, between the cover and the panel, and the
/// panel's share, at most 800). Returns the room left of the panel (for
/// the cover, [`cover_in`]) and the panel.
fn layout(window: f32, area: Rect) -> (Rect, Rect) {
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

/// The Song and Video switch's row and the cover under it, square and at
/// most `largest` (400, `--album-art-size`; any in full screen), together
/// centred in `main`.
fn cover_in(main: Rect, largest: f32) -> (Rect, Rect) {
    let length = main
        .width()
        .min(main.height() - video::SWITCH_ROOM)
        .clamp(0.0, largest);
    video::stack(main, Vec2::splat(length))
}

/// A click on the cover pauses and plays again, and shows what it did: a
/// disc with the icon, growing as it fades over 0.5 s.
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
    ui.painter().circle_filled(
        art.center(),
        26.0 * grow,
        Color32::from_black_alpha((77.0 * fade) as u8),
    );
    let icon = if paused { Icon::Pause } else { Icon::Play };
    theme::paint_icon(
        ui,
        icon,
        Rect::from_center_size(art.center(), Vec2::splat(52.0 * grow)),
        36.0 * grow,
        Color32::from_white_alpha((255.0 * fade) as u8),
    );
}

/// UP NEXT, LYRICS, RELATED: capitals 14/500, all white, in a row 48 high
/// over a white@0.10 hairline; the chosen one's white line, 1 thick,
/// slides to it (0.15 s). Lyrics at 25% when the song has none.
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
        let color = if enabled {
            PALETTE.text
        } else {
            Color32::from_white_alpha(64)
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
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, chosen, name)
        });
        if response.hovered() && enabled {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().galley_with_override_text_color(
            rect.center() - words.size() / 2.0,
            words,
            color,
        );
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
            ui.add_space(48.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.add(egui::Spinner::new().size(22.0).color(PALETTE.secondary));
                ui.add_space(12.0);
                theme::label(
                    ui,
                    "Searching for lyrics",
                    theme::medium(20.0),
                    PALETTE.secondary,
                );
            });
        }
        Some(State::Missing) => {
            ui.add_space(48.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                theme::label(
                    ui,
                    "No lyrics found",
                    theme::medium(20.0),
                    PALETTE.secondary,
                );
            });
        }
        Some(State::Ready(lyrics)) => {
            super::lyric_lines::translate_bar(app, ui, &entry.track.video_id, 24.0);
            lines(app, ui, lyrics, &entry.track.video_id);
        }
    }
}

/// How bright a line is (white at this much): the one being sung, the
/// next (`blyrics--pre-animating`), and the others (`color: 0.3`, the
/// ones already sung at `opacity: 0.4` of that).
const LIT: f32 = 1.0;
const NEXT: f32 = 0.5;
const REST: f32 = 0.3;
const SUNG: f32 = 0.3 * 0.4;
/// The words of the line being sung not yet sung.
const UNSUNG: f32 = 0.45;
/// What is under a line (in Latin letters, in English), as a share of the
/// line's brightness.
const UNDER: f32 = 0.7;
/// The other lines' blur (`$lyrics-blur-amount: 6px`), as a share of the
/// most this draws.
const BLURRED: f32 = 1.0;

/// Better Lyrics' lines as the theme sets them: Inter 40/600 (smaller in a
/// narrow panel) on lines 1.33 times as tall, each with 20 above and below
/// and 10 at the sides; the line being sung white, the next half lit, the
/// rest at 30% and blurred (those sung fainter still), each change easing
/// over 0.6 s (the blur over 0.8 s); the line being sung glows as it
/// starts, its words lit one by one as they are sung. The sung line stays
/// a third of the way down, moving in 0.5 s. A click on a line jumps
/// there; the pointer on a line lights it. Under each line, with
/// Translate on, it in Latin letters and in English.
fn lines(app: &App, ui: &mut egui::Ui, lyrics: &Lyrics, video_id: &str) {
    let id = ui.id().with(("lyrics", video_id));
    let now = ui.input(|i| i.time);
    // Timed to the song: while its video plays, by YouTube's map.
    let clock = app.lyrics_clock();
    let current = clock.and_then(|position| lyrics.current(position));
    let translated = super::lyric_lines::translation(app, lyrics, video_id);
    let viewport = ui.available_height();
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
    // Scrolling by hand pauses following for a few seconds, and lifts the
    // blur meanwhile (`blyrics-user-scrolling`).
    let scrolled = ui.input(|i| i.smooth_scroll_delta.y != 0.0);
    if scrolled && ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
        ui.data_mut(|d| d.insert_temp(id.with("manual"), now + 4.0));
    }
    let tops: Vec<f32> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let manual_until: f64 = ui.data(|d| d.get_temp(id.with("manual"))).unwrap_or(0.0);
    let manual = now <= manual_until;
    let follow = lyrics.synced && !manual;
    let target = current
        .and_then(|i| tops.get(i))
        .map_or(0.0, |top| (top - viewport * 0.3).max(0.0));
    let offset = ui
        .ctx()
        .animate_value_with_time(id.with("scroll"), target, 0.5);
    // When the line being sung changed, for its glow.
    let (lit_line, lit_at): (Option<usize>, f64) = ui
        .data(|d| d.get_temp(id.with("lit")))
        .unwrap_or((None, 0.0));
    let lit_at = if lit_line == current {
        lit_at
    } else {
        ui.data_mut(|d| d.insert_temp(id.with("lit"), (current, now)));
        now
    };
    let glow = (1.0 - (now - lit_at) as f32 / 1.0).clamp(0.0, 1.0);
    if glow > 0.0 {
        ui.ctx().request_repaint();
    }

    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if follow {
        area = area.vertical_scroll_offset(offset);
    }
    area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let origin = ui.min_rect().top();
        ui.add_space(20.0);
        let width = (ui.available_width() - 24.0).max(0.0);
        let size = (width / 11.25).clamp(24.0, 40.0);
        let font = fastframe_fonts::Weight::SemiBold.font_id(size);
        let small = fastframe_fonts::Weight::Medium.font_id((size * 0.5).clamp(14.0, 20.0));
        let feather = size * if lyrics.timed_words { 0.5 } else { 0.9 };
        let (pad_y, pad_x) = (size / 2.0, 10.0);
        let mut new_tops = Vec::with_capacity(lyrics.lines.len());
        for (i, line) in lyrics.lines.iter().enumerate() {
            let text = if line.text.trim().is_empty() {
                if lyrics.synced { "♪" } else { " " }
            } else {
                line.text.as_str()
            };
            let mut job = egui::text::LayoutJob::simple(
                text.to_string(),
                font.clone(),
                Color32::WHITE,
                (width - 2.0 * pad_x).max(0.0),
            );
            job.sections[0].format.line_height = Some(size * 1.333);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let inner = (width - 2.0 * pad_x).max(0.0);
            let under: Vec<_> = super::lyric_lines::under(translated, i)
                .map(|words| {
                    super::lyric_lines::layout(ui, words, &small, inner, Some(small.size * 1.3))
                })
                .collect();
            let under_height: f32 = under
                .iter()
                .map(|line| super::lyric_lines::UNDER_GAP + line.size().y)
                .sum();
            let sense = if line.start.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(
                vec2(width, galley.size().y + under_height + 2.0 * pad_y),
                sense,
            );
            super::lyric_lines::name(&response, text, line.start.is_some());
            new_tops.push(rect.top() - origin);
            let (bright, blur) = if response.hovered() || !lyrics.synced {
                (LIT, 0.0)
            } else {
                match current {
                    Some(c) if c == i => (LIT, 0.0),
                    Some(c) if i < c => (SUNG, BLURRED),
                    Some(c) if i == c + 1 => (NEXT, 0.0),
                    None if i == 0 => (NEXT, 0.0),
                    _ => (REST, BLURRED),
                }
            };
            let blur = if manual { 0.0 } else { blur };
            let ctx = ui.ctx();
            let bright = ctx.animate_value_with_time(id.with(("bright", i)), bright, 0.6);
            let blur = ctx.animate_value_with_time(id.with(("blur", i)), blur, 0.8);
            if ui.is_rect_visible(rect) {
                let at = rect.min + vec2(pad_x, pad_y);
                if current == Some(i) && glow > 0.0 {
                    soft_text(ui, at, &galley, glow * 0.35, 1.6);
                }
                match clock {
                    // The line being sung, word by word.
                    Some(position)
                        if current == Some(i) && !line.words.is_empty() && blur < 0.02 =>
                    {
                        let white = |share: f32| {
                            Color32::from_white_alpha((share.clamp(0.0, 1.0) * 255.0) as u8)
                        };
                        let lit = super::lyric_lines::sung(
                            &galley,
                            &line.words,
                            position,
                            white(bright),
                            white(bright.min(UNSUNG)),
                            feather,
                        );
                        ui.painter().galley(at, lit, white(bright));
                    }
                    _ => soft_text(ui, at, &galley, bright, blur),
                }
                let mut y = at.y + galley.size().y;
                for line in &under {
                    y += super::lyric_lines::UNDER_GAP;
                    soft_text(ui, egui::pos2(at.x, y), line, bright * UNDER, blur);
                    y += line.size().y;
                }
            }
            if let Some(start) = line.start {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    app.act(Action::Seek(app.lyrics_moment(start)));
                    ui.data_mut(|d| d.insert_temp(id.with("manual"), 0.0f64));
                }
            }
        }
        ui.add_space(40.0);
        ui.horizontal(|ui| {
            ui.add_space(pad_x);
            theme::label(
                ui,
                &format!("Source: {}", lyrics.source),
                theme::regular(14.0),
                PALETTE.faint,
            );
        });
        ui.add_space(if lyrics.synced { viewport * 0.6 } else { 40.0 });
        ui.data_mut(|d| d.insert_temp(id, new_tops));
    });
}

/// Text in white at `bright`, blurred by `blur` (0 sharp, 1 the theme's
/// 6 points, more for a glow): copies of it round a small circle, the
/// sharp text fading as the blur grows.
fn soft_text(
    ui: &egui::Ui,
    at: egui::Pos2,
    galley: &std::sync::Arc<egui::Galley>,
    bright: f32,
    blur: f32,
) {
    let painter = ui.painter();
    let white = |share: f32| Color32::from_white_alpha((share.clamp(0.0, 1.0) * 255.0) as u8);
    if blur < 0.02 {
        painter.galley_with_override_text_color(at, galley.clone(), white(bright));
        return;
    }
    let sharp = (1.0 - blur).max(0.0);
    if sharp > 0.0 {
        painter.galley_with_override_text_color(at, galley.clone(), white(bright * sharp));
    }
    const COPIES: usize = 10;
    let reach = 4.0 * blur;
    let each = bright * blur.min(1.0) * 1.6 / COPIES as f32;
    for k in 0..COPIES {
        let angle = std::f32::consts::TAU * k as f32 / COPIES as f32;
        let radius = if k % 2 == 0 { reach } else { reach * 0.5 };
        let offset = vec2(angle.cos(), angle.sin()) * radius;
        painter.galley_with_override_text_color(at + offset, galley.clone(), white(each));
    }
}

fn related(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.related.get(&entry.track.video_id) {
        Some(crate::app::Loadable::Ready(found)) => {
            // Songs played from here start a queue of their own.
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

    #[test]
    fn rows_are_the_themes_and_stay_so() {
        crate::theme::set(crate::theme::Theme::DynamicBackground);
        let list = Row::LIST.themed();
        assert_eq!((list.height, list.gap, list.art), (56.0, 0.0, 38.0));
        // Theming twice changes nothing (lists theme their rows, and each
        // row again).
        assert_eq!(list.themed(), list);
        assert!(!Row::QUEUE.themed().line && !Row::SHELF.themed().line);
        assert_eq!(Row::QUEUE.themed().height + Row::QUEUE.themed().gap, 66.0);
        crate::theme::set(crate::theme::Theme::YouTubeMusic);
        assert_eq!(Row::LIST.themed(), Row::LIST);
    }

    #[test]
    fn the_player_page_keeps_cover_and_panel_apart_and_inside() {
        for (window, width, height) in [
            (960.0, 720.0, 464.0),
            (1280.0, 1040.0, 684.0),
            (1920.0, 1680.0, 1000.0),
        ] {
            let area = Rect::from_min_size(pos2(240.0, 64.0), vec2(width, height));
            let (main, panel) = layout(window, area);
            let (row, art) = cover_in(main, 400.0);
            assert!(area.contains_rect(art) && area.contains_rect(panel));
            assert!(area.contains_rect(row));
            assert!(art.right() + 24.0 <= panel.left());
            assert!(art.width() <= 400.0 && (art.width() - art.height()).abs() < 0.01);
        }
    }
}
