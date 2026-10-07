//! What lies behind the pages. The window itself is YouTube Music's
//! near-black. An album's or playlist's page is washed at the top with its
//! cover's colour, and the player page lies over the playing song's cover,
//! blurred into a soft wash of its colours (a few pixels stretched over
//! the page) under a dark veil, fading from one song to the next.

use egui::{Color32, Mesh, Pos2, Rect, Shape, TextureId, pos2, vec2};

use crate::app::App;
use crate::theme::PALETTE;

/// How strongly the cover's colours show on the player page.
const STRENGTH: f32 = 0.85;

pub fn paint(ui: &egui::Ui, screen: Rect) {
    ui.ctx()
        .layer_painter(egui::LayerId::background())
        .rect_filled(screen, 0.0, PALETTE.window);
}

/// The playing song's cover behind the player page, in `rect`.
pub fn cover(app: &App, ui: &egui::Ui, rect: Rect) {
    let ctx = ui.ctx();
    let painter = ui.painter().with_clip_rect(rect);
    let time = ctx.input(|i| i.time) as f32;
    // Slowly drifting.
    let drift = vec2((time * 0.05).sin(), (time * 0.037).cos()) * 0.12;
    let backdrop = app.backdrop.borrow();
    if let Some(current) = &backdrop.current {
        let shown = ctx.animate_bool_with_time(egui::Id::new(("backdrop", &current.0)), true, 0.9);
        if let Some(previous) = &backdrop.previous
            && shown < 1.0
        {
            stretched(
                &painter,
                rect,
                previous.1.id(),
                STRENGTH * (1.0 - shown),
                drift,
            );
        }
        stretched(&painter, rect, current.1.id(), STRENGTH * shown, drift);
    }
    // The veil: darker towards the bottom, so words stay easy to read.
    gradient(
        &painter,
        rect,
        Color32::from_black_alpha(120),
        Color32::from_black_alpha(215),
    );
}

/// How strongly a page's cover colours the very top of the window.
const WASH: f32 = 0.42;

/// A wash of `color`, the colour of a page's cover, behind its top:
/// strongest at the top of `whole` and gone at its bottom. `rect` is the
/// part of it to paint (the top bar paints its own part, the page the
/// rest, and the two meet without an edge).
pub fn wash(ui: &egui::Ui, rect: Rect, color: Color32, whole: egui::Rangef) {
    let at = |y: f32| {
        let down = ((y - whole.min) / (whole.max - whole.min).max(1.0)).clamp(0.0, 1.0);
        color.gamma_multiply(WASH * (1.0 - down))
    };
    gradient(ui.painter(), rect, at(rect.top()), at(rect.bottom()));
}

fn gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// A texture stretched past the page's edges (so its edge pixels do not
/// show as bands).
fn stretched(
    painter: &egui::Painter,
    rect: Rect,
    texture: TextureId,
    strength: f32,
    drift: egui::Vec2,
) {
    if strength <= 0.0 {
        return;
    }
    let bleed = rect.size().max_elem() * 0.15;
    // Drifting looks at a slightly smaller part of the cover, moving.
    let inset = drift.length().min(0.15);
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))
        .shrink(inset)
        .translate(drift * 0.5);
    painter.image(
        texture,
        rect.expand(bleed),
        uv,
        Color32::from_white_alpha((strength * 255.0) as u8),
    );
}
