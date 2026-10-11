//! Covers on screen: asked for once, kept as textures, and let go when
//! not drawn for a while so memory stays small.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use egui::TextureHandle;

use crate::backend::{Backend, Request};

enum Slot {
    Loading,
    /// The texture, the colours taken from it, and the cover at 24 × 24
    /// (`backend::Picture::soft`).
    Ready(TextureHandle, crate::colors::Summary, egui::ColorImage),
    /// It did not load, at this time.
    Failed(Instant),
}

/// How long a cover that did not load waits before it is asked for again,
/// when it is next on screen (the network may be back).
const RETRY_AFTER: Duration = Duration::from_secs(60);

struct Cached {
    slot: Slot,
    /// The frame it was last drawn in.
    used: u64,
    /// The texture's size in memory.
    bytes: usize,
}

#[derive(Default)]
pub struct Images {
    cache: HashMap<String, Cached>,
    frame: u64,
    /// All textures' size in memory.
    bytes: usize,
    /// Pictures arrived but not yet handed to the graphics card: a little
    /// each frame ([`UPLOAD_PER_FRAME`]), so a page's covers arriving
    /// together do not hold one frame up (a pause in scrolling).
    waiting: VecDeque<(String, crate::backend::Picture)>,
}

/// How much picture goes to the graphics card in one frame, in bytes:
/// about two cards' covers, or a screen of rows'. A larger one goes alone.
const UPLOAD_PER_FRAME: usize = 1024 * 1024;

/// The most memory covers may take (on the graphics card, and on a Mac in
/// the app's own memory). A full-size cover (544 by 544) is about 1.2 MB;
/// the covers on screen at once take about 15 to 25 MB on a 2x screen.
const BUDGET: usize = 32 * 1024 * 1024;
/// The most addresses remembered (failed and loading ones included).
const MAX_ENTRIES: usize = 2000;

impl Images {
    /// Call once per frame, before drawing: pictures that arrived go to
    /// the graphics card, a little at a time; then, over budget, the covers
    /// drawn longest ago go; those drawn last frame (on screen) stay.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.upload(ctx);
        if self.within_limits() {
            return;
        }
        let mut old: Vec<(u64, String)> = self
            .cache
            .iter()
            .filter(|(_, cached)| cached.used + 1 < self.frame)
            .map(|(url, cached)| (cached.used, url.clone()))
            .collect();
        old.sort_unstable();
        for (_, url) in old {
            if self.within_limits() {
                break;
            }
            if let Some(gone) = self.cache.remove(&url) {
                self.bytes -= gone.bytes;
            }
        }
    }

    fn within_limits(&self) -> bool {
        self.bytes <= BUDGET && self.cache.len() <= MAX_ENTRIES
    }

    /// The texture for `url`, asking the backend for it the first time.
    pub fn get(&mut self, url: &str, backend: &Backend) -> Option<TextureHandle> {
        let frame = self.frame;
        match self.cache.get_mut(url) {
            Some(cached) => {
                cached.used = frame;
                if matches!(cached.slot, Slot::Failed(at) if at.elapsed() >= RETRY_AFTER) {
                    cached.slot = Slot::Loading;
                    backend.send(Request::Image(url.to_string()));
                }
                match &cached.slot {
                    Slot::Ready(texture, ..) => Some(texture.clone()),
                    Slot::Loading | Slot::Failed(_) => None,
                }
            }
            None => {
                self.cache.insert(
                    url.to_string(),
                    Cached {
                        slot: Slot::Loading,
                        used: frame,
                        bytes: 0,
                    },
                );
                backend.send(Request::Image(url.to_string()));
                None
            }
        }
    }

    /// Whether the picture at `url` was asked for and did not load. It is
    /// not asked for again here.
    pub fn failed(&mut self, url: &str) -> bool {
        let frame = self.frame;
        self.cache.get_mut(url).is_some_and(|cached| {
            cached.used = frame;
            matches!(cached.slot, Slot::Failed(_))
        })
    }

    /// The colours of the cover at `url`, asking for it the first time.
    pub fn summary(&mut self, url: &str, backend: &Backend) -> Option<crate::colors::Summary> {
        let _texture = self.get(url, backend)?;
        match &self.cache.get(url)?.slot {
            Slot::Ready(_, summary, _) => Some(summary.clone()),
            _ => None,
        }
    }

    /// The cover at `url` at 24 × 24, asking for it the first time.
    pub fn soft(&mut self, url: &str, backend: &Backend) -> Option<egui::ColorImage> {
        let _texture = self.get(url, backend)?;
        match &self.cache.get(url)?.slot {
            Slot::Ready(_, _, soft) => Some(soft.clone()),
            _ => None,
        }
    }

    /// A picture the backend skipped, as it had scrolled past: asked for
    /// again when it next shows.
    pub fn skipped(&mut self, url: &str) {
        if let Some(gone) = self.cache.remove(url) {
            self.bytes -= gone.bytes;
        }
    }

    /// A picture arrived from the backend: it waits for the next frame
    /// ([`Images::begin_frame`]).
    pub fn arrived(&mut self, url: String, picture: Option<crate::backend::Picture>) {
        match picture {
            Some(picture) => self.waiting.push_back((url, picture)),
            None => {
                let used = self.frame;
                let failed = Cached {
                    slot: Slot::Failed(Instant::now()),
                    used,
                    bytes: 0,
                };
                if let Some(old) = self.cache.insert(url, failed) {
                    self.bytes -= old.bytes;
                }
            }
        }
    }

    /// Hands waiting pictures to the graphics card, up to
    /// [`UPLOAD_PER_FRAME`] (at least one); the rest next frame. One let go
    /// meanwhile (scrolled past, or dropped for room) is dropped.
    fn upload(&mut self, ctx: &egui::Context) {
        let mut sent = 0;
        while let Some((_, picture)) = self.waiting.front() {
            let bytes = picture.image.size[0] * picture.image.size[1] * 4;
            if sent > 0 && sent + bytes > UPLOAD_PER_FRAME {
                break;
            }
            let Some((url, picture)) = self.waiting.pop_front() else {
                break;
            };
            let Some(cached) = self.cache.get_mut(&url) else {
                continue;
            };
            sent += bytes;
            let texture = ctx.load_texture(&url, picture.image, egui::TextureOptions::LINEAR);
            cached.slot = Slot::Ready(texture, picture.summary, picture.soft);
            self.bytes = self.bytes - cached.bytes + bytes;
            cached.bytes = bytes;
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Covers that arrive together reach the graphics card a little each
    /// frame (a megabyte at most, one at least), so none holds a frame up
    /// (a pause in scrolling); one let go meanwhile is dropped.
    #[test]
    fn covers_arriving_together_go_up_a_few_each_frame() {
        let ctx = egui::Context::default();
        let (backend, _requests, _events) = Backend::for_tests();
        let mut images = Images::default();
        let cover = || {
            crate::backend::Picture::new(egui::ColorImage::new(
                [339, 339],
                vec![egui::Color32::GRAY; 339 * 339],
            ))
        };
        for n in 0..10 {
            assert!(images.get(&format!("c{n}"), &backend).is_none());
        }
        for n in 0..10 {
            images.arrived(format!("c{n}"), Some(cover()));
        }
        // Scrolled past before it went up: let go.
        images.skipped("c9");
        let ready = |images: &Images| {
            images
                .cache
                .values()
                .filter(|c| matches!(c.slot, Slot::Ready(..)))
                .count()
        };
        let mut shown = Vec::new();
        for _ in 0..6 {
            images.begin_frame(&ctx);
            shown.push(ready(&images));
        }
        // 460 KB each: two a frame.
        assert_eq!(shown, [2, 4, 6, 8, 9, 9]);
        assert!(images.waiting.is_empty());
        // A cover larger than a frame's share still goes, alone.
        assert!(images.get("big", &backend).is_none());
        images.arrived(
            "big".into(),
            Some(crate::backend::Picture::new(egui::ColorImage::new(
                [800, 800],
                vec![egui::Color32::GRAY; 800 * 800],
            ))),
        );
        images.begin_frame(&ctx);
        assert_eq!(ready(&images), 10);
    }
}
