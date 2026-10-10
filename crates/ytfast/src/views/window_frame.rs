//! YTFast's own window frame on Windows, in place of Windows' title bar:
//! the top bar moves the window (drag it) and maximizes it (double-click),
//! Windows 11's three buttons stand at its right end, as tall as the bar
//! so their glyphs sit on its middle line with the bar's other buttons,
//! and the window's edges resize it. Windows still draws the shadow and rounds the corners.
//! The Mac keeps its own title bar.

use egui::{
    Color32, CornerRadius, CursorIcon, Id, PointerButton, Rect, ResizeDirection, Sense, Stroke,
    Vec2, ViewportCommand, pos2, vec2,
};

use crate::theme::PALETTE;

/// YTFast draws its own frame (on Windows).
pub const OWN_FRAME: bool = cfg!(windows);

/// One of the buttons: Windows 11's caption buttons are 46 wide (32 high
/// there; here as high as the bar they stand in).
const BUTTON_WIDTH: f32 = 46.0;

/// How far in from the window's edge a press resizes it.
const EDGE: f32 = 5.0;
/// How far from a corner a press resizes both ways.
const CORNER: f32 = 16.0;

/// The width the buttons take at the window's top right (none on the Mac).
pub fn buttons_width() -> f32 {
    if OWN_FRAME { 3.0 * BUTTON_WIDTH } else { 0.0 }
}

/// Lets `bar` move the window, as a title bar does: drag to move (Windows
/// snaps it to the screen's edges), double-click to maximize or restore.
/// Call it before drawing what is on the bar, so its buttons come first.
pub fn drag_area(ui: &egui::Ui, bar: Rect) {
    if !OWN_FRAME {
        return;
    }
    let response = ui.interact(bar, Id::new("window-drag"), Sense::click_and_drag());
    if response.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// What a caption button does.
#[derive(Clone, Copy, PartialEq)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// Windows 11's minimize, maximize (restore when maximized) and close, at
/// the right end of `bar`: 46 wide and as high as the bar, glyphs 10
/// across on its middle line, in the bar's own white (its quieter grey
/// while the window is in the background); under the pointer white@0.06
/// (white@0.04 pressed), and close `#c42b1c` with a white glyph.
pub fn buttons(ui: &egui::Ui, bar: Rect) {
    if !OWN_FRAME {
        return;
    }
    let (maximized, focused) = ui.input(|i| {
        let viewport = i.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.focused.unwrap_or(true),
        )
    });
    let mut right = bar.right();
    for caption in [Caption::Close, Caption::Maximize, Caption::Minimize] {
        let rect = Rect::from_min_size(
            pos2(right - BUTTON_WIDTH, bar.top()),
            vec2(BUTTON_WIDTH, bar.height()),
        );
        right -= BUTTON_WIDTH;
        let name = match caption {
            Caption::Minimize => "Minimize window",
            Caption::Maximize if maximized => "Restore window",
            Caption::Maximize => "Maximize window",
            Caption::Close => "Close window",
        };
        let response = ui.interact(rect, Id::new("window-button").with(name), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        let pressed = response.is_pointer_button_down_on();
        let lit = response.hovered() || pressed;
        let close = caption == Caption::Close;
        let fill = match (close, lit, pressed) {
            (true, true, false) => Color32::from_rgb(0xc4, 0x2b, 0x1c),
            (true, true, true) => Color32::from_rgba_unmultiplied(0xc4, 0x2b, 0x1c, 0xe6),
            (false, true, false) => Color32::from_white_alpha(15),
            (false, true, true) => Color32::from_white_alpha(10),
            _ => Color32::TRANSPARENT,
        };
        // The top right corner keeps the window's rounding.
        let rounding = if close && !maximized {
            CornerRadius {
                ne: 8,
                ..CornerRadius::ZERO
            }
        } else {
            CornerRadius::ZERO
        };
        ui.painter().rect_filled(rect, rounding, fill);
        let color = if close && lit {
            PALETTE.text
        } else if !focused {
            PALETTE.secondary
        } else if pressed {
            Color32::from_white_alpha(200)
        } else {
            PALETTE.text
        };
        glyph(ui, caption, maximized, rect.center(), color);
        if response.clicked() {
            let command = match caption {
                Caption::Minimize => ViewportCommand::Minimized(true),
                Caption::Maximize => ViewportCommand::Maximized(!maximized),
                Caption::Close => ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// A caption button's glyph, 10 across, centred on `center`, in lines one
/// point thick, as Segoe Fluent Icons draws them.
fn glyph(ui: &egui::Ui, caption: Caption, maximized: bool, center: egui::Pos2, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, color);
    let c = pos2(
        painter.round_to_pixel_center(center.x),
        painter.round_to_pixel_center(center.y),
    );
    match caption {
        Caption::Minimize => {
            painter.line_segment([pos2(c.x - 5.0, c.y), pos2(c.x + 5.0, c.y)], stroke);
        }
        Caption::Maximize if maximized => {
            // Restore: a square 8 across, and the corner of another behind
            // it, 2 up and right.
            let front = Rect::from_min_size(pos2(c.x - 5.0, c.y - 3.0), Vec2::splat(8.0));
            painter.rect_stroke(
                front,
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment(
                [pos2(c.x - 3.0, c.y - 5.0), pos2(c.x + 4.0, c.y - 5.0)],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x + 5.0, c.y - 4.0), pos2(c.x + 5.0, c.y + 3.0)],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x + 4.0, c.y - 5.0), pos2(c.x + 5.0, c.y - 4.0)],
                stroke,
            );
        }
        Caption::Maximize => {
            let square = Rect::from_center_size(c, Vec2::splat(10.0));
            painter.rect_stroke(
                square,
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Caption::Close => {
            painter.line_segment(
                [pos2(c.x - 5.0, c.y - 5.0), pos2(c.x + 5.0, c.y + 5.0)],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x + 5.0, c.y - 5.0), pos2(c.x - 5.0, c.y + 5.0)],
                stroke,
            );
        }
    }
}

/// The window's edges and corners resize it (not while maximized). They
/// lie over everything, 5 in from the edge, but leave the buttons' corner
/// to Close, as Windows does.
pub fn edges(ctx: &egui::Context) {
    if !OWN_FRAME {
        return;
    }
    let (maximized, fullscreen) = ctx.input(|i| {
        let viewport = i.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }
    let w = ctx.content_rect();
    let buttons_left = w.right() - buttons_width();
    let zones = [
        (
            ResizeDirection::North,
            Rect::from_min_max(
                pos2(w.left() + CORNER, w.top()),
                pos2(buttons_left, w.top() + EDGE),
            ),
        ),
        (
            ResizeDirection::South,
            Rect::from_min_max(
                pos2(w.left() + CORNER, w.bottom() - EDGE),
                pos2(w.right() - CORNER, w.bottom()),
            ),
        ),
        (
            ResizeDirection::West,
            Rect::from_min_max(
                pos2(w.left(), w.top() + CORNER),
                pos2(w.left() + EDGE, w.bottom() - CORNER),
            ),
        ),
        (
            ResizeDirection::East,
            Rect::from_min_max(
                pos2(w.right() - EDGE, w.top() + crate::theme::top_bar_height()),
                pos2(w.right(), w.bottom() - CORNER),
            ),
        ),
        (
            ResizeDirection::NorthWest,
            Rect::from_min_size(w.min, Vec2::splat(CORNER)),
        ),
        (
            ResizeDirection::SouthWest,
            Rect::from_min_max(
                pos2(w.left(), w.bottom() - CORNER),
                pos2(w.left() + CORNER, w.bottom()),
            ),
        ),
        (
            ResizeDirection::SouthEast,
            Rect::from_min_max(pos2(w.right() - CORNER, w.bottom() - CORNER), w.max),
        ),
    ];
    for (index, (direction, zone)) in zones.into_iter().enumerate() {
        if zone.width() <= 0.0 || zone.height() <= 0.0 {
            continue;
        }
        egui::Area::new(Id::new("window-edge").with(index))
            .order(egui::Order::Foreground)
            .fixed_pos(zone.min)
            .constrain(false)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(zone.size(), Sense::drag());
                if response.hovered() {
                    ctx.set_cursor_icon(cursor(direction));
                    if ui.input(|i| i.pointer.primary_pressed()) {
                        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                    }
                }
            });
    }
}

/// The pointer for resizing in `direction`.
fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}
