//! Colours from album art: the cover shrunk to a few pixels, which drawn
//! stretched is the blurred cover behind the top of an album's or
//! playlist's page.

use egui::{Color32, ColorImage};

/// What the look takes from a cover.
#[derive(Clone)]
pub struct Summary {
    /// The cover shrunk to a few pixels: drawn stretched over the page,
    /// it becomes a soft blur of the cover's colours.
    pub tiny: ColorImage,
    /// The cover shrunk to 16 × 16 (1 KB), from which Premium makes its
    /// backgrounds (`wash`).
    pub small: ColorImage,
}

/// The side of [`Summary::tiny`].
const TINY: usize = 6;

/// The side of [`Summary::small`].
const SMALL: usize = 16;

/// The side of a [`wash`].
const WASH: usize = 48;

/// Summarises a cover.
pub fn summarize(image: &ColorImage) -> Summary {
    Summary {
        tiny: shrink(image, TINY),
        small: shrink(image, SMALL),
    }
}

/// A cover's 16 × 16 ([`Summary::small`]) made into Premium's background:
/// drawn up to 48 × 48 and blurred (three box blurs 4 either side, near
/// enough a Gaussian), so its light and dark parts stay where they were but
/// soft, not averaged into one grey; its colours richer, and kept dark
/// enough that white words read over them.
pub fn wash(small: &ColorImage) -> ColorImage {
    let mut pixels = enlarge(small, WASH);
    for _ in 0..3 {
        box_blur(&mut pixels, WASH, 4);
    }
    let pixels = pixels
        .iter()
        .map(|&[r, g, b]| {
            let pixel = Color32::from_rgb(r.round() as u8, g.round() as u8, b.round() as u8);
            let mut hsv = egui::ecolor::HsvaGamma::from(pixel);
            hsv.s = (hsv.s * 1.5).min(1.0);
            hsv.v = (hsv.v * 0.9).clamp(0.16, 0.6);
            Color32::from(hsv)
        })
        .collect();
    ColorImage::new([WASH, WASH], pixels)
}

/// `image` drawn up to `side` × `side`, each new pixel blended from the
/// four nearest old ones.
fn enlarge(image: &ColorImage, side: usize) -> Vec<[f32; 3]> {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        return vec![[0.0; 3]; side * side];
    }
    let at = |x: usize, y: usize| {
        let p = image.pixels[y.min(height - 1) * width + x.min(width - 1)];
        [f32::from(p.r()), f32::from(p.g()), f32::from(p.b())]
    };
    // Where pixel `i` of `side` falls among `old` pixels: the one before
    // it and how far towards the next.
    let place = |i: usize, old: usize| {
        let f = ((i as f32 + 0.5) * old as f32 / side as f32 - 0.5).max(0.0);
        (f.floor() as usize, f.fract())
    };
    let mut pixels = Vec::with_capacity(side * side);
    for y in 0..side {
        let (y0, ty) = place(y, height);
        for x in 0..side {
            let (x0, tx) = place(x, width);
            let (a, b, c, d) = (
                at(x0, y0),
                at(x0 + 1, y0),
                at(x0, y0 + 1),
                at(x0 + 1, y0 + 1),
            );
            pixels.push(std::array::from_fn(|i| {
                let top = a[i] + (b[i] - a[i]) * tx;
                let bottom = c[i] + (d[i] - c[i]) * tx;
                top + (bottom - top) * ty
            }));
        }
    }
    pixels
}

/// A box blur of `pixels` (`side` × `side`), `radius` either side, across
/// then down, the edges held.
fn box_blur(pixels: &mut [[f32; 3]], side: usize, radius: usize) {
    let count = (2 * radius + 1) as f32;
    let mut line = vec![[0.0f32; 3]; side];
    for across in [true, false] {
        let index = |row: usize, at: usize| {
            if across {
                row * side + at
            } else {
                at * side + row
            }
        };
        for row in 0..side {
            for (at, out) in line.iter_mut().enumerate() {
                let mut sum = [0.0f32; 3];
                for k in 0..=2 * radius {
                    let from = (at + k).saturating_sub(radius).min(side - 1);
                    for (s, v) in sum.iter_mut().zip(pixels[index(row, from)]) {
                        *s += v;
                    }
                }
                *out = sum.map(|s| s / count);
            }
            for (at, value) in line.iter().enumerate() {
                pixels[index(row, at)] = *value;
            }
        }
    }
}

/// `image` averaged down to `side` × `side` pixels.
pub fn shrink(image: &ColorImage, side: usize) -> ColorImage {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        return ColorImage::new([side, side], vec![Color32::BLACK; side * side]);
    }
    let mut pixels = Vec::with_capacity(side * side);
    for y in 0..side {
        let (top, bottom) = (
            y * height / side,
            ((y + 1) * height / side).max(y * height / side + 1),
        );
        for x in 0..side {
            let (left, right) = (
                x * width / side,
                ((x + 1) * width / side).max(x * width / side + 1),
            );
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for row in top..bottom.min(height) {
                for column in left..right.min(width) {
                    let p = image.pixels[row * width + column];
                    r += u32::from(p.r());
                    g += u32::from(p.g());
                    b += u32::from(p.b());
                    n += 1;
                }
            }
            let n = n.max(1);
            pixels.push(Color32::from_rgb(
                (r / n) as u8,
                (g / n) as u8,
                (b / n) as u8,
            ));
        }
    }
    ColorImage::new([side, side], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_averages() {
        let image = ColorImage::new(
            [2, 2],
            vec![
                Color32::from_rgb(0, 0, 0),
                Color32::from_rgb(200, 0, 0),
                Color32::from_rgb(0, 100, 0),
                Color32::from_rgb(0, 0, 40),
            ],
        );
        let one = shrink(&image, 1);
        assert_eq!(one.pixels[0], Color32::from_rgb(50, 25, 10));
        assert_eq!(shrink(&image, 6).size, [6, 6]);
    }

    #[test]
    fn a_wash_keeps_light_and_dark_apart_and_never_glares() {
        // A cover dark on the left and light on the right.
        let pixels = (0..16 * 16)
            .map(|i| {
                if i % 16 < 8 {
                    Color32::from_rgb(10, 20, 40)
                } else {
                    Color32::from_rgb(230, 240, 250)
                }
            })
            .collect();
        let wash = wash(&ColorImage::new([16, 16], pixels));
        assert_eq!(wash.size, [48, 48]);
        let value = |p: Color32| egui::ecolor::HsvaGamma::from(p).v;
        let (left, right) = (wash.pixels[24 * 48], wash.pixels[24 * 48 + 47]);
        // Still darker on the left: not one grey.
        assert!(value(left) + 0.2 < value(right));
        // Nothing brighter than 0.6, so white words read over it.
        assert!(wash.pixels.iter().all(|&p| value(p) <= 0.61));
    }
}
