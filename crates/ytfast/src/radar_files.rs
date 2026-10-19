//! Radar's two files, in YtFast's own data folder, readable by this user
//! only: what the listener did (`listening.json`, kept on the computer and
//! never sent anywhere) and the day's Radar (`radar.json`), so the page
//! shows at once and YouTube is asked again only after a day. A file that
//! cannot be read (none yet, or damaged) starts empty. Not in the demo.

use std::path::PathBuf;

use ytfast_core::radar::{Listening, Radar};

const LISTENING: &str = "listening.json";
const RADAR: &str = "radar.json";

pub struct RadarFiles {
    folder: PathBuf,
}

impl RadarFiles {
    /// YtFast's data folder (`%APPDATA%\YtFast\data` on Windows,
    /// `~/Library/Application Support/YtFast` on a Mac).
    pub fn open() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("", "", "YtFast")?;
        Some(Self::at(dirs.data_dir().to_path_buf()))
    }

    pub fn at(folder: PathBuf) -> Self {
        Self { folder }
    }

    pub fn listening(&self) -> Listening {
        self.read(LISTENING).unwrap_or_default()
    }

    pub fn radar(&self) -> Option<Radar> {
        self.read(RADAR)
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        let text = std::fs::read_to_string(self.folder.join(name)).ok()?;
        serde_json::from_str(&text)
            .inspect_err(|e| {
                log::warn!("Radar: {name} could not be read, so it starts afresh: {e}")
            })
            .ok()
    }

    pub fn save_listening(&self, listening: &Listening) {
        self.write(LISTENING, listening);
    }

    pub fn save_radar(&self, radar: Option<&Radar>) {
        match radar {
            Some(radar) => self.write(RADAR, radar),
            None => {
                let _ = std::fs::remove_file(self.folder.join(RADAR));
            }
        }
    }

    fn write(&self, name: &str, value: &impl serde::Serialize) {
        let written = serde_json::to_vec(value)
            .map_err(std::io::Error::other)
            .and_then(|bytes| fastframe_file::write_private(&self.folder.join(name), bytes));
        if let Err(e) = written {
            log::warn!("Radar: {name} could not be saved: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_keep_what_radar_noted_and_made() {
        let folder =
            std::env::temp_dir().join(format!("ytfast-radar-files-{}", std::process::id()));
        let files = RadarFiles::at(folder.clone());
        // Nothing yet: an empty record, no Radar.
        assert!(files.listening().is_empty());
        assert!(files.radar().is_none());
        let mut listening = Listening::default();
        let track = ytfast_core::read::Track {
            video_id: "a".into(),
            title: "A".into(),
            ..Default::default()
        };
        listening.played(
            &track,
            100.0,
            100.0,
            ytfast_core::radar::Ending::Finished,
            1,
        );
        files.save_listening(&listening);
        let radar = Radar {
            made: 7,
            ..Radar::default()
        };
        files.save_radar(Some(&radar));
        assert_eq!(files.listening(), listening);
        assert_eq!(files.radar(), Some(radar));
        // A damaged file starts afresh.
        std::fs::write(folder.join(LISTENING), "{not json").unwrap();
        assert!(files.listening().is_empty());
        files.save_radar(None);
        assert!(files.radar().is_none());
        let _ = std::fs::remove_dir_all(folder);
    }
}
