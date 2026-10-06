//! Colours from album art: the soft backdrop behind the window and the
//! accent colour, as Even Better Lyrics Plus takes them from the cover.

use egui::{Color32, ColorImage, ecolor::HsvaGamma};

/// What the look takes from a cover.
#[derive(Clone)]
pub struct Summary {
    /// The cover shrunk to a few pixels: drawn stretched over the window,
    /// it becomes a soft blur of the cover's colours.
    pub tiny: ColorImage,
    /// The cover's most vivid colour, made light enough to read on dark.
    pub accent: Color32,
}

/// The side of [`Summary::tiny`].
const TINY: usize = 6;
/// The side sampled for the accent colour.
const SAMPLE: usize = 24;

/// Summarises a cover.
pub fn summarize(image: &ColorImage) -> Summary {
    Summary {
        tiny: shrink(image, TINY),
        accent: accent(&shrink(image, SAMPLE)),
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

/// The most vivid colour, lightened for dark backgrounds. A grey cover
/// gives a soft white.
pub fn accent(sample: &ColorImage) -> Color32 {
    let best = sample
        .pixels
        .iter()
        // Measured as eyes see colour (gamma), not as light adds up.
        .map(|p| HsvaGamma::from(*p))
        .filter(|c| c.v > 0.2)
        .max_by(|a, b| (a.s * a.v).total_cmp(&(b.s * b.v)));
    match best {
        Some(c) if c.s > 0.18 => Color32::from(HsvaGamma {
            h: c.h,
            s: c.s.clamp(0.35, 0.75),
            v: c.v.max(0.9),
            a: 1.0,
        }),
        _ => Color32::from_rgb(0xec, 0xec, 0xf1),
    }
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
    fn accents_are_vivid_and_light() {
        let red = ColorImage::new(
            [2, 1],
            vec![
                Color32::from_rgb(120, 10, 10),
                Color32::from_rgb(40, 40, 40),
            ],
        );
        let accent = HsvaGamma::from(super::accent(&red));
        assert!(accent.v >= 0.89 && accent.s >= 0.35, "{accent:?}");
        let grey = ColorImage::new([1, 1], vec![Color32::from_rgb(90, 90, 90)]);
        assert_eq!(super::accent(&grey), Color32::from_rgb(0xec, 0xec, 0xf1));
    }
}
