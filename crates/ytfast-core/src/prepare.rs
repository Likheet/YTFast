//! Getting a song ready to play: finding its audio, then starting its
//! download and waiting only for the first part. The check program and the
//! app both use this.
//!
//! Finding happens the website's way when it can ([`crate::direct`]: one
//! request, about half a second), and through yt-dlp otherwise (about ten
//! seconds, but it copes with whatever YouTube changes).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio;
use crate::direct::Direct;
use crate::innertube::Session;
use crate::read::PlayerInfo;
use crate::stream::{self, SongData, Source};
use crate::ytdlp::{Resolved, YtDlp};

/// Enough of a song to start playing it: the file's header and its first
/// few seconds.
const START_BYTES: usize = 256 * 1024;
/// The longest the first part may take to arrive.
const START_WAIT: Duration = Duration::from_secs(20);

/// Everything needed to play one song.
pub struct Prepared {
    pub video_id: String,
    /// The song's audio (AAC in MP4), still arriving: the download carries
    /// on in the background.
    pub data: Arc<SongData>,
    /// From [`audio::gain_for_loudness`].
    pub gain: f32,
    pub info: PlayerInfo,
    /// What was chosen, for people: "AAC 256 kbps (format 141)".
    pub format: String,
    pub premium: bool,
    pub duration_seconds: Option<f64>,
    /// How long finding the audio took.
    pub find_time: Duration,
    /// How long the first part took to arrive after that.
    pub start_time: Duration,
    /// Found the website's way, not through yt-dlp.
    pub direct: bool,
    /// Why the fast way was not used, when it was tried and failed.
    pub direct_problem: Option<String>,
    /// Why the song's details could not be read, when they could not.
    /// Playing works without them; History reporting does not.
    pub details_problem: Option<String>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("video_id", &self.video_id)
            .field("data", &self.data)
            .field("format", &self.format)
            .field("direct", &self.direct)
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
    /// The fast way, when it could be set up.
    pub direct: Option<Arc<Direct>>,
}

/// A song's audio, found one way or the other.
struct Located {
    source: Source,
    info: PlayerInfo,
    format: String,
    premium: bool,
    duration_seconds: Option<f64>,
    details_problem: Option<String>,
}

impl Preparer {
    /// Prepares `video_id`: finds its audio and returns once the first part
    /// has arrived. `resolved` skips finding when yt-dlp already did.
    pub async fn prepare(
        &self,
        video_id: &str,
        resolved: Option<Resolved>,
    ) -> Result<Prepared, PrepareError> {
        let started = Instant::now();
        let mut direct_problem = None;
        if resolved.is_none()
            && let Some(direct) = &self.direct
        {
            match direct.find(video_id).await {
                Ok(found) => {
                    let find_time = started.elapsed();
                    let located = Located {
                        source: found.source,
                        info: found.info,
                        format: found.format,
                        premium: found.premium,
                        duration_seconds: found.duration_seconds,
                        details_problem: None,
                    };
                    match self.begin(video_id, located, find_time, true).await {
                        Ok(prepared) => return Ok(prepared),
                        Err(problem) => {
                            // The address stopped working: find it afresh.
                            direct.forget(video_id);
                            direct_problem = Some(problem);
                        }
                    }
                }
                Err(problem) => direct_problem = Some(problem),
            }
            if let Some(problem) = &direct_problem {
                log::warn!("the fast way did not work ({problem}); using yt-dlp");
            }
        }

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
        let located = Located {
            source: Source {
                url: format.url.clone(),
                headers: format.headers.clone(),
                size: format.size,
            },
            info,
            format: format.describe(),
            premium: format.is_premium(),
            duration_seconds: resolved.duration_seconds,
            details_problem,
        };
        let mut prepared = self
            .begin(video_id, located, find_time, false)
            .await
            .map_err(PrepareError::Download)?;
        prepared.direct_problem = direct_problem;
        Ok(prepared)
    }

    /// Starts the download and waits for the first part.
    async fn begin(
        &self,
        video_id: &str,
        located: Located,
        find_time: Duration,
        direct: bool,
    ) -> Result<Prepared, String> {
        let started = Instant::now();
        let data = SongData::new(located.source.size);
        {
            let (http, source, data) = (
                self.download.clone(),
                located.source.clone(),
                Arc::clone(&data),
            );
            tokio::spawn(async move {
                if let Err(e) = stream::fetch(&http, &source, &data).await {
                    log::warn!("a song's download stopped: {e}");
                }
            });
        }
        let waiting = Arc::clone(&data);
        tokio::task::spawn_blocking(move || waiting.wait_for(START_BYTES, START_WAIT))
            .await
            .map_err(|e| e.to_string())??;
        Ok(Prepared {
            video_id: video_id.to_string(),
            data,
            gain: audio::gain_for_loudness(located.info.loudness_db),
            format: located.format,
            premium: located.premium,
            duration_seconds: located.duration_seconds,
            info: located.info,
            find_time,
            start_time: started.elapsed(),
            direct,
            direct_problem: None,
            details_problem: located.details_problem,
        })
    }

    /// Finds a song's audio ahead of time, the fast way only, so playing it
    /// later starts at once. Does nothing without the fast way.
    pub async fn warm(&self, video_id: &str) {
        if let Some(direct) = &self.direct
            && let Err(e) = direct.find(video_id).await
        {
            log::debug!("could not find a song ahead of time: {e}");
        }
    }
}
