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
}

/// The side of [`Summary::tiny`].
const TINY: usize = 6;

/// Summarises a cover.
pub fn summarize(image: &ColorImage) -> Summary {
    Summary {
        tiny: shrink(image, TINY),
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
}
