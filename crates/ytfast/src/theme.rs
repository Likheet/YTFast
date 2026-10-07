//! YtFast's look: YouTube Music's own colours and sizes (measured from
//! music.youtube.com), the Inter font, Lucide icons, and the drawing
//! helpers every view uses.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontId, Galley, Rect, Response, Sense, Stroke, Vec2};

/// YouTube Music's colours.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Behind everything (`#030303`).
    pub window: Color32,
    /// The player bar and menus (`#212121`).
    pub panel: Color32,
    /// Buttons, chips, the chosen row: white at 10%.
    pub surface: Color32,
    /// The same, under the pointer: white at 20%.
    pub surface_hover: Color32,
    /// The search box: white at 15%.
    pub field: Color32,
    /// Hairlines: white at 10%.
    pub outline: Color32,
    pub text: Color32,
    /// Second lines: white at 70%.
    pub secondary: Color32,
    /// Quieter text (`#aaaaaa`).
    pub dim: Color32,
    /// What cannot be used: white at 30%.
    pub faint: Color32,
    /// YouTube's red: the progress line.
    pub accent: Color32,
    /// Subscribe's outline.
    pub subscribe: Color32,
    /// A switch that is on.
    pub switch: Color32,
    /// Menus (`#282828`).
    pub menu: Color32,
    pub danger: Color32,
}

/// White at `alpha` (0 to 255), premultiplied as egui wants.
const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

pub const PALETTE: Palette = Palette {
    window: Color32::from_rgb(0x03, 0x03, 0x03),
    panel: Color32::from_rgb(0x21, 0x21, 0x21),
    surface: white(26),
    surface_hover: white(51),
    field: white(38),
    outline: white(26),
    text: Color32::from_rgb(0xff, 0xff, 0xff),
    secondary: white(179),
    dim: Color32::from_rgb(0xaa, 0xaa, 0xaa),
    faint: white(77),
    accent: Color32::from_rgb(0xff, 0x00, 0x00),
    subscribe: Color32::from_rgb(0xff, 0x55, 0x77),
    switch: Color32::from_rgb(0x3e, 0xa6, 0xff),
    menu: Color32::from_rgb(0x28, 0x28, 0x28),
    danger: Color32::from_rgb(0xff, 0x6b, 0x6b),
};

pub const TOP_BAR_HEIGHT: f32 = 64.0;
pub const PLAYER_BAR_HEIGHT: f32 = 72.0;
/// The menu on the left, open and closed.
pub const GUIDE_WIDTH: f32 = 240.0;
pub const GUIDE_MINI_WIDTH: f32 = 72.0;

/// The empty space left and right of a page `width` wide, as YouTube
/// Music leaves it (about 100 in a wide window, less in a narrow one).
pub fn page_margin(width: f32) -> f32 {
    (width * 0.09).clamp(24.0, 100.0)
}

fastframe_icons::icons! {
    /// Every icon the window draws.
    pub enum Icon {
        prefix: "ytfast-icon-",
        directory: "../assets/icons/",
        Home => "house",
        Explore => "compass",
        Library => "library",
        Heart => "heart",
        HeartFilled => "heart-filled",
        ListMusic => "list-music",
        SkipBack => "skip-back-filled",
        SkipForward => "skip-forward-filled",
        Play => "play-filled",
        Pause => "pause-filled",
        Shuffle => "shuffle",
        Repeat => "repeat",
        RepeatOne => "repeat-1",
        Volume => "volume-1",
        Music => "music",
        Loading => "loader-circle",
        Radio => "radio",
        Disc => "disc-3",
        CirclePlay => "circle-play",
        Back => lucide "chevron-left",
        Forward => lucide "chevron-right",
        Search => lucide "search",
        Close => lucide "x",
        LogOut => lucide "log-out",
        Alert => lucide "circle-alert",
        Muted => lucide "volume-x",
        Loud => lucide "volume-2",
        Refresh => lucide "refresh-cw",
        User => lucide "user",
        ThumbsUp => "thumbs-up",
        ThumbsUpFilled => "thumbs-up-filled",
        ThumbsDown => "thumbs-down",
        ThumbsDownFilled => "thumbs-down-filled",
        Collapse => lucide "chevron-down",
        Expand => lucide "chevron-up",
        More => lucide "ellipsis",
        Plus => lucide "plus",
        Trash => lucide "trash-2",
        Pencil => lucide "pencil",
        Settings => lucide "settings",
        Lyrics => lucide "mic",
        History => lucide "clock",
        Copy => lucide "copy",
        Check => lucide "check",
        Menu => "menu",
        MoreVertical => "ellipsis-vertical",
        Pin => lucide "pin",
    }
}

/// Sets up fonts, icons and egui's colours. Call once, at start.
pub fn install(ctx: &egui::Context) {
    let rendering = fastframe_text::detect();
    let mut fonts = fastframe_fonts::FontSetup::default().definitions();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);

    let p = PALETTE;
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = p.window;
        v.window_fill = p.menu;
        v.window_stroke = Stroke::NONE;
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = p.surface;
        v.faint_bg_color = p.panel;
        v.override_text_color = Some(p.text);
        v.hyperlink_color = p.text;
        v.selection.bg_fill = p.accent.gamma_multiply(0.6);
        v.selection.stroke = Stroke::new(1.0, p.text);
        v.widgets.noninteractive.bg_fill = p.panel;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.outline);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.secondary);
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.hovered.bg_fill = p.surface_hover;
        v.widgets.hovered.weak_bg_fill = p.surface_hover;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.active.bg_fill = p.surface_hover;
        v.widgets.active.weak_bg_fill = p.surface_hover;
        v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.slider_rail_height = 4.0;
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = true;
    });
    // fastframe-text's rendering settings, after the visuals.
    ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
}

pub fn regular(size: f32) -> FontId {
    fastframe_fonts::Weight::Regular.font_id(size)
}

pub fn medium(size: f32) -> FontId {
    fastframe_fonts::Weight::Medium.font_id(size)
}

pub fn bold(size: f32) -> FontId {
    fastframe_fonts::Weight::Bold.font_id(size)
}

/// Sets a menu's look, as YouTube Music's menus are: roomy rows of plain
/// words. Call it first inside a menu.
pub fn menu(ui: &mut egui::Ui) {
    ui.set_min_width(220.0);
    let style = ui.style_mut();
    style.spacing.button_padding = egui::vec2(16.0, 11.0);
    style.spacing.item_spacing.y = 0.0;
    style.override_font_id = Some(regular(14.0));
}

/// One line of text that is cut short with "…" when it does not fit.
pub fn label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).font(font).color(color))
            .truncate()
            .selectable(false),
    )
}

/// Text laid out to fit `width`: on at most `rows` lines, the last one cut
/// short with "…".
pub fn fit(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    ui.fonts_mut(|f| f.layout_job(job(text, font, color, width, rows)))
}

/// [`fit`], with each line centred. Paint it at the middle of where it
/// goes, not at its left.
pub fn fit_centered(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = job(text, font, color, width, rows);
    job.halign = egui::Align::Center;
    ui.fonts_mut(|f| f.layout_job(job))
}

fn job(text: &str, font: FontId, color: Color32, width: f32, rows: usize) -> egui::text::LayoutJob {
    let format = egui::TextFormat::simple(font, color);
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), format);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: rows.max(1),
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job
}

/// Paints one line of text at `pos` (its left top), cut short with "…" at
/// `width`. Returns the size drawn.
pub fn paint_line(
    ui: &egui::Ui,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Vec2 {
    let galley = fit(ui, text, font, color, width, 1);
    let size = galley.size();
    ui.painter().galley(pos, galley, color);
    size
}

/// How a round icon button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// Only the icon; a soft disc under the pointer.
    Plain,
    /// On a disc of white at 10%, as YouTube Music's small buttons are.
    Tonal,
    /// A white disc with a black icon: the main Play button.
    Filled,
}

/// A round button `diameter` across, with `icon` drawn `icon_size` large.
pub fn round_button(
    ui: &mut egui::Ui,
    icon: Icon,
    diameter: f32,
    icon_size: f32,
    style: Round,
    color: Color32,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && ui.is_enabled();
        let pressed = response.is_pointer_button_down_on();
        let (fill, tint) = match style {
            Round::Plain if hovered => (Some(PALETTE.surface), color),
            Round::Plain => (None, color),
            Round::Tonal if hovered => (Some(PALETTE.surface_hover), color),
            Round::Tonal => (Some(PALETTE.surface), color),
            Round::Filled => (Some(PALETTE.text), Color32::BLACK),
        };
        let scale = if pressed && style == Round::Filled {
            0.95
        } else {
            1.0
        };
        if let Some(fill) = fill {
            ui.painter()
                .circle_filled(rect.center(), diameter / 2.0 * scale, fill);
        }
        let tint = if ui.is_enabled() {
            tint
        } else {
            tint.gamma_multiply(0.4)
        };
        paint_icon(ui, icon, rect, icon_size * scale, tint);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// A filter button ("Songs", "Albums", a mood): rounded, white at 10%;
/// white with black words when chosen.
pub fn chip(ui: &mut egui::Ui, text: &str, chosen: bool, height: f32) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        if chosen { medium(14.0) } else { regular(14.0) },
        PALETTE.text,
    );
    let size = egui::vec2(galley.size().x + 24.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, text)
    });
    if ui.is_rect_visible(rect) {
        let (fill, color) = if chosen {
            (PALETTE.text, Color32::BLACK)
        } else if response.hovered() {
            (PALETTE.surface_hover, PALETTE.text)
        } else {
            (PALETTE.surface, PALETTE.text)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        let at = rect.center() - galley.size() / 2.0;
        ui.painter()
            .galley_with_override_text_color(at, galley, color);
    }
    response
}

/// YtFast's mark (the app icon) in `rect`.
pub fn paint_logo(ui: &egui::Ui, rect: Rect) {
    egui::Image::new(egui::include_image!("../assets/logo.svg")).paint_at(ui, rect);
}

pub fn paint_icon(ui: &egui::Ui, icon: Icon, rect: Rect, size: f32, tint: Color32) {
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    icon.image(tint, size).paint_at(ui, icon_rect);
}

/// How a pill button is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pill {
    /// White, with black words: the main thing to do.
    Filled,
    /// White at 10%.
    Tonal,
    /// Only an outline and words, in this colour (Subscribe).
    Outline(Color32),
}

/// A pill button 36 high, with an icon before its words when given one.
pub fn pill(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, style: Pill) -> Response {
    let color = match style {
        Pill::Filled => Color32::from_rgb(0x0f, 0x0f, 0x0f),
        Pill::Tonal => PALETTE.text,
        Pill::Outline(color) => color,
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), medium(14.0), color);
    let icon_width = if icon.is_some() { 24.0 } else { 0.0 };
    let width = (galley.size().x + icon_width + 32.0).max(72.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let radius = CornerRadius::same(18);
        match style {
            Pill::Filled => {
                let fill = if hovered {
                    Color32::from_rgb(0xd9, 0xd9, 0xd9)
                } else {
                    Color32::from_rgb(0xf1, 0xf1, 0xf1)
                };
                ui.painter().rect_filled(rect, radius, fill);
            }
            Pill::Tonal => {
                let fill = if hovered {
                    PALETTE.surface_hover
                } else {
                    PALETTE.surface
                };
                ui.painter().rect_filled(rect, radius, fill);
            }
            Pill::Outline(_) => {
                if hovered {
                    ui.painter().rect_filled(rect, radius, PALETTE.surface);
                }
                ui.painter().rect_stroke(
                    rect.shrink(0.5),
                    radius,
                    Stroke::new(1.0, PALETTE.surface_hover),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let content = galley.size().x + icon_width;
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(
                egui::pos2(x - 2.0, rect.center().y - 9.0),
                Vec2::splat(18.0),
            );
            paint_icon(ui, icon, at, 18.0, color);
            x += icon_width;
        }
        let at = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, color);
    }
    response
}

/// A pill button with only words: white when `primary`, else quiet.
pub fn pill_button(ui: &mut egui::Ui, text: &str, primary: bool) -> Response {
    let style = if primary { Pill::Filled } else { Pill::Tonal };
    pill(ui, None, text, style)
}

/// `m:ss` (or `h:mm:ss`).
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_format() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(3_725.0), "1:02:05");
    }
}
