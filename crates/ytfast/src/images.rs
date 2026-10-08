//! Covers on screen: asked for once, kept as textures, and let go when
//! not drawn for a while so memory stays small.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::TextureHandle;

use crate::backend::{Backend, Request};

enum Slot {
    Loading,
    Ready(TextureHandle, crate::colors::Summary),
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
}

/// The most memory covers may take (on the graphics card, and on a Mac in
/// the app's own memory). A full-size cover (544 by 544) is about 1.2 MB;
/// the covers on screen at once take about 15 to 25 MB on a 2x screen.
const BUDGET: usize = 32 * 1024 * 1024;
/// The most addresses remembered (failed and loading ones included).
const MAX_ENTRIES: usize = 2000;

impl Images {
    /// Call once per frame, before drawing. Over budget, the covers drawn
    /// longest ago go first; those drawn last frame (on screen) stay.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
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
                    Slot::Ready(texture, _) => Some(texture.clone()),
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

    /// The colours of the cover at `url`, asking for it the first time.
    pub fn summary(&mut self, url: &str, backend: &Backend) -> Option<crate::colors::Summary> {
        let _texture = self.get(url, backend)?;
        match &self.cache.get(url)?.slot {
            Slot::Ready(_, summary) => Some(summary.clone()),
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

    /// A picture arrived from the backend.
    pub fn arrived(
        &mut self,
        ctx: &egui::Context,
        url: String,
        picture: Option<crate::backend::Picture>,
    ) {
        let (slot, bytes) = match picture {
            Some(picture) => {
                let bytes = picture.image.size[0] * picture.image.size[1] * 4;
                let texture = ctx.load_texture(&url, picture.image, egui::TextureOptions::LINEAR);
                (Slot::Ready(texture, picture.summary), bytes)
            }
            None => (Slot::Failed(Instant::now()), 0),
        };
        let used = self.frame;
        if let Some(old) = self.cache.insert(url, Cached { slot, used, bytes }) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }
}
