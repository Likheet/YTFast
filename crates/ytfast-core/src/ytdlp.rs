//! Running yt-dlp: to read the YouTube sign-in from a browser, and to find
//! the audio of a song for the signed-in account.
//!
//! yt-dlp runs for a moment per song and exits, so it holds no memory while
//! music plays. Its output is read as JSON; its error text is shortened and
//! stripped of addresses before anyone sees it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::cookies::CookieJar;
use crate::helpers::Helpers;
use crate::redact;

/// Browsers yt-dlp can read cookies from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Firefox,
    Safari,
    Edge,
    Brave,
}

impl Browser {
    pub const ALL: [Browser; 5] = [
        Self::Chrome,
        Self::Firefox,
        Self::Safari,
        Self::Edge,
        Self::Brave,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Firefox => "Firefox",
            Self::Safari => "Safari",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "Brave",
        }
    }

    fn arg(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Firefox => "firefox",
            Self::Safari => "safari",
            Self::Edge => "edge",
            Self::Brave => "brave",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| {
            b.arg().eq_ignore_ascii_case(name.trim()) || b.label().eq_ignore_ascii_case(name.trim())
        })
    }

    /// Why this browser cannot work on this computer, if it cannot. The
    /// app shows this, and it reads browsers only; the check adds its own
    /// way round it (a cookies.txt file).
    pub fn problem_here(self) -> Option<&'static str> {
        match self {
            // Chrome-family browsers on Windows encrypt cookies in a way only
            // the browser itself can undo (app-bound encryption), and yt-dlp
            // only reads the older format there.
            Self::Chrome | Self::Edge | Self::Brave if cfg!(windows) => Some(
                "On Windows, Chrome, Edge and Brave lock their sign-in data so other programs cannot read it. Use Firefox.",
            ),
            Self::Safari if !cfg!(target_os = "macos") => Some("Safari is only on Macs."),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum YtDlpError {
    #[error("yt-dlp could not be started: {0}")]
    Start(String),
    #[error("yt-dlp took longer than {0} seconds and was stopped")]
    TimedOut(u64),
    #[error("{summary}")]
    Failed {
        summary: String,
        /// The last lines yt-dlp printed, without addresses.
        details: String,
    },
}

impl YtDlpError {
    /// yt-dlp found the sign-in it was given no longer accepted. A browser
    /// renews its cookies as it goes, which ends an earlier copy's;
    /// reading the browser's again mends it.
    pub fn sign_in_expired(&self) -> bool {
        matches!(self, Self::Failed { details, .. } if says_sign_in_expired(&details.to_ascii_lowercase()))
    }

    /// yt-dlp found that only this song cannot be played here (removed,
    /// private, for a channel's members, not offered in this country), not
    /// something every song would run into.
    pub fn song_unavailable(&self) -> bool {
        matches!(self, Self::Failed { details, .. } if says_song_unavailable(&details.to_ascii_lowercase()))
    }

    /// yt-dlp could not find a browser's sign-in data, or was not allowed
    /// to read it. On a Mac without Full Disk Access the system hides other
    /// apps' data, which reads as missing too
    /// ([`crate::cookies::full_disk_access`]).
    pub fn browser_data_unreadable(&self) -> bool {
        matches!(self, Self::Failed { details, .. } if says_browser_data_unreadable(&details.to_ascii_lowercase()))
    }
}

/// yt-dlp's words (in lower case) for a browser's data it could not find or
/// was not allowed to read.
fn says_browser_data_unreadable(lower: &str) -> bool {
    (lower.contains("could not find") && lower.contains("cookies database"))
        || lower.contains("operation not permitted")
        || lower.contains("permission denied")
}

/// yt-dlp's words (in lower case) for cookies YouTube stopped accepting.
fn says_sign_in_expired(lower: &str) -> bool {
    lower.contains("no longer valid") || lower.contains("rotated")
}

/// yt-dlp's words (in lower case) for a problem every song would have:
/// YouTube slowing the account down, or audio yt-dlp could not get (after
/// a change at YouTube that a newer yt-dlp mends).
fn says_every_song(lower: &str) -> bool {
    [
        "try again later",
        "rate-limit",
        "rate limit",
        "http error 429",
        "not a bot",
        "requested format",
    ]
    .iter()
    .any(|words| lower.contains(words))
}

/// yt-dlp's words (in lower case) for one song that cannot be played here.
fn says_song_unavailable(lower: &str) -> bool {
    !says_every_song(lower)
        && [
            "video unavailable",
            "is not available",
            "no longer available",
            "private video",
            "members-only",
            "has been removed",
            "in your country",
            "confirm your age",
        ]
        .iter()
        .any(|words| lower.contains(words))
}

/// One audio-only stream yt-dlp found.
#[derive(Clone)]
pub struct AudioFormat {
    pub format_id: String,
    /// YouTube's number for the format (`140` is AAC 128 kbps, `141` AAC
    /// 256 kbps for Premium, `251` Opus, `774` Opus 256 kbps for Premium).
    pub itag: Option<u32>,
    pub ext: String,
    pub codec: String,
    pub kbps: Option<f64>,
    pub size: Option<u64>,
    pub note: String,
    pub has_drm: bool,
    /// Dynamic range compressed: a quieter, flattened copy. Avoided.
    pub is_drc: bool,
    /// The stream address. Short-lived and tied to this session; never shown.
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioFormat")
            .field("format_id", &self.format_id)
            .field("codec", &self.codec)
            .field("kbps", &self.kbps)
            .field("size", &self.size)
            .finish()
    }
}

impl AudioFormat {
    /// Premium-only quality (256 kbps).
    pub fn is_premium(&self) -> bool {
        matches!(self.itag, Some(141 | 774)) || self.note.to_ascii_lowercase().contains("premium")
    }

    /// AAC in MP4, which YtFast's decoder plays.
    pub fn is_aac(&self) -> bool {
        self.codec.starts_with("mp4a") || self.ext == "m4a"
    }

    pub fn describe(&self) -> String {
        let codec = if self.is_aac() {
            "AAC"
        } else if self.codec.contains("opus") {
            "Opus"
        } else {
            self.codec.as_str()
        };
        match self.kbps {
            Some(kbps) => format!("{codec} {kbps:.0} kbps (format {})", self.format_id),
            None => format!("{codec} (format {})", self.format_id),
        }
    }
}

/// What yt-dlp found for one song.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub video_id: String,
    pub duration_seconds: Option<f64>,
    /// Audio-only streams, without DRM.
    pub formats: Vec<AudioFormat>,
    pub took: Duration,
}

impl Resolved {
    /// The stream to play: Premium AAC (256 kbps) when there is one, else
    /// the standard AAC (128 kbps). Opus is not played yet.
    pub fn best_playable(&self) -> Option<&AudioFormat> {
        let mut candidates: Vec<&AudioFormat> = self
            .formats
            .iter()
            .filter(|f| f.is_aac() && !f.has_drm)
            .collect();
        candidates.sort_by(|a, b| {
            let key = |f: &AudioFormat| (!f.is_drc, f.is_premium(), f.kbps.unwrap_or(0.0) as u32);
            key(b).cmp(&key(a))
        });
        candidates.into_iter().next()
    }

    pub fn premium_formats(&self) -> Vec<&AudioFormat> {
        self.formats.iter().filter(|f| f.is_premium()).collect()
    }
}

/// yt-dlp, set up to use YtFast's Deno and cache folder.
#[derive(Clone, Debug)]
pub struct YtDlp {
    exe: PathBuf,
    deno: PathBuf,
    cache_dir: PathBuf,
}

impl YtDlp {
    pub fn new(helpers: &Helpers, cache_dir: PathBuf) -> Self {
        Self {
            exe: helpers.yt_dlp.clone(),
            deno: helpers.deno.clone(),
            cache_dir,
        }
    }

    fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.exe);
        command
            // No 11-character arguments starting with "-" (like
            // "--cache-dir"): yt-dlp mistakes them for video IDs and warns.
            .args(["--ignore-config", "--color", "never", "--no-progress"])
            .arg(format!("--cache-dir={}", self.cache_dir.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        no_console_window(&mut command);
        command
    }

    async fn run(
        &self,
        mut command: tokio::process::Command,
        limit: Duration,
    ) -> Result<std::process::Output, YtDlpError> {
        let child = command
            .spawn()
            .map_err(|e| YtDlpError::Start(e.to_string()))?;
        match tokio::time::timeout(limit, child.wait_with_output()).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(YtDlpError::Start(e.to_string())),
            Err(_) => Err(YtDlpError::TimedOut(limit.as_secs())),
        }
    }

    /// `yt-dlp --version`, to prove it runs on this computer.
    pub async fn version(&self) -> Result<String, YtDlpError> {
        let mut command = tokio::process::Command::new(&self.exe);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        no_console_window(&mut command);
        let output = self.run(command, Duration::from_secs(60)).await?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if output.status.success() && !text.is_empty() {
            Ok(text)
        } else {
            Err(failure("yt-dlp did not start properly", &output.stderr))
        }
    }

    /// Reads the browser's cookies and keeps YouTube's. yt-dlp writes the
    /// whole browser jar into `scratch` (a private folder); it is read and
    /// deleted at once, and only YouTube's cookies are returned. `profile`
    /// names a browser profile (Chrome's "Profile 1"), or the usual one.
    pub async fn read_browser_sign_in(
        &self,
        browser: Browser,
        profile: Option<&str>,
        scratch: &Path,
    ) -> Result<CookieJar, YtDlpError> {
        let out = scratch.join("browser-cookies.txt");
        let _ = std::fs::remove_file(&out);
        let source = match profile.map(str::trim).filter(|p| !p.is_empty()) {
            Some(profile) => format!("{}:{profile}", browser.arg()),
            None => browser.arg().to_string(),
        };
        let mut command = self.command();
        // No address: yt-dlp only loads the cookies, saves them to `out` on
        // its way out, and complains that there was nothing to download.
        command
            .arg("--cookies-from-browser")
            .arg(source)
            .arg("--cookies")
            .arg(&out);
        let result = self.run(command, Duration::from_secs(120)).await;
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        let output = result?;
        let jar = CookieJar::parse_netscape(&text).youtube_only();
        if jar.is_empty() {
            return Err(failure(
                &format!("No YouTube sign-in was found in {}", browser.label()),
                &output.stderr,
            ));
        }
        Ok(jar)
    }

    /// Finds the audio streams of `video_id` for the account in
    /// `cookies_file` (a Netscape file of YouTube cookies only).
    pub async fn resolve(
        &self,
        video_id: &str,
        cookies_file: &Path,
    ) -> Result<Resolved, YtDlpError> {
        if !is_video_id(video_id) {
            return Err(YtDlpError::Failed {
                summary: "That is not a YouTube song ID".into(),
                details: String::new(),
            });
        }
        let started = Instant::now();
        // yt-dlp writes its cookies back to the file it was given as it
        // finishes, which would put back a sign-in older than one read
        // again meanwhile: it gets a copy of its own, deleted afterwards.
        let cookies = private_copy(cookies_file).map_err(|e| {
            YtDlpError::Start(format!("the sign-in could not be handed to yt-dlp: {e}"))
        })?;
        let mut command = self.command();
        command
            .args(["-J", "--no-playlist", "--js-runtimes"])
            .arg(format!("deno:{}", self.deno.display()))
            .arg("--cookies")
            .arg(cookies.path())
            .arg("--")
            .arg(format!("https://music.youtube.com/watch?v={video_id}"));
        let output = self.run(command, Duration::from_secs(120)).await;
        drop(cookies);
        let output = output?;
        if !output.status.success() {
            return Err(failure("yt-dlp could not get this song", &output.stderr));
        }
        let json: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| YtDlpError::Failed {
                summary: format!("yt-dlp's answer could not be read ({e})"),
                details: tail(&output.stderr),
            })?;
        let mut resolved = parse_info(&json);
        resolved.video_id = video_id.to_string();
        resolved.took = started.elapsed();
        Ok(resolved)
    }
}

/// A copy of a cookie file beside it, readable only by this user (as
/// `tempfile` makes files), deleted when dropped.
fn private_copy(file: &Path) -> std::io::Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let folder = file.parent().unwrap_or_else(|| Path::new("."));
    let mut copy = tempfile::Builder::new()
        .prefix("yt-dlp-cookies-")
        .suffix(".txt")
        .tempfile_in(folder)?;
    copy.write_all(&std::fs::read(file)?)?;
    Ok(copy)
}

/// On Windows, a program started from a windowed app opens a console
/// window of its own unless told not to.
pub(crate) fn no_console_window(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// YouTube video IDs are 11 characters of `A-Z a-z 0-9 - _`. Checking keeps
/// anything else away from yt-dlp's command line.
pub fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reads yt-dlp's `-J` output.
pub fn parse_info(json: &Value) -> Resolved {
    let formats = json
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_format)
        .collect();
    Resolved {
        video_id: json
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        duration_seconds: json.get("duration").and_then(Value::as_f64),
        formats,
        took: Duration::ZERO,
    }
}

fn parse_format(f: &Value) -> Option<AudioFormat> {
    let s = |k: &str| {
        f.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // Audio only: no video codec.
    let vcodec = s("vcodec");
    if !(vcodec.is_empty() || vcodec == "none") {
        return None;
    }
    let codec = s("acodec");
    if codec.is_empty() || codec == "none" {
        return None;
    }
    let url = s("url");
    if !url.starts_with("https://") {
        return None;
    }
    let protocol = s("protocol");
    if !protocol.is_empty() && protocol != "https" {
        // Manifests (DASH, HLS) need a different downloader.
        return None;
    }
    let format_id = s("format_id");
    let itag = format_id.split('-').next().and_then(|n| n.parse().ok());
    let headers = f
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|h| {
            h.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(AudioFormat {
        is_drc: format_id.ends_with("-drc"),
        itag,
        format_id,
        ext: s("ext"),
        codec,
        kbps: f
            .get("abr")
            .and_then(Value::as_f64)
            .or_else(|| f.get("tbr").and_then(Value::as_f64)),
        // Only the exact size: an estimate (`filesize_approx`) would cut
        // the song short or ask past its end. Without it, the server says.
        size: f.get("filesize").and_then(Value::as_u64),
        note: s("format_note"),
        has_drm: f.get("has_drm").and_then(Value::as_bool).unwrap_or(false),
        url,
        headers,
    })
}

fn tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(8);
    redact::urls(&lines[start..].join("\n"))
}

/// A failure with a plain-language summary chosen from what yt-dlp said.
fn failure(default: &str, stderr: &[u8]) -> YtDlpError {
    let details = tail(stderr);
    let lower = details.to_ascii_lowercase();
    let summary = if lower.contains("not a bot") {
        "YouTube asked to confirm this is not a bot. Open music.youtube.com in your browser, play a song there, then try again"
    } else if says_sign_in_expired(&lower) {
        "YouTube no longer accepts the saved sign-in. Sign in to music.youtube.com in your browser again"
    } else if lower.contains("could not find") && lower.contains("cookies database") {
        "That browser's sign-in data was not found. Is it installed, and have you opened music.youtube.com in it?"
    } else if lower.contains("operation not permitted") || lower.contains("permission denied") {
        "This computer did not allow reading that browser's data. On a Mac, YTFast needs Full Disk Access: in System Settings, Privacy & Security, Full Disk Access, turn on YTFast (or Terminal, for the check). Or choose another browser"
    } else if lower.contains("keyring") || lower.contains("keychain") || lower.contains("decrypt") {
        "The browser's sign-in data could not be unlocked. On a Mac, click Always Allow when asked about the browser's Safe Storage: YTFast reads the sign-in again by itself from time to time, and would otherwise ask each time"
    } else if lower.contains("javascript runtime") {
        "yt-dlp could not use Deno to answer YouTube's challenge"
    } else if lower.contains("requested format") {
        "yt-dlp could not get this song's audio. If this keeps happening, quit YTFast and open it again: it then gets the newest yt-dlp"
    } else if says_every_song(&lower) {
        "YouTube asked YTFast to slow down. Wait a while, then try again"
    } else if says_song_unavailable(&lower) {
        "YouTube says this song is not available to this account"
    } else {
        default
    };
    YtDlpError::Failed {
        summary: summary.to_string(),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info() -> Value {
        json!({
            "id": "abcdefghijk",
            "duration": 214.0,
            "formats": [
                { "format_id": "139", "ext": "m4a", "acodec": "mp4a.40.5", "vcodec": "none", "abr": 48.8, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=1" },
                { "format_id": "140", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 129.5, "filesize": 3_400_000, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=2", "http_headers": { "User-Agent": "UA" } },
                { "format_id": "140-drc", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 129.5, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=3" },
                { "format_id": "141", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 255.8, "format_note": "high, Premium", "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=4" },
                { "format_id": "251", "ext": "webm", "acodec": "opus", "vcodec": "none", "abr": 135.0, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=5" },
                { "format_id": "774", "ext": "webm", "acodec": "opus", "vcodec": "none", "abr": 256.0, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=6" },
                { "format_id": "18", "ext": "mp4", "acodec": "mp4a.40.2", "vcodec": "avc1.42001E", "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=7" },
                { "format_id": "233", "ext": "mp4", "acodec": "mp4a.40.2", "vcodec": "none", "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/a.m3u8" },
                { "format_id": "999", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 300.0, "has_drm": true, "protocol": "https", "url": "https://rr1.googlevideo.com/videoplayback?a=8" }
            ]
        })
    }

    #[test]
    fn keeps_plain_audio_streams_only() {
        let resolved = parse_info(&info());
        let ids: Vec<&str> = resolved
            .formats
            .iter()
            .map(|f| f.format_id.as_str())
            .collect();
        assert_eq!(ids, ["139", "140", "140-drc", "141", "251", "774", "999"]);
        assert_eq!(resolved.duration_seconds, Some(214.0));
    }

    #[test]
    fn prefers_premium_aac_and_avoids_drc_and_drm() {
        let resolved = parse_info(&info());
        assert_eq!(resolved.best_playable().unwrap().format_id, "141");
        let premium: Vec<&str> = resolved
            .premium_formats()
            .iter()
            .map(|f| f.format_id.as_str())
            .collect();
        assert_eq!(premium, ["141", "774"]);

        let mut standard = info();
        standard["formats"]
            .as_array_mut()
            .unwrap()
            .retain(|f| f["format_id"] != "141");
        let resolved = parse_info(&standard);
        let best = resolved.best_playable().unwrap();
        assert_eq!(best.format_id, "140");
        assert_eq!(best.headers, [("User-Agent".to_string(), "UA".to_string())]);
    }

    #[test]
    fn stream_addresses_are_never_shown() {
        let resolved = parse_info(&info());
        assert!(!format!("{resolved:?}").contains("googlevideo"));
    }

    #[test]
    fn video_ids_are_checked() {
        assert!(is_video_id("dQw4w9WgXcQ"));
        assert!(is_video_id("3_R4ulvg8OY"));
        assert!(!is_video_id("--exec=bad"));
        assert!(!is_video_id("short"));
        assert!(!is_video_id("abc def ghi"));
    }

    #[test]
    fn explains_common_failures() {
        let err = failure("default", b"ERROR: [youtube] x: Sign in to confirm you\xe2\x80\x99re not a bot. https://x.example/?t=1");
        let YtDlpError::Failed { summary, details } = err else {
            panic!()
        };
        assert!(summary.contains("not a bot"));
        assert!(!details.contains("t=1"));
        let err = failure(
            "default",
            b"ERROR: could not find chrome cookies database in \"/x\"",
        );
        assert!(err.to_string().contains("not found"));
        assert!(!err.sign_in_expired());
        assert!(err.browser_data_unreadable());
        let err = failure(
            "default",
            b"ERROR: [Errno 1] Operation not permitted: '/Users/x/Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies'",
        );
        assert!(err.browser_data_unreadable());
        assert!(err.to_string().contains("Full Disk Access"));
        assert!(!failure("default", b"something else").browser_data_unreadable());
        let err = failure(
            "default",
            b"ERROR: [youtube] x: The provided YouTube account cookies are no longer valid. They have likely been rotated in the browser as a security measure.",
        );
        assert!(err.sign_in_expired());
        assert!(err.to_string().contains("no longer accepts"));
        assert_eq!(failure("default", b"something else").to_string(), "default");
    }

    #[test]
    fn tells_one_songs_problem_from_every_songs() {
        let one_song = |stderr: &[u8]| failure("default", stderr).song_unavailable();
        assert!(one_song(
            b"ERROR: [youtube] abc: Video unavailable. This video has been removed by the uploader"
        ));
        assert!(one_song(
            b"ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video"
        ));
        assert!(one_song(
            b"ERROR: [youtube] abc: Join this channel to get access to members-only content like this video"
        ));
        assert!(one_song(
            b"ERROR: [youtube] abc: The uploader has not made this video available in your country"
        ));
        // Every song would fail the same way: the queue must not skip
        // through them.
        assert!(!one_song(
            b"ERROR: [youtube] abc: Requested format is not available. Use --list-formats for a list of available formats"
        ));
        assert!(!one_song(
            b"ERROR: [youtube] abc: Video unavailable. This content isn't available, try again later."
        ));
        assert!(!one_song(
            b"ERROR: [youtube] abc: Sign in to confirm you\xe2\x80\x99re not a bot."
        ));
        assert!(!one_song(b"something else"));
        let private = failure("default", b"ERROR: [youtube] abc: Private video.");
        assert!(
            private
                .to_string()
                .contains("not available to this account")
        );
        let format = failure(
            "default",
            b"ERROR: [youtube] abc: Requested format is not available.",
        );
        assert!(format.to_string().contains("newest yt-dlp"));
    }

    #[test]
    fn browsers_by_name() {
        assert_eq!(Browser::parse("firefox"), Some(Browser::Firefox));
        assert_eq!(Browser::parse("Google Chrome"), Some(Browser::Chrome));
        assert_eq!(Browser::parse("netscape"), None);
    }
}
