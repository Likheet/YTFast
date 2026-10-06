//! YtFast's look: YouTube Music's dark colours, the Inter font, Lucide
//! icons, and the few drawing helpers every view uses.

use egui::{Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, Vec2};

/// Colours, after YouTube Music's dark theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Behind everything (the page).
    pub window: Color32,
    /// The sidebar and the player bar.
    pub panel: Color32,
    /// Raised things: search box, buttons, placeholders.
    pub surface: Color32,
    pub surface_hover: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub dim: Color32,
    /// YouTube red: the progress bar, the active item.
    pub accent: Color32,
    pub danger: Color32,
}

pub const PALETTE: Palette = Palette {
    window: Color32::from_rgb(0x03, 0x03, 0x03),
    panel: Color32::from_rgb(0x0f, 0x0f, 0x0f),
    surface: Color32::from_rgb(0x21, 0x21, 0x21),
    surface_hover: Color32::from_rgb(0x2e, 0x2e, 0x2e),
    outline: Color32::from_rgb(0x2a, 0x2a, 0x2a),
    text: Color32::from_rgb(0xff, 0xff, 0xff),
    secondary: Color32::from_rgb(0xaa, 0xaa, 0xaa),
    dim: Color32::from_rgb(0x71, 0x71, 0x71),
    accent: Color32::from_rgb(0xff, 0x00, 0x33),
    danger: Color32::from_rgb(0xff, 0x6b, 0x6b),
};

pub const PLAYER_BAR_HEIGHT: f32 = 72.0;
pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const TOP_BAR_HEIGHT: f32 = 64.0;
pub const QUEUE_WIDTH: f32 = 340.0;
pub const CARD_SIZE: f32 = 176.0;
pub const ROW_HEIGHT: f32 = 56.0;

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
        v.window_fill = p.panel;
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

/// One line of text that is cut short with "…" when it does not fit.
pub fn label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).font(font).color(color))
            .truncate()
            .selectable(false),
    )
}

/// An icon that is a button: brighter when hovered.
pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    tooltip: &str,
) -> Response {
    let edge = size + 12.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(edge), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        let tint = if response.hovered() {
            PALETTE.text
        } else {
            color
        };
        if response.hovered() {
            ui.painter()
                .circle_filled(rect.center(), edge / 2.0, PALETTE.surface);
        }
        paint_icon(ui, icon, rect, size, tint);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// The big round play/pause button of the player bar.
pub fn round_button(ui: &mut egui::Ui, icon: Icon, diameter: f32, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        let scale = if response.is_pointer_button_down_on() {
            0.94
        } else {
            1.0
        };
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0 * scale, PALETTE.text);
        paint_icon(ui, icon, rect, diameter * 0.45 * scale, Color32::BLACK);
    }
    response.on_hover_text(tooltip)
}

/// YtFast's mark (the app icon) in `rect`.
pub fn paint_logo(ui: &egui::Ui, rect: Rect) {
    egui::Image::new(egui::include_image!("../assets/logo.svg")).paint_at(ui, rect);
}

pub fn paint_icon(ui: &egui::Ui, icon: Icon, rect: Rect, size: f32, tint: Color32) {
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    icon.image(tint, size).paint_at(ui, icon_rect);
}

/// A rounded pill button with text ("Play", "Sign in").
pub fn pill_button(ui: &mut egui::Ui, text: &str, primary: bool) -> Response {
    let (fill, color) = if primary {
        (PALETTE.text, Color32::BLACK)
    } else {
        (PALETTE.surface, PALETTE.text)
    };
    let button = egui::Button::new(egui::RichText::new(text).font(medium(14.0)).color(color))
        .fill(fill)
        .corner_radius(CornerRadius::same(18))
        .min_size(egui::vec2(88.0, 36.0));
    ui.add(button)
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
