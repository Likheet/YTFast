//! Getting a song ready to play: finding its audio (yt-dlp), asking
//! YouTube for its details (loudness, where to report the play), and
//! downloading it whole. The check program and the app both use this.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio;
use crate::innertube::Session;
use crate::read::PlayerInfo;
use crate::ytdlp::{Resolved, YtDlp};

/// Everything needed to play one song.
pub struct Prepared {
    pub video_id: String,
    /// The whole song, compressed (AAC in MP4).
    pub bytes: Vec<u8>,
    /// From [`audio::gain_for_loudness`].
    pub gain: f32,
    pub info: PlayerInfo,
    /// What was chosen, for people: "AAC 258 kbps (format 141)".
    pub format: String,
    pub premium: bool,
    /// The length yt-dlp reported.
    pub duration_seconds: Option<f64>,
    pub find_time: Duration,
    pub download_time: Duration,
    /// Why the song's details could not be read, when they could not.
    /// Playing works without them; History reporting does not.
    pub details_problem: Option<String>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("video_id", &self.video_id)
            .field("bytes", &self.bytes.len())
            .field("format", &self.format)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("{0}")]
    Find(String),
    #[error("YouTube offered no audio YtFast can play for this song")]
    NoPlayableAudio,
    #[error("{0}")]
    Download(String),
}

/// What preparing needs. Cheap to clone into background tasks.
#[derive(Clone)]
pub struct Preparer {
    pub session: Arc<Session>,
    pub yt_dlp: YtDlp,
    /// [`crate::net::download_client`].
    pub download: reqwest::Client,
    /// The YouTube cookies, as a file for yt-dlp.
    pub cookies_file: PathBuf,
}

impl Preparer {
    /// Prepares `video_id`. `resolved` skips finding the audio when that
    /// was already done. `progress` gets the bytes downloaded so far and
    /// the total when known.
    pub async fn prepare(
        &self,
        video_id: &str,
        resolved: Option<Resolved>,
        progress: &(dyn Fn(u64, Option<u64>) + Sync),
    ) -> Result<Prepared, PrepareError> {
        let started = Instant::now();
        let resolved = match resolved {
            Some(r) => r,
            None => self
                .yt_dlp
                .resolve(video_id, &self.cookies_file)
                .await
                .map_err(|e| PrepareError::Find(e.to_string()))?,
        };
        let find_time = if resolved.took.is_zero() {
            started.elapsed()
        } else {
            resolved.took
        };
        let format = resolved
            .best_playable()
            .cloned()
            .ok_or(PrepareError::NoPlayableAudio)?;

        let (info, details_problem) = match self.session.player(video_id).await {
            Ok(info) => (info, None),
            Err(e) => (PlayerInfo::default(), Some(e.to_string())),
        };
        let downloaded = audio::download(&self.download, &format, progress)
            .await
            .map_err(|e| PrepareError::Download(e.to_string()))?;
        Ok(Prepared {
            video_id: video_id.to_string(),
            bytes: downloaded.bytes,
            gain: audio::gain_for_loudness(info.loudness_db),
            format: format.describe(),
            premium: format.is_premium(),
            duration_seconds: resolved.duration_seconds,
            info,
            find_time,
            download_time: downloaded.took,
            details_problem,
        })
    }
}
