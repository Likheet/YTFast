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
}

#[derive(Default)]
pub struct Images {
    cache: HashMap<String, Cached>,
    frame: u64,
}

/// Textures kept at most; the least recently drawn go first.
const KEEP: usize = 400;

impl Images {
    /// Call once per frame.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.cache.len() > KEEP && self.frame.is_multiple_of(120) {
            let mut by_use: Vec<(u64, String)> = self
                .cache
                .iter()
                .map(|(k, v)| (v.used, k.clone()))
                .collect();
            by_use.sort();
            for (_, key) in by_use.into_iter().take(self.cache.len() - KEEP) {
                self.cache.remove(&key);
            }
        }
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
                    },
                );
                backend.send(Request::Image(url.to_string()));
                None
            }
        }
    }

    /// A picture arrived from the backend.
    pub fn arrived(&mut self, ctx: &egui::Context, url: String, picture: Option<egui::ColorImage>) {
        let slot = match picture {
            Some(picture) => {
                Slot::Ready(ctx.load_texture(&url, picture, egui::TextureOptions::LINEAR))
            }
            None => Slot::Failed,
        };
        let used = self.frame;
        self.cache.insert(url, Cached { slot, used });
    }
}
