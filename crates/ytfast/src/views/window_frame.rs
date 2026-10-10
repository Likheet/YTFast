//! YTFast's own window frame on Windows, in place of Windows' title bar:
//! the top bar moves the window (drag it) and maximizes it (double-click),
//! minimize, maximize and close stand at its right end on its middle line,
//! round as the bar's other small buttons (each answers the pointer over
//! the bar's whole height, and close up to the window's corner), and the
//! window's edges resize it. Windows still draws the shadow and rounds the
//! corners.
//! The Mac keeps its own title bar.

use egui::{
    Color32, CornerRadius, CursorIcon, Id, PointerButton, Rect, ResizeDirection, Sense, Stroke,
    Vec2, ViewportCommand, pos2,
};

use crate::theme::PALETTE;

/// YTFast draws its own frame (on Windows).
pub const OWN_FRAME: bool = cfg!(windows);

/// The window's buttons: round, 36 across (as the bar's other small
/// buttons), 4 apart, the last 4 from the window's right edge.
const BUTTON: f32 = 36.0;
const BUTTON_GAP: f32 = 4.0;
const BUTTONS_MARGIN: f32 = 4.0;

/// How far in from the window's edge a press resizes it.
const EDGE: f32 = 5.0;
/// How far from a corner a press resizes both ways.
const CORNER: f32 = 16.0;

/// The width the buttons take at the window's top right (none on the Mac).
pub fn buttons_width() -> f32 {
    if OWN_FRAME {
        3.0 * BUTTON + 2.0 * BUTTON_GAP + BUTTONS_MARGIN
    } else {
        0.0
    }
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

/// Minimize, maximize (restore when maximized) and close, at the right end
/// of `bar`, on its middle line, drawn as YTFast's other small buttons
/// are: round, 36 across, a soft white@0.12 disc under the pointer (close's
/// red, `#e5484d`, with its glyph white), and glyphs 12 across in lines
/// 1.5 thick with round ends, in the bar's white (its quieter grey while
/// the window is in the background). A hand under the pointer, as every
/// button.
///
/// Each answers the pointer in a column of the bar's whole height, the
/// columns meeting halfway between the buttons, and close's reaching the
/// window's right edge: the pointer thrown into the top right corner, as
/// far as it goes, lands on close, as on Windows' own title bars.
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
    let y = bar.center().y;
    let mut right = bar.right() - BUTTONS_MARGIN;
    for caption in [Caption::Close, Caption::Maximize, Caption::Minimize] {
        let disc = Rect::from_min_max(
            pos2(right - BUTTON, y - BUTTON / 2.0),
            pos2(right, y + BUTTON / 2.0),
        );
        right -= BUTTON + BUTTON_GAP;
        // Where it answers the pointer: the bar's height, from halfway to
        // the next button (minimize from its own edge, where the bar moves
        // the window), close to the window's edge.
        let left = match caption {
            Caption::Minimize => disc.left(),
            _ => disc.left() - BUTTON_GAP / 2.0,
        };
        let right_edge = match caption {
            Caption::Close => bar.right(),
            _ => disc.right() + BUTTON_GAP / 2.0,
        };
        let rect = Rect::from_min_max(
            pos2(left, bar.top().min(disc.top())),
            pos2(right_edge, bar.bottom().max(disc.bottom())),
        );
        let name = match caption {
            Caption::Minimize => "Minimize window",
            Caption::Maximize if maximized => "Restore window",
            Caption::Maximize => "Maximize window",
            Caption::Close => "Close window",
        };
        let response = ui.interact(rect, Id::new("window-button").with(name), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        crate::theme::pointing(ui, &response);
        let pressed = response.is_pointer_button_down_on();
        let lit = response.hovered() || pressed;
        let close = caption == Caption::Close;
        if lit {
            let fill = match (close, pressed) {
                (true, false) => Color32::from_rgb(0xe5, 0x48, 0x4d),
                (true, true) => Color32::from_rgb(0xc9, 0x3a, 0x3f),
                (false, false) => Color32::from_white_alpha(31),
                (false, true) => Color32::from_white_alpha(20),
            };
            ui.painter()
                .circle_filled(disc.center(), BUTTON / 2.0, fill);
        }
        let color = if close && lit {
            Color32::WHITE
        } else if !focused {
            PALETTE.secondary
        } else {
            PALETTE.text
        };
        glyph(ui, caption, maximized, disc.center(), color);
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

/// A caption button's glyph, 12 across, centred on `center`: lines 1.5
/// thick with round ends (a dot at each end), squares with corners 2.5.
fn glyph(ui: &egui::Ui, caption: Caption, maximized: bool, center: egui::Pos2, color: Color32) {
    const WIDTH: f32 = 1.5;
    let painter = ui.painter();
    let stroke = Stroke::new(WIDTH, color);
    let c = center;
    // A line with round ends.
    let line = |from: egui::Pos2, to: egui::Pos2| {
        painter.line_segment([from, to], stroke);
        painter.circle_filled(from, WIDTH / 2.0, color);
        painter.circle_filled(to, WIDTH / 2.0, color);
    };
    match caption {
        Caption::Minimize => line(pos2(c.x - 6.0, c.y), pos2(c.x + 6.0, c.y)),
        Caption::Maximize if maximized => {
            // Restore: a square 9 across, and behind it, 3 up and right,
            // the top and right of another.
            let front = Rect::from_min_size(pos2(c.x - 6.0, c.y - 3.0), Vec2::splat(9.0));
            painter.rect_stroke(
                front,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Middle,
            );
            let (left, top, right, bottom) = (
                front.left() + 3.0,
                front.top() - 3.0,
                front.right() + 3.0,
                front.bottom() - 3.0,
            );
            let r = 2.0;
            // From the front square's top, up the back's left side, round
            // its corners, down its right side to the front square's
            // right.
            let corner = |cx: f32, cy: f32, from: f32| {
                (0..=4).map(move |i| {
                    let angle = from + std::f32::consts::FRAC_PI_2 * (i as f32 / 4.0);
                    pos2(cx + r * angle.cos(), cy + r * angle.sin())
                })
            };
            let mut points = vec![pos2(left, front.top())];
            points.extend(corner(left + r, top + r, std::f32::consts::PI));
            points.extend(corner(right - r, top + r, -std::f32::consts::FRAC_PI_2));
            points.push(pos2(right, bottom));
            points.push(pos2(front.right(), bottom));
            painter.add(egui::Shape::line(points, stroke));
        }
        Caption::Maximize => {
            let square = Rect::from_center_size(c, Vec2::splat(11.0));
            painter.rect_stroke(
                square,
                CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Caption::Close => {
            line(pos2(c.x - 5.0, c.y - 5.0), pos2(c.x + 5.0, c.y + 5.0));
            line(pos2(c.x + 5.0, c.y - 5.0), pos2(c.x - 5.0, c.y + 5.0));
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
