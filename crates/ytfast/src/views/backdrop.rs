//! The window's backdrop: the playing song's cover, blurred into a soft
//! wash of its colours (a few pixels stretched over the window), under a
//! dark veil so text stays easy to read. It fades between songs.

use egui::{Color32, Mesh, Pos2, Rect, Shape, TextureId, Vec2, pos2, vec2};

use crate::app::App;
use crate::theme::PALETTE;

/// How strongly the cover's colours show, at most.
const STRENGTH: f32 = 0.62;
/// On the player page.
const PAGE_STRENGTH: f32 = 0.9;

pub fn paint(app: &App, ui: &egui::Ui, screen: Rect) {
    let ctx = ui.ctx();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, 0.0, PALETTE.window);

    // Stronger, and slowly drifting, on the player page.
    let page = ctx.animate_bool_with_time(egui::Id::new("backdrop-page"), app.now_playing, 0.4);
    let strength = STRENGTH + (PAGE_STRENGTH - STRENGTH) * page;
    let time = ctx.input(|i| i.time) as f32;
    let drift = if app.now_playing {
        vec2((time * 0.05).sin(), (time * 0.037).cos()) * 0.12 * page
    } else {
        Vec2::ZERO
    };
    let backdrop = app.backdrop.borrow();
    if let Some(current) = &backdrop.current {
        let shown = ctx.animate_bool_with_time(egui::Id::new(("backdrop", &current.0)), true, 0.9);
        if let Some(previous) = &backdrop.previous
            && shown < 1.0
        {
            stretched(
                &painter,
                screen,
                previous.1.id(),
                strength * (1.0 - shown),
                drift,
            );
        }
        stretched(&painter, screen, current.1.id(), strength * shown, drift);
    }
    // The veil: darker towards the bottom, where the player bar and most
    // text are.
    let top = Color32::from_black_alpha(110);
    let bottom = Color32::from_black_alpha(205);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(screen.left_top(), top);
    mesh.colored_vertex(screen.right_top(), top);
    mesh.colored_vertex(screen.right_bottom(), bottom);
    mesh.colored_vertex(screen.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// A texture stretched past the window's edges (so its edge pixels do not
/// show as bands).
fn stretched(
    painter: &egui::Painter,
    screen: Rect,
    texture: TextureId,
    strength: f32,
    drift: Vec2,
) {
    if strength <= 0.0 {
        return;
    }
    let bleed = screen.size().max_elem() * 0.15;
    // Drifting looks at a slightly smaller part of the cover, moving.
    let inset = drift.length().min(0.15);
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))
        .shrink(inset)
        .translate(drift * 0.5);
    painter.image(
        texture,
        screen.expand(bleed),
        uv,
        Color32::from_white_alpha((strength * 255.0) as u8),
    );
}
