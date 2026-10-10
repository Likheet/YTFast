//! What lies behind the pages. The window itself is YouTube Music's
//! near-black, as is the player page. An album's or playlist's page has
//! its cover, blurred, at the top; an artist's page its picture.

use egui::layers::ShapeIdx;
use egui::{Color32, Mesh, Pos2, Rect, Shape, TextureId, pos2};

use crate::theme::PALETTE;

/// The window's colour, and two places kept just above it, which the page
/// knows of only after the bars and the menu are drawn but must lie under
/// them: an album's background (`album_cover`), and over it Premium's
/// player page colours (`listening`).
pub fn paint(ui: &egui::Ui, screen: Rect) -> (ShapeIdx, ShapeIdx) {
    let painter = ui.ctx().layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, 0.0, PALETTE.window);
    (painter.add(Shape::Noop), painter.add(Shape::Noop))
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
    // Premium: a lighter veil, so more of the cover's colour shows.
    let darkest = if crate::theme::premium() { 110 } else { 153 };
    let veil = gradient_shape(dark, Color32::from_black_alpha(darkest), PALETTE.window);
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

/// Premium's player page: the playing song's colours (`App::listening_wash`,
/// the cover blurred, so it never moves) over `screen`, darkened more
/// toward the foot and at the corners so white words read on them, at
/// `shown` (fading in as the page rises).
pub fn listening(screen: Rect, texture: TextureId, shown: f32) -> Shape {
    let alpha = |a: f32| (a * shown) as u8;
    Shape::Vec(vec![
        Shape::image(
            texture,
            screen,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::from_white_alpha(alpha(255.0)),
        ),
        gradient_shape(
            screen,
            Color32::from_black_alpha(alpha(40.0)),
            Color32::from_black_alpha(alpha(150.0)),
        ),
        vignette(screen, alpha(110.0)),
    ])
}

/// Premium's main pages: the playing song's colours (`App::listening_wash`)
/// across the window's top, fading out by 70% of its height into the
/// page's charcoal; with nothing playing (or its cover not yet here), a
/// deep wine-to-indigo glow there instead. Fixed to the window and still,
/// so it costs nothing while idle. At `shown`.
pub fn ambient(screen: Rect, texture: Option<TextureId>, shown: f32) -> Shape {
    let foot = screen.top() + screen.height() * 0.7;
    let band = Rect::from_min_max(screen.min, pos2(screen.right(), foot));
    let corners = [
        band.left_top(),
        band.right_top(),
        band.right_bottom(),
        band.left_bottom(),
    ];
    let mut mesh = match texture {
        Some(texture) => {
            let top = Color32::from_white_alpha((150.0 * shown) as u8);
            let uvs = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 0.7),
                pos2(0.0, 0.7),
            ];
            let colors = [top, top, Color32::TRANSPARENT, Color32::TRANSPARENT];
            let mut mesh = Mesh::with_texture(texture);
            for ((pos, uv), color) in corners.into_iter().zip(uvs).zip(colors) {
                mesh.vertices.push(egui::epaint::Vertex { pos, uv, color });
            }
            mesh
        }
        None => {
            let glow = (100.0 * shown) as u8;
            let wine = Color32::from_rgba_unmultiplied(0x5a, 0x16, 0x2c, glow);
            let indigo = Color32::from_rgba_unmultiplied(0x22, 0x26, 0x5c, glow);
            let colors = [wine, indigo, Color32::TRANSPARENT, Color32::TRANSPARENT];
            let mut mesh = Mesh::default();
            for (pos, color) in corners.into_iter().zip(colors) {
                mesh.colored_vertex(pos, color);
            }
            mesh
        }
    };
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

/// Black `alpha` at `rect`'s corners, half that at its edges' middles,
/// nothing at its centre: the edges darkened, as a lens does.
fn vignette(rect: Rect, alpha: u8) -> Shape {
    let edge = Color32::from_black_alpha(alpha / 2);
    let corner = Color32::from_black_alpha(alpha);
    let mut mesh = Mesh::default();
    let xs = [rect.left(), rect.center().x, rect.right()];
    let ys = [rect.top(), rect.center().y, rect.bottom()];
    for (row, &y) in ys.iter().enumerate() {
        for (column, &x) in xs.iter().enumerate() {
            let color = match (row, column) {
                (1, 1) => Color32::TRANSPARENT,
                (1, _) | (_, 1) => edge,
                _ => corner,
            };
            mesh.colored_vertex(pos2(x, y), color);
        }
    }
    for (a, b, c, d) in [(0, 1, 4, 3), (1, 2, 5, 4), (3, 4, 7, 6), (4, 5, 8, 7)] {
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(a, c, d);
    }
    Shape::mesh(mesh)
}
