//! The Dynamic Background theme (Settings, Theme): the playing song's
//! cover, blurred and slowly moving, behind the whole window, with the bars,
//! the menu and every panel as glass over it.
//!
//! Recreated from chengggit's "Dynamic Background" theme for Better Lyrics
//! (<https://github.com/chengggit/YouTube-Music-Dynamic-Theme>, its default
//! preset), MIT licence, Copyright (c) chengggit: its colours, blur, corners
//! and sizes are carried over, measured from music.youtube.com wearing it
//! (1280 wide). The moving background follows Better Lyrics' kawarp (MIT):
//! the blurred cover, its coordinates warped by slow waves.
//!
//! YouTube Music's layout and sizes stay as they are; this file holds what
//! the theme draws differently, and the views ask for it behind
//! [`theme::dynamic`](crate::theme::dynamic). The page-level parts (the
//! player page, song rows, cards) are in `views/dynamic.rs`.

use std::cell::RefCell;
use std::f32::consts::TAU;

use egui::epaint::{RectShape, Vertex};
use egui::{
    Color32, ColorImage, CornerRadius, Mesh, Painter, Pos2, Rect, Response, Sense, Shadow,
    TextureHandle, TextureId, Vec2, pos2, vec2,
};

use crate::app::{App, PlayState};
use crate::theme::{self, Icon, Palette, Pill};

/// White at `alpha` (0 to 255), premultiplied as egui wants.
const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

/// The theme's colours. The window, the panels and the frame's lines are
/// clear, so the background shows through the bars and the menu; text,
/// icons and the accent are white (`$accent: white`), secondary text
/// white@0.70; buttons white@0.10, white@0.20 under the pointer
/// (`btn-color`, `btn-highlight`).
pub const PALETTE: Palette = Palette {
    window: Color32::TRANSPARENT,
    panel: Color32::TRANSPARENT,
    surface: white(26),
    surface_hover: white(51),
    // `ui-contrast`: the search box.
    field: white(26),
    outline: white(26),
    divider: Color32::TRANSPARENT,
    text: Color32::WHITE,
    secondary: white(179),
    dim: white(179),
    faint: white(77),
    accent: Color32::WHITE,
    subscribe: Color32::WHITE,
    switch: Color32::WHITE,
    // egui's own pop-ups (tooltips): a dark glass.
    menu: Color32::from_rgba_premultiplied(24, 24, 24, 235),
    danger: Color32::from_rgb(0xff, 0x4e, 0x45),
    // Every icon is white (`yt-icon { color: var(--icon) }`); those that
    // are off a little less, so on and off still differ.
    quiet: white(217),
    disabled: white(102),
    thumb: white(38),
    hint: white(179),
    button: Color32::from_rgb(0xf1, 0xf1, 0xf1),
};

/// `--radius-art`: small covers (the player bar's, a menu's).
pub const RADIUS_ART: u8 = 8;
/// `--radius-art-md`: the top result's picture.
pub const RADIUS_ART_MD: u8 = 16;
/// `--radius-art-lg`: cards, an album's cover, the player page's cover.
pub const RADIUS_ART_LG: u8 = 20;
/// `--radius-btn`: chips and mood buttons.
pub const RADIUS_BTN: u8 = 12;
/// `--radius-btn-alt`: buttons with words, the shelves' arrows.
pub const RADIUS_BTN_ALT: u8 = 16;
/// `--radius-highlight`: a row or an entry under the pointer.
pub const RADIUS_HIGHLIGHT: u8 = 8;
/// `--radius-panel`: the search box, its suggestions, toasts.
pub const RADIUS_PANEL: u8 = 12;
/// `--radius-panel-lg`: menus and dialogs.
pub const RADIUS_PANEL_LG: u8 = 24;

/// `--ui-shadow`: `0 4px 16px rgba(0, 0, 0, .2)`.
pub const SHADOW: Shadow = Shadow {
    offset: [0, 4],
    blur: 16,
    spread: 0,
    color: Color32::from_rgba_premultiplied(0, 0, 0, 51),
};

/// The background's settings (`$bg-blur: 0.6`, 60 points; `$bg-saturate`,
/// `$bg-scale`; the default preset's `bg-brightness`).
const SATURATE: f32 = 1.5;
const BRIGHTNESS: f32 = 0.7;
const SCALE: f32 = 1.2;
/// The cover's blur, in the background texture's own pixels (64 across):
/// 60 points on a cover drawn about 1536 wide at 1280.
const BLUR: f32 = 2.5;
const SIDE: usize = 64;

/// A new song's colours take this long to come in (`kawarpTransitionDuration`).
const FADE: f64 = 1.5;
/// How often a new song's colours are drawn as they come in: 30 times a
/// second, for 1.5 s.
const FRAME: std::time::Duration = std::time::Duration::from_millis(33);
/// How often the moving background is drawn: 15 times a second, and only
/// while a song plays (the window draws nothing otherwise). The waves move
/// at most about 1 point a frame then, under 60 points of blur, so the
/// steps do not show, and the window costs half as much as at 30.
const MOVING_FRAME: std::time::Duration = std::time::Duration::from_millis(66);
/// Behind the cover, and the whole background before anything has played.
const BASE: Color32 = Color32::from_rgb(0x0e, 0x0e, 0x0e);

/// The background kept from frame to frame (in egui's memory).
#[derive(Clone, Default)]
struct Backdrop {
    /// The cover shown, and its address.
    current: Option<(String, TextureHandle)>,
    /// The cover before, while the new one fades in.
    previous: Option<TextureHandle>,
    /// When the current one arrived.
    arrived: f64,
    /// How far the waves have moved, in seconds of playing.
    clock: f64,
    /// The time of the last frame drawn.
    last: Option<f64>,
}

/// What the background shows this frame, for the glass to show again.
#[derive(Clone, Copy)]
struct Shown {
    screen: Rect,
    clock: f32,
    previous: Option<TextureId>,
    current: Option<(TextureId, f32)>,
}

thread_local! {
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// Paints the background behind everything: the playing song's cover,
/// blurred, its colours richer and darker, warped by slow waves while the
/// song plays (`app.settings.moving_background`), fading over to the next
/// song's. Call each frame before the views.
pub fn paint(app: &App, ui: &egui::Ui) {
    let ctx = ui.ctx();
    let screen = ctx.content_rect();
    let now = ui.input(|i| i.time);
    let id = egui::Id::new("dynamic-background");
    let mut state: Backdrop = ctx.data(|d| d.get_temp(id)).unwrap_or_default();

    // The playing song's cover, once its picture has arrived.
    let wanted = app
        .playback
        .entry
        .as_ref()
        .and_then(|entry| entry.track.thumbnail.as_ref())
        .map(|thumb| thumb.sized(120));
    if let Some(url) = wanted
        && state.current.as_ref().map(|(had, _)| had) != Some(&url)
        && let Some(summary) = app.images.borrow_mut().summary(&url, &app.backend)
    {
        let texture = ctx.load_texture(
            "dynamic-background",
            wash(&summary.soft),
            egui::TextureOptions::LINEAR,
        );
        state.previous = state.current.take().map(|(_, texture)| texture);
        state.current = Some((url, texture));
        state.arrived = now;
    }
    let fade = ((now - state.arrived) / FADE).clamp(0.0, 1.0) as f32;
    let fade = theme::bezier(0.42, 0.0, 0.58, 1.0, fade);
    if fade >= 1.0 {
        state.previous = None;
    } else {
        ctx.request_repaint_after(FRAME);
    }

    // The waves move only while a song sounds; paused, they stay where
    // they are.
    let sounding = app.playback.state == PlayState::Playing && !app.audio_status.paused;
    let elapsed = state.last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
    state.last = Some(now);
    if sounding && app.settings.moving_background {
        state.clock += elapsed;
        ctx.request_repaint_after(MOVING_FRAME);
    }

    let shown = Shown {
        screen,
        clock: state.clock as f32,
        previous: state.previous.as_ref().map(TextureHandle::id),
        current: state
            .current
            .as_ref()
            .map(|(_, texture)| (texture.id(), fade)),
    };
    let painter = ctx.layer_painter(egui::LayerId::background());
    paint_shown(&painter, &shown);
    SHOWN.with(|cell| *cell.borrow_mut() = Some(shown));
    ctx.data_mut(|d| d.insert_temp(id, state));
}

/// The background again inside `clip` (the player page, as it rises over
/// the page): the same pixels, so it joins the rest without a seam.
pub fn paint_again(painter: &Painter, clip: Rect) {
    let Some(shown) = SHOWN.with(|cell| *cell.borrow()) else {
        painter.rect_filled(clip, 0.0, BASE);
        return;
    };
    paint_shown(
        &painter.with_clip_rect(clip.intersect(painter.clip_rect())),
        &shown,
    );
}

fn paint_shown(painter: &Painter, shown: &Shown) {
    painter.rect_filled(shown.screen, 0.0, BASE);
    if let Some(previous) = shown.previous {
        painter.add(warped(previous, shown.screen, shown.clock, Color32::WHITE));
    }
    if let Some((current, fade)) = shown.current {
        let alpha = (fade * 255.0).round() as u8;
        painter.add(warped(current, shown.screen, shown.clock, white(alpha)));
    }
}

/// The cover over `screen` as a grid whose corners take the cover's
/// colours from warped places: smooth waves, slowly moving with `clock`.
fn warped(texture: TextureId, screen: Rect, clock: f32, tint: Color32) -> Mesh {
    const COLUMNS: u32 = 24;
    const ROWS: u32 = 16;
    let mut mesh = Mesh::with_texture(texture);
    for row in 0..=ROWS {
        for column in 0..=COLUMNS {
            let (x, y) = (column as f32 / COLUMNS as f32, row as f32 / ROWS as f32);
            mesh.vertices.push(Vertex {
                pos: screen.lerp_inside(vec2(x, y)),
                uv: flow(cover_uv(screen, x, y), clock),
                color: tint,
            });
        }
    }
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let a = row * (COLUMNS + 1) + column;
            let below = a + COLUMNS + 1;
            mesh.add_triangle(a, a + 1, below + 1);
            mesh.add_triangle(a, below + 1, below);
        }
    }
    mesh
}

/// Where the point `x`, `y` across the window (0 to 1) finds the cover:
/// `background-size: cover`, centred, zoomed 1.2 times (`$bg-scale`).
fn cover_uv(screen: Rect, x: f32, y: f32) -> Pos2 {
    let aspect = screen.width() / screen.height().max(1.0);
    let (across, down) = if aspect >= 1.0 {
        (1.0, 1.0 / aspect)
    } else {
        (aspect, 1.0)
    };
    pos2(
        0.5 + (x - 0.5) * across / SCALE,
        0.5 + (y - 0.5) * down / SCALE,
    )
}

/// `uv` moved by the waves at `clock` (seconds of playing): two slow
/// waves each way and a gentle turn about the middle, each taking from
/// about 25 to 90 seconds to come round.
fn flow(uv: Pos2, clock: f32) -> Pos2 {
    let wave = |phase: f32| (TAU * phase).sin();
    let (u, v) = (uv.x, uv.y);
    let du = 0.06 * wave(0.8 * v + 0.037 * clock)
        + 0.035 * wave(1.3 * u - 0.6 * v + 0.023 * clock + 0.27);
    let dv = 0.06 * wave(0.9 * u - 0.031 * clock + 0.06)
        + 0.035 * wave(1.1 * v + 0.5 * u - 0.019 * clock + 0.49);
    let angle = 0.14 * wave(0.011 * clock);
    let (sin, cos) = angle.sin_cos();
    let (x, y) = (u - 0.5, v - 0.5);
    pos2(0.5 + x * cos - y * sin + du, 0.5 + x * sin + y * cos + dv)
}

/// A cover's 24 × 24 pixels made into the background: 64 × 64, blurred,
/// its colours richer (`saturate(1.5)`) and darker (`brightness(0.7)`), in
/// that order, as the theme's `filter` has them.
pub fn wash(soft: &ColorImage) -> ColorImage {
    let [width, height] = soft.size;
    if width == 0 || height == 0 {
        return ColorImage::new([SIDE, SIDE], vec![BASE; SIDE * SIDE]);
    }
    // Upscaled smoothly (each pixel's centre where it was).
    let sample = |x: f32, y: f32| -> [f32; 3] {
        let x = (x * width as f32 - 0.5).clamp(0.0, (width - 1) as f32);
        let y = (y * height as f32 - 0.5).clamp(0.0, (height - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| {
            let p = soft.pixels[y * width + x];
            [f32::from(p.r()), f32::from(p.g()), f32::from(p.b())]
        };
        let mut out = [0.0; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            let top = at(x0, y0)[channel] * (1.0 - fx) + at(x1, y0)[channel] * fx;
            let bottom = at(x0, y1)[channel] * (1.0 - fx) + at(x1, y1)[channel] * fx;
            *value = top * (1.0 - fy) + bottom * fy;
        }
        out
    };
    let mut pixels: Vec<[f32; 3]> = (0..SIDE * SIDE)
        .map(|i| {
            let (x, y) = (i % SIDE, i / SIDE);
            sample(
                (x as f32 + 0.5) / SIDE as f32,
                (y as f32 + 0.5) / SIDE as f32,
            )
        })
        .collect();
    blur(&mut pixels, SIDE, BLUR);
    let toned: Vec<[f32; 3]> = pixels
        .into_iter()
        .map(|rgb| saturate(rgb, SATURATE).map(|c| (c * BRIGHTNESS).clamp(0.0, 255.0)))
        .collect();
    // A bright cover is dimmed further, so white words stay readable on it
    // (kawarp's `autoDimBrightArtwork`): past a quarter of white on
    // average, the rest is kept at 40%.
    let light = toned
        .iter()
        .map(|[r, g, b]| (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0)
        .sum::<f32>()
        / toned.len() as f32;
    let dim = if light > DIM_FROM {
        (DIM_FROM + (light - DIM_FROM) * 0.4) / light
    } else {
        1.0
    };
    let image = toned
        .into_iter()
        .map(|rgb| {
            let [r, g, b] = rgb.map(|c| (c * dim).round() as u8);
            Color32::from_rgb(r, g, b)
        })
        .collect();
    ColorImage::new([SIDE, SIDE], image)
}

/// How light (0 black, 1 white) a background may be on average before it
/// is dimmed further.
const DIM_FROM: f32 = 0.25;

/// A Gaussian blur of `sigma` pixels over a square image `side` across,
/// its edges carried on.
fn blur(pixels: &mut [[f32; 3]], side: usize, sigma: f32) {
    let reach = (sigma * 3.0).ceil() as isize;
    let weights: Vec<f32> = (-reach..=reach)
        .map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    let clamp = |i: isize| i.clamp(0, side as isize - 1) as usize;
    for across in [true, false] {
        let source = pixels.to_vec();
        for y in 0..side {
            for x in 0..side {
                let mut sum = [0.0; 3];
                for (k, weight) in weights.iter().enumerate() {
                    let d = k as isize - reach;
                    let (sx, sy) = if across {
                        (clamp(x as isize + d), y)
                    } else {
                        (x, clamp(y as isize + d))
                    };
                    let p = source[sy * side + sx];
                    for c in 0..3 {
                        sum[c] += p[c] * weight;
                    }
                }
                pixels[y * side + x] = sum.map(|s| s / total);
            }
        }
    }
}

/// CSS's `saturate(amount)` (Filter Effects, `feColorMatrix` saturate).
fn saturate([r, g, b]: [f32; 3], s: f32) -> [f32; 3] {
    [
        (0.213 + 0.787 * s) * r + (0.715 - 0.715 * s) * g + (0.072 - 0.072 * s) * b,
        (0.213 - 0.213 * s) * r + (0.715 + 0.285 * s) * g + (0.072 - 0.072 * s) * b,
        (0.213 - 0.213 * s) * r + (0.715 - 0.715 * s) * g + (0.072 + 0.928 * s) * b,
    ]
}

/// Glass in `rect`: the background as it lies there (still, not moving),
/// a breath of white over it, so a menu or a dialog reads as a pane over
/// the page (the theme's `backdrop-filter: blur(10px)` over clear).
pub fn glass(painter: &Painter, rect: Rect, corners: CornerRadius) {
    match SHOWN.with(|cell| *cell.borrow()) {
        Some(shown) => {
            painter.rect_filled(rect, corners, BASE);
            let uv = Rect::from_min_max(
                cover_uv(
                    shown.screen,
                    (rect.left() - shown.screen.left()) / shown.screen.width(),
                    (rect.top() - shown.screen.top()) / shown.screen.height(),
                ),
                cover_uv(
                    shown.screen,
                    (rect.right() - shown.screen.left()) / shown.screen.width(),
                    (rect.bottom() - shown.screen.top()) / shown.screen.height(),
                ),
            );
            if let Some(previous) = shown.previous {
                painter.add(
                    RectShape::filled(rect, corners, Color32::WHITE).with_texture(previous, uv),
                );
            }
            if let Some((current, fade)) = shown.current {
                let alpha = (fade * 255.0).round() as u8;
                painter
                    .add(RectShape::filled(rect, corners, white(alpha)).with_texture(current, uv));
            }
        }
        None => {
            painter.rect_filled(rect, corners, BASE);
        }
    }
    painter.rect_filled(rect, corners, white(10));
}

/// Glass behind what is being drawn in a menu, a dialog or another pop-up,
/// as large as it was last frame (its frame is clear). Call first inside
/// it.
pub fn glass_behind(ui: &egui::Ui, radius: u8) {
    let layer = ui.layer_id();
    if let Some(rect) = ui.ctx().memory(|m| m.area_rect(layer.id)) {
        glass(
            &ui.ctx().layer_painter(layer),
            rect,
            CornerRadius::same(radius),
        );
    }
}

/// A menu: clear (glass is painted behind it, [`glass_behind`]), corners
/// 24, the theme's shadow, 16 above and below its entries.
pub fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .corner_radius(CornerRadius::same(RADIUS_PANEL_LG))
        .shadow(SHADOW)
        .inner_margin(egui::Margin::symmetric(0, 16))
}

/// A dialog's frame: as a menu's, its parts keeping their own padding.
pub fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .corner_radius(CornerRadius::same(RADIUS_PANEL_LG))
        .shadow(SHADOW)
}

/// The main round button (an album's Play): white@0.10 glass with a white
/// icon, white@0.20 under the pointer, growing a little when pressed.
pub fn round_filled(
    ui: &mut egui::Ui,
    icon: Icon,
    diameter: f32,
    icon_size: f32,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            response.hovered() && enabled,
            0.3,
        );
        let fill = lerp(PALETTE.surface, PALETTE.surface_hover, hover);
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, fill);
        let tint = if enabled {
            PALETTE.text
        } else {
            PALETTE.disabled
        };
        theme::paint_icon(ui, icon, rect, icon_size * (1.0 + 0.1 * hover), tint);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// A chip: white@0.10, white@0.20 under the pointer, corners 12; the one
/// chosen white@0.20 with white words (`btn-highlight`, `accent`).
pub fn chip(ui: &mut egui::Ui, text: &str, chosen: bool, height: f32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::regular(14.0), PALETTE.text);
    let size = vec2(galley.size().x + 24.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, text)
    });
    if ui.is_rect_visible(rect) {
        let lit = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            chosen || response.hovered(),
            0.3,
        );
        let fill = lerp(PALETTE.surface, PALETTE.surface_hover, lit);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_BTN), fill);
        let at = rect.center() - galley.size() / 2.0;
        ui.painter()
            .galley_with_override_text_color(at, galley, PALETTE.text);
    }
    response
}

/// A button with words, 36 high and corners 16: white@0.10 glass with
/// white words (white@0.20 under the pointer), as the theme draws
/// YouTube Music's filled, tonal and outlined buttons alike; Subscribe
/// white@0.20 (white@0.40 under the pointer); words-only buttons keep no
/// fill until pointed at.
pub fn pill(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    style: Pill,
    width: Option<f32>,
) -> Response {
    let enabled = ui.is_enabled();
    let color = if enabled {
        PALETTE.text
    } else {
        PALETTE.disabled
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::medium(14.0), color);
    let icon_width = if icon.is_some() { 24.0 } else { 0.0 };
    let width = width.unwrap_or(galley.size().x + icon_width + 32.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            response.hovered() && enabled,
            0.3,
        );
        let (rest, lit) = match style {
            Pill::Ringed(_) => (white(51), white(102)),
            Pill::Plain | Pill::Link => (Color32::TRANSPARENT, PALETTE.surface),
            Pill::Filled | Pill::Tonal | Pill::Outline(_) => {
                (PALETTE.surface, PALETTE.surface_hover)
            }
        };
        let fill = if enabled {
            lerp(rest, lit, hover)
        } else {
            PALETTE.surface
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_BTN_ALT), fill);
        let content = galley.size().x + icon_width;
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(pos2(x - 6.0, rect.center().y - 12.0), Vec2::splat(24.0));
            theme::paint_icon(ui, icon, at, 24.0, color);
            x += icon_width;
        }
        let at = pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, color);
    }
    response
}

/// The theme's switch: a bar 36 × 14 of white@0.20 and a knob 20 across of
/// `#b4b4b4`; on, the bar white@0.50 and the knob white (`accent`), moving
/// over 0.08 s. Named `name` for screen readers. Takes 36 × 20.
pub fn toggle(ui: &mut egui::Ui, on: bool, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, name));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(response.id, on, 0.08);
        let bar = Rect::from_center_size(rect.center(), vec2(36.0, 14.0));
        let track = lerp(white(51), white(128), t);
        let knob = lerp(Color32::from_rgb(0xb4, 0xb4, 0xb4), Color32::WHITE, t);
        ui.painter().rect_filled(bar, CornerRadius::same(7), track);
        let centre = pos2(rect.left() + 10.0 + 16.0 * t, rect.center().y);
        ui.painter()
            .circle_filled(centre + vec2(0.0, 1.0), 11.0, Color32::from_black_alpha(50));
        ui.painter().circle_filled(centre, 10.0, knob);
    }
    response
}

/// `a` turning into `b` (`t` from 0 to 1), premultiplied as they are.
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_becomes_a_soft_darker_richer_background() {
        // Half red, half blue, side by side.
        let pixels = (0..24 * 24)
            .map(|i| {
                if i % 24 < 12 {
                    Color32::from_rgb(200, 40, 40)
                } else {
                    Color32::from_rgb(40, 40, 200)
                }
            })
            .collect();
        let washed = wash(&ColorImage::new([24, 24], pixels));
        assert_eq!(washed.size, [SIDE, SIDE]);
        let left = washed.pixels[SIDE * 32 + 2];
        let middle = washed.pixels[SIDE * 32 + SIDE / 2];
        let right = washed.pixels[SIDE * 32 + SIDE - 3];
        // Darker than the cover (brightness 0.7), the red richer.
        assert!(left.r() < 200 && left.r() > 150, "{left:?}");
        assert!(left.g() < 30, "{left:?}");
        assert!(right.b() > 150 && right.r() < 30, "{right:?}");
        // The edge between them blurred: the middle mixes both.
        assert!(middle.r() > 40 && middle.b() > 40, "{middle:?}");
    }

    #[test]
    fn a_bright_cover_is_dimmed_further() {
        // Light grey: 240 × 0.7 = 168, about two thirds of white, kept
        // at about two fifths.
        let light = ColorImage::new([24, 24], vec![Color32::from_rgb(240, 240, 240); 24 * 24]);
        let washed = wash(&light);
        let pixel = washed.pixels[SIDE * 10 + 10];
        assert!((95..=115).contains(&pixel.r()), "{pixel:?}");
        // A dark one is left as the theme has it.
        let dark = ColorImage::new([24, 24], vec![Color32::from_rgb(60, 60, 60); 24 * 24]);
        assert_eq!(wash(&dark).pixels[0], Color32::from_rgb(42, 42, 42));
    }

    #[test]
    fn an_empty_cover_gives_the_plain_background() {
        let washed = wash(&ColorImage::new([0, 0], Vec::new()));
        assert_eq!(washed.size, [SIDE, SIDE]);
        assert!(washed.pixels.iter().all(|p| *p == BASE));
    }

    #[test]
    fn the_cover_fills_the_window_zoomed() {
        // A wide window shows the cover's whole width (less the zoom), and
        // a band across its middle.
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 820.0));
        let top_left = cover_uv(screen, 0.0, 0.0);
        let bottom_right = cover_uv(screen, 1.0, 1.0);
        let close = |a: f32, b: f32| (a - b).abs() < 0.001;
        assert!(close(top_left.x, 0.5 - 0.5 / SCALE));
        assert!(close(bottom_right.x, 0.5 + 0.5 / SCALE));
        assert!(top_left.y > top_left.x && bottom_right.y < bottom_right.x);
        assert!(close(cover_uv(screen, 0.5, 0.5).x, 0.5));
    }

    #[test]
    fn the_waves_stay_near_the_cover_and_move() {
        for clock in [0.0, 13.0, 600.0] {
            for (u, v) in [(0.1, 0.1), (0.5, 0.5), (0.9, 0.3)] {
                let moved = flow(pos2(u, v), clock);
                assert!((moved.x - u).abs() < 0.2 && (moved.y - v).abs() < 0.2);
            }
        }
        assert_ne!(flow(pos2(0.3, 0.3), 0.0), flow(pos2(0.3, 0.3), 5.0));
    }

    #[test]
    fn saturate_keeps_greys_grey() {
        let [r, g, b] = saturate([100.0, 100.0, 100.0], SATURATE);
        assert!((r - 100.0).abs() < 0.01 && (g - 100.0).abs() < 0.01 && (b - 100.0).abs() < 0.01);
    }
}
