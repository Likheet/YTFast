//! YTFast's icon by the clock: in the notification area on Windows, in the
//! menu bar on a Mac (fastframe-tray, as Spotifast's). It stays while YTFast
//! runs, its window closed too (Settings, "Keep playing when closed"): a
//! click brings the window back, and its menu plays, pauses, skips and
//! quits. On a Mac a click on the Dock's icon brings the window back too.

use fastframe_tray::{Config, MenuItem, Tray};

/// The menu's entries.
pub const SHOW: &str = "show";
pub const PLAY_PAUSE: &str = "play-pause";
pub const NEXT: &str = "next";
pub const PREVIOUS: &str = "previous";
pub const QUIT: &str = "quit";

/// The most characters a tooltip shows on Windows (its 128, less the end).
const TOOLTIP_MOST: usize = 127;

/// The app's name, as the icon and its menu say it.
pub fn name(demo: bool) -> &'static str {
    if demo { "YTFast demo" } else { "YTFast" }
}

/// Puts the icon by the clock; `wake` is called after each click. None
/// where it cannot be made: closing the window then quits.
pub fn spawn(demo: bool, wake: impl Fn() + Send + Sync + 'static) -> Option<Tray> {
    let name = name(demo);
    Tray::spawn(
        Config {
            id: if demo { "ytfast-demo" } else { "ytfast" },
            title: name.into(),
            icon,
            template_icon: Some(template),
            themed_icon: false,
            menu_on_click: false,
            menu: vec![
                MenuItem::action(SHOW, format!("Show {name}")),
                MenuItem::Separator,
                MenuItem::action(PLAY_PAUSE, "Play").enabled(false),
                MenuItem::action(NEXT, "Next").enabled(false),
                MenuItem::action(PREVIOUS, "Previous").enabled(false),
                MenuItem::Separator,
                MenuItem::action(QUIT, format!("Quit {name}")),
            ],
        },
        wake,
    )
}

/// What the icon's menu and tooltip say, from what plays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The music sounds: the menu says Pause.
    pub sounding: bool,
    /// A song is in the player: Play, Next and Previous can be chosen.
    pub song: bool,
    pub tooltip: String,
}

impl Shown {
    /// Tells the icon what changed since `before`.
    pub fn apply(&self, before: &Self, tray: &mut Tray) {
        if self.sounding != before.sounding {
            tray.set_label(PLAY_PAUSE, if self.sounding { "Pause" } else { "Play" });
        }
        if self.song != before.song {
            for id in [PLAY_PAUSE, NEXT, PREVIOUS] {
                tray.set_enabled(id, self.song);
            }
        }
        if self.tooltip != before.tooltip {
            tray.set_tooltip(self.tooltip.clone());
        }
    }
}

/// The tooltip: the app's name, and under it the song in the player
/// ("Song • Artist"), cut to what Windows shows.
pub fn tooltip(name: &str, song: Option<(&str, &str)>) -> String {
    let text = match song {
        Some((title, "")) => format!("{name}\n{title}"),
        Some((title, artists)) => format!("{name}\n{title} • {artists}"),
        None => name.to_string(),
    };
    if text.chars().count() <= TOOLTIP_MOST {
        return text;
    }
    let mut cut: String = text.chars().take(TOOLTIP_MOST - 1).collect();
    cut.push('…');
    cut
}

/// The app's icon, `size` pixels a side, as RGBA.
fn icon(size: usize) -> Vec<u8> {
    resized(size).into_raw()
}

/// The icon as a Mac's menu bar wants it: in one colour (black, which the
/// Mac recolours to suit the bar), the rounded square with its white play
/// sign and lines cut out.
fn template(size: usize) -> Vec<u8> {
    let mut image = resized(size);
    for pixel in image.pixels_mut() {
        let [_, green, _, alpha] = pixel.0;
        // The square's red has no green; the play sign's white is all of it.
        let kept = u16::from(alpha) * (255 - u16::from(green)) / 255;
        pixel.0 = [0, 0, 0, u8::try_from(kept).unwrap_or(u8::MAX)];
    }
    image.into_raw()
}

/// The app's icon (`assets/icon-256.png`) at `size` pixels a side.
fn resized(size: usize) -> image::RgbaImage {
    let side = u32::try_from(size).unwrap_or(32).max(1);
    match image::load_from_memory_with_format(
        include_bytes!("../assets/icon-256.png"),
        image::ImageFormat::Png,
    ) {
        Ok(icon) => image::imageops::resize(
            &icon.to_rgba8(),
            side,
            side,
            image::imageops::FilterType::Lanczos3,
        ),
        Err(e) => {
            log::warn!("the tray icon could not be drawn: {e}");
            image::RgbaImage::new(side, side)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_names_the_song_and_fits_windows() {
        assert_eq!(tooltip("YTFast", None), "YTFast");
        assert_eq!(
            tooltip("YTFast", Some(("Lemon", "Kenshi Yonezu"))),
            "YTFast\nLemon • Kenshi Yonezu"
        );
        assert_eq!(tooltip("YTFast", Some(("Lemon", ""))), "YTFast\nLemon");
        let long = tooltip("YTFast", Some((&"アイドル".repeat(60), "YOASOBI")));
        assert_eq!(long.chars().count(), TOOLTIP_MOST, "{long}");
        assert!(long.ends_with('…'));
    }

    #[test]
    fn the_icons_are_drawn_at_the_size_asked() {
        for size in [16, 20, 32, 36] {
            assert_eq!(icon(size).len(), size * size * 4);
        }
        // The menu bar's: black only, the square kept, the play sign cut out.
        let template = template(64);
        assert!(template.chunks(4).all(|p| p[..3] == [0, 0, 0]));
        let alpha = |x: usize, y: usize| template[(y * 64 + x) * 4 + 3];
        assert!(alpha(12, 32) > 200, "the square's red, at its left");
        assert!(alpha(40, 32) < 60, "the play sign, cut out");
        assert_eq!(alpha(0, 0), 0, "the corner, outside the square");
    }
}
