//! Covers on screen: asked for once, kept as textures, and let go when
//! not drawn for a while so memory stays small.

use std::collections::HashMap;

use egui::TextureHandle;

use crate::backend::{Backend, Request};

enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

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

/// The most memory covers may take. A full-size cover is about 0.5 MB.
const BUDGET: usize = 64 * 1024 * 1024;
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
                match &cached.slot {
                    Slot::Ready(texture) => Some(texture.clone()),
                    Slot::Loading | Slot::Failed => None,
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

    /// A picture arrived from the backend.
    pub fn arrived(&mut self, ctx: &egui::Context, url: String, picture: Option<egui::ColorImage>) {
        let (slot, bytes) = match picture {
            Some(picture) => {
                let bytes = picture.size[0] * picture.size[1] * 4;
                let texture = ctx.load_texture(&url, picture, egui::TextureOptions::LINEAR);
                (Slot::Ready(texture), bytes)
            }
            None => (Slot::Failed, 0),
        };
        let used = self.frame;
        if let Some(old) = self.cache.insert(url, Cached { slot, used, bytes }) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }
}
