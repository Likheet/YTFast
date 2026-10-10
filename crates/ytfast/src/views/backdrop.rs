//! What lies behind the pages. The window itself is YouTube Music's
//! near-black, as is the player page. An album's or playlist's page has
//! its cover, blurred, at the top; an artist's page its picture.

use egui::layers::ShapeIdx;
use egui::{Color32, Mesh, Pos2, Rect, Shape, TextureId, pos2};

use crate::theme::PALETTE;

/// The window's colour, and a place kept just above it for an album's
/// background (`album_cover`), which the page knows of only after the bars
/// and the menu are drawn but must lie under them.
pub fn paint(ui: &egui::Ui, screen: Rect) -> ShapeIdx {
    let painter = ui.ctx().layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, 0.0, PALETTE.window);
    painter.add(Shape::Noop)
}

/// Behind an album's or playlist's page, as YouTube Music: its cover,
/// blurred (80 points), across the window's top, 0.49 of the window high
/// and as wide as the window but its scroll bar's room; its edges fading
/// as a blur does. Over it, from the window's top to its foot, black from
/// 60% to the window's colour. Both move up by `offset` as the songs
/// scroll; `shown` fades it in. Painted in `slot`, under the bars.
pub fn album_cover(
    ui: &egui::Ui,
    slot: ShapeIdx,
    screen: Rect,
    texture: TextureId,
    shown: f32,
    offset: f32,
) {
    // Dynamic Background: the song's own background shows instead.
    if crate::theme::dynamic() {
        return;
    }
    let painter = ui.ctx().layer_painter(egui::LayerId::background());
    let top = screen.top() - offset;
    let width = screen.width() - 12.0;
    let height = screen.height() * 0.49;
    // The band, its edges half seen and gone 160 below its foot.
    let xs = [
        0.0,
        160.0_f32.min(width / 2.0),
        (width - 160.0).max(width / 2.0),
        width,
    ];
    let x_alpha = [0.5, 1.0, 1.0, 0.5];
    let ys = [0.0, 160.0_f32.min(height / 2.0), height, height + 160.0];
    let y_alpha = [0.5, 1.0, 0.5, 0.0];
    let mut mesh = Mesh::with_texture(texture);
    for (row, (&y, &ya)) in ys.iter().zip(&y_alpha).enumerate() {
        for (&x, &xa) in xs.iter().zip(&x_alpha) {
            let uv = pos2(x / width, (y / height).min(1.0));
            let alpha = (xa * ya * shown * 255.0) as u8;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(screen.left() + x, top + y),
                uv,
                color: Color32::from_white_alpha(alpha),
            });
        }
        if row > 0 {
            let base = (row as u32 - 1) * 4;
            for column in 0..3 {
                let a = base + column;
                mesh.add_triangle(a, a + 1, a + 5);
                mesh.add_triangle(a, a + 5, a + 4);
            }
        }
    }
    // The darkening, the whole window wide.
    let dark = Rect::from_min_max(
        pos2(screen.left(), top),
        pos2(screen.right(), top + screen.height()),
    );
    let veil = gradient_shape(dark, Color32::from_black_alpha(153), PALETTE.window);
    painter.set(slot, Shape::Vec(vec![Shape::mesh(mesh), veil]));
}

/// Behind an artist's header, as YouTube Music: its picture filling
/// `frame` (cropped to fill, its top kept), and from `fade` down to the
/// header's foot a fade to the window's colour, solid for the last 8.98%
/// (`linear-gradient(1turn, #030303 8.98%, transparent)`), the whole
/// window wide. Painted in `slot`, under the bars and the menu.
pub fn artist_picture(
    ui: &egui::Ui,
    slot: ShapeIdx,
    screen: Rect,
    frame: Rect,
    texture: Option<(TextureId, [usize; 2])>,
    fade: f32,
) {
    if crate::theme::dynamic() {
        // The menu is see-through: the picture runs the window's width.
        let frame = Rect::from_x_y_ranges(screen.x_range(), frame.y_range());
        return super::dynamic::artist_picture(ui, slot, frame, texture);
    }
    let mut shapes = Vec::new();
    if let Some((texture, [w, h])) = texture {
        let picture = w as f32 / h.max(1) as f32;
        let shape = frame.width() / frame.height().max(1.0);
        let uv = if picture > shape {
            // Wider than its frame: the sides go.
            let keep = shape / picture;
            Rect::from_min_max(pos2((1.0 - keep) / 2.0, 0.0), pos2((1.0 + keep) / 2.0, 1.0))
        } else {
            // Taller: keep the top, where faces are.
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, picture / shape))
        };
        shapes.push(Shape::image(texture, frame, uv, Color32::WHITE));
    }
    let foot = frame.bottom();
    let solid = foot - (foot - fade) * 0.0898;
    shapes.push(gradient_shape(
        Rect::from_min_max(pos2(screen.left(), fade), pos2(screen.right(), solid)),
        Color32::TRANSPARENT,
        PALETTE.window,
    ));
    shapes.push(Shape::rect_filled(
        Rect::from_min_max(pos2(screen.left(), solid), pos2(screen.right(), foot)),
        0.0,
        PALETTE.window,
    ));
    ui.ctx()
        .layer_painter(egui::LayerId::background())
        .set(slot, Shape::Vec(shapes));
}

/// `rect` from `top`'s colour at its top to `bottom`'s at its foot.
fn gradient_shape(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

/// Premium's player page: a still wash of the small cover already loaded
/// (no blur pass, nothing moving), fading to the window's colour.
pub fn listening(ui: &egui::Ui, rect: Rect, texture: TextureId) {
    ui.painter().image(
        texture,
        rect,
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::from_white_alpha(42),
    );
    ui.painter()
        .add(gradient_shape(rect, Color32::TRANSPARENT, PALETTE.window));
}
