//! The settings and the queue kept for the next run, in the file eframe
//! keeps them in (`app.ron`, keys `ytfast` and `ytfast-queue`). eframe reads
//! it as a window opens and writes it while one is open. YTFast runs on with
//! no window (closed while it plays, or started with the computer), so it
//! reads the file itself before any window opens, and writes it itself while
//! none is: in eframe's own format, a RON map of keys to RON text, every
//! other key (the window's size and place) kept as it was.

use std::collections::HashMap;
use std::path::PathBuf;

/// The name eframe keeps the real app's file under (the window's app id).
pub const APP_ID: &str = "ytfast";

/// The file the settings and the queue are kept in.
pub struct Store {
    path: PathBuf,
    /// The demo reads its own file (`demo-window.ron`, where a look can be
    /// chosen for it) and writes nothing.
    read_only: bool,
}

impl Store {
    /// The real app's file (eframe's, by the app id), or the demo's own.
    pub fn for_app(demo: bool) -> Option<Self> {
        let path = if demo {
            directories::ProjectDirs::from("", "", "YtFast")?
                .cache_dir()
                .join("demo-window.ron")
        } else {
            eframe::storage_dir(APP_ID)?.join("app.ron")
        };
        Some(Self {
            path,
            read_only: demo,
        })
    }

    /// A file of the tests' own.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            read_only: false,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// The value kept under `key`, read as `eframe::get_value` reads it.
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let text = self.read().remove(key)?;
        ron::from_str(&text)
            .inspect_err(|e| log::warn!("the saved {key} could not be read: {e}"))
            .ok()
    }

    /// Keeps `values` (each written by [`text`]) in the file, beside what
    /// it holds, the whole file replaced at once (never half written).
    pub fn set(&self, values: &[(&str, String)]) {
        if self.read_only {
            return;
        }
        let mut kept = self.read();
        for (key, value) in values {
            kept.insert((*key).to_owned(), value.clone());
        }
        let written = ron::ser::to_string_pretty(&kept, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)
            .and_then(|text| fastframe_file::write(&self.path, text));
        if let Err(e) = written {
            log::warn!("the settings could not be saved: {e}");
        }
    }

    /// Every key and its RON text; nothing when the file is missing or
    /// cannot be read (eframe then starts from nothing too).
    fn read(&self) -> HashMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }
}

/// A value as `eframe::set_value` writes it.
pub fn text<T: serde::Serialize>(value: &T) -> Option<String> {
    ron::ser::to_string(value)
        .inspect_err(|e| log::warn!("a setting could not be written: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What eframe wrote is read, and writing keeps what eframe keeps
    /// beside it (the window's place), in a file eframe reads back.
    #[test]
    fn the_file_is_read_and_written_as_eframe_does() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.ron");
        // As eframe writes it: a map of keys to RON text.
        let eframe_wrote: HashMap<String, String> = [
            (
                "window".to_string(),
                "(inner_size: Some((1280, 820)))".to_string(),
            ),
            ("ytfast".to_string(), "(volume: 0.5)".to_string()),
        ]
        .into();
        std::fs::write(
            &path,
            ron::ser::to_string_pretty(&eframe_wrote, ron::ser::PrettyConfig::default()).unwrap(),
        )
        .unwrap();

        let store = Store::at(path.clone());
        let settings: crate::app::Settings = store.get("ytfast").unwrap();
        assert!((settings.volume - 0.5).abs() < f32::EPSILON);
        assert!(store.get::<crate::app::Settings>("missing").is_none());

        store.set(&[
            ("ytfast-queue", text(&Some(7_u32)).unwrap()),
            ("ytfast", text(&crate::app::Settings::default()).unwrap()),
        ]);
        let read: HashMap<String, String> =
            ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read["window"], "(inner_size: Some((1280, 820)))");
        assert_eq!(store.get::<Option<u32>>("ytfast-queue"), Some(Some(7)));
        let settings: crate::app::Settings = store.get("ytfast").unwrap();
        assert!((settings.volume - 0.8).abs() < f32::EPSILON);
    }

    /// The demo's file is read but never written.
    #[test]
    fn the_demo_writes_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store {
            path: folder.path().join("demo-window.ron"),
            read_only: true,
        };
        store.set(&[("ytfast", "()".to_string())]);
        assert!(!store.path().exists());
    }
}
