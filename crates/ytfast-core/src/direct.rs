//! Finding a song's audio the way music.youtube.com does: one `player`
//! request, the stream address unlocked with YouTube's own player code
//! (kept ready in [`Solver`]), no yt-dlp. About half a second instead of
//! ten. yt-dlp stays as the fallback when this fails ([`crate::prepare`]).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::innertube::Session;
use crate::read::{self, PlayerInfo, StreamFormat};
use crate::redact;
use crate::solver::{Answers, PlayerCode, Solver};
use crate::stream::Source;

/// A song's audio, found.
#[derive(Clone)]
pub struct Found {
    pub source: Source,
    pub info: PlayerInfo,
    /// For people: "AAC 256 kbps (format 141)".
    pub format: String,
    pub premium: bool,
    pub duration_seconds: Option<f64>,
    /// How long finding took: zero when the song was found earlier and
    /// remembered.
    pub took: Duration,
}

/// Stream addresses work for about six hours; found songs are reused for
/// less than that.
const KEEP_FOUND: Duration = Duration::from_secs(3 * 3600);
/// Found songs remembered at most (songs pointed at, top results).
const MAX_FOUND: usize = 40;
/// When a player could not be got ready, the fast way rests this long
/// before trying it again (songs come through yt-dlp meanwhile), rather
/// than spending seconds failing the same way on every song.
const REST_AFTER_FAILURE: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
struct Player {
    id: String,
    /// The signature timestamp the `player` request must carry for the
    /// addresses to unlock with this player.
    sts: u64,
}

/// A player that could not be got ready.
struct Failed {
    player: String,
    at: Instant,
    why: String,
}

pub struct Direct {
    session: Arc<Session>,
    solver: Solver,
    /// Where player code is kept between runs.
    cache: PathBuf,
    player: Mutex<Option<Player>>,
    failed: std::sync::Mutex<Option<Failed>>,
    found: std::sync::Mutex<HashMap<String, (Instant, Found)>>,
}

impl Direct {
    pub fn new(session: Arc<Session>, solver: Solver, cache: PathBuf) -> Self {
        Self {
            session,
            solver,
            cache,
            player: Mutex::new(None),
            failed: std::sync::Mutex::new(None),
            found: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Gets YouTube's player code and the solver ready, so the first song
    /// is quick too. Called once after signing in; safe to call again.
    pub async fn warm_up(&self) -> Result<(), String> {
        self.player(true).await.map(|_| ())
    }

    /// Stops the solver when idle (see [`Solver::stop_if_idle`]).
    pub async fn rest_if_idle(&self, idle: Duration) {
        self.solver.stop_if_idle(idle).await;
    }

    /// The player, loaded in the solver. While it is being got ready,
    /// others `wait` for it, or are turned away at once.
    async fn player(&self, wait: bool) -> Result<Player, String> {
        let mut current = if wait {
            self.player.lock().await
        } else {
            self.player
                .try_lock()
                .map_err(|_| "the fast way is busy getting ready")?
        };
        let url = self
            .session
            .player_js_url()
            .ok_or("YouTube Music's page did not say where its player is")?;
        let id = player_id(&url).ok_or("an unexpected player address")?;
        if let Some(player) = current.as_ref().filter(|p| p.id == id)
            && self.solver.loaded().await.as_deref() == Some(id.as_str())
        {
            return Ok(player.clone());
        }
        if let Some(why) = self.resting(&id) {
            return Err(why);
        }
        let sts = self.get_ready(&id, &url).await?;
        let player = Player { id, sts };
        *current = Some(player.clone());
        Ok(player)
    }

    /// Why the fast way is resting with player `id`, if it is (see
    /// [`REST_AFTER_FAILURE`]).
    fn resting(&self, id: &str) -> Option<String> {
        let failed = self
            .failed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        failed
            .as_ref()
            .filter(|f| f.player == id && f.at.elapsed() < REST_AFTER_FAILURE)
            .map(|f| format!("{} (the fast way is resting)", f.why))
    }

    /// Loads player `id` into the solver, from the copy kept on disk when
    /// there is one, and returns its signature timestamp. A failure is
    /// remembered, so the fast way rests.
    async fn get_ready(&self, id: &str, url: &str) -> Result<u64, String> {
        std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        let code_path = self.cache.join(format!("{id}.js"));
        let ready_path = self.cache.join(format!("{id}.ready.js"));
        let forget_player = || {
            let _ = std::fs::remove_file(&code_path);
            let _ = std::fs::remove_file(&ready_path);
        };
        if let Ok(code) = std::fs::read_to_string(&code_path) {
            match self.load_player(id, code, &ready_path).await {
                Ok(sts) => return Ok(sts),
                Err(e) => {
                    // Perhaps a damaged copy: fetched again below.
                    log::info!("the saved player did not work ({e}); getting it again");
                    forget_player();
                }
            }
        }
        let code = self
            .session
            .fetch_text(url)
            .await
            .map_err(|e| format!("could not get YouTube's player: {e}"))?;
        // Older players go: only the current one is kept.
        if let Ok(entries) = std::fs::read_dir(&self.cache) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        if let Err(e) = write_whole(&code_path, &code) {
            log::info!("could not keep YouTube's player for next time: {e}");
        }
        match self.load_player(id, code, &ready_path).await {
            Ok(sts) => {
                *self
                    .failed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                Ok(sts)
            }
            Err(why) => {
                forget_player();
                // Whatever it read of the player, the solver gives back.
                self.solver.stop().await;
                *self
                    .failed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Failed {
                    player: id.to_string(),
                    at: Instant::now(),
                    why: why.clone(),
                });
                Err(why)
            }
        }
    }

    /// Loads player `id` from its `code`, or from the prepared form kept at
    /// `ready_path` when there is one, and returns its signature timestamp.
    async fn load_player(&self, id: &str, code: String, ready_path: &Path) -> Result<u64, String> {
        let sts = signature_timestamp(&code).ok_or("no signature timestamp in the player")?;
        if let Ok(ready) = std::fs::read_to_string(ready_path) {
            match self.solver.load(id, PlayerCode::Preprocessed(ready)).await {
                Ok(_) => return Ok(sts),
                Err(e) => {
                    log::info!("the saved prepared player did not load: {e}");
                    let _ = std::fs::remove_file(ready_path);
                }
            }
        }
        let ready = self.solver.load(id, PlayerCode::Code(code.clone())).await?;
        if let Some(ready) = ready {
            // Reading the whole player leaves the solver larger for good; a
            // fresh one given only the prepared code is smaller.
            self.solver.stop().await;
            match self
                .solver
                .load(id, PlayerCode::Preprocessed(ready.clone()))
                .await
            {
                Ok(_) => {
                    if let Err(e) = write_whole(ready_path, &ready) {
                        log::info!("could not keep the prepared player for next time: {e}");
                    }
                }
                Err(e) => {
                    log::info!("the prepared player did not load: {e}");
                    self.solver.load(id, PlayerCode::Code(code)).await?;
                }
            }
        }
        Ok(sts)
    }

    /// Finds a song's audio. A song found in the last hours is reused.
    pub async fn find(&self, video_id: &str) -> Result<Found, String> {
        self.find_with(video_id, true).await
    }

    /// Finds a song's audio ahead of time (a song the pointer rests on, the
    /// top search result). Skipped while the player is being got ready, so
    /// a song the listener plays never waits behind these.
    pub async fn find_ahead(&self, video_id: &str) -> Result<Found, String> {
        self.find_with(video_id, false).await
    }

    async fn find_with(&self, video_id: &str, wait: bool) -> Result<Found, String> {
        if let Some(found) = self.remembered(video_id) {
            return Ok(found);
        }
        let started = Instant::now();
        let player = self.player(wait).await?;
        let reply = self
            .session
            .player_reply(video_id, Some(player.sts))
            .await
            .map_err(|e| e.to_string())?;
        let info = read::player_info(&reply);
        if info.status.as_deref() != Some("OK") {
            return Err(info
                .reason
                .clone()
                .unwrap_or_else(|| "YouTube says this song cannot play".into()));
        }
        let formats = read::stream_formats(&reply);
        let format = read::best_stream(&formats).ok_or_else(|| why_no_stream(&formats))?;
        let url = self.unlock(&player, format).await?;
        let found = Found {
            source: Source {
                url,
                headers: vec![
                    ("User-Agent".into(), crate::net::USER_AGENT.into()),
                    ("Origin".into(), "https://music.youtube.com".into()),
                    ("Referer".into(), "https://music.youtube.com/".into()),
                ],
                size: format.content_length,
            },
            format: format.describe(),
            premium: format.is_premium(),
            duration_seconds: format
                .duration_ms
                .map(|ms| ms as f64 / 1000.0)
                .or(info.length_seconds.map(f64::from)),
            info,
            took: started.elapsed(),
        };
        self.remember(video_id, &found);
        Ok(found)
    }

    fn remembered(&self, video_id: &str) -> Option<Found> {
        let found = self
            .found
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (at, found) = found.get(video_id)?;
        (at.elapsed() < KEEP_FOUND).then(|| Found {
            took: Duration::ZERO,
            ..found.clone()
        })
    }

    fn remember(&self, video_id: &str, found: &Found) {
        let mut all = self
            .found
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        all.retain(|_, (at, _)| at.elapsed() < KEEP_FOUND);
        while all.len() >= MAX_FOUND {
            let oldest = all
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(key) => all.remove(&key),
                None => break,
            };
        }
        all.insert(video_id.to_string(), (Instant::now(), found.clone()));
    }

    /// Forgets a found song (its address stopped working).
    pub fn forget(&self, video_id: &str) {
        self.found
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(video_id);
    }

    /// The stream address with its puzzles solved: the signature (when the
    /// address is scrambled) and `n` (without which YouTube slows the
    /// stream to a crawl).
    async fn unlock(&self, player: &Player, format: &read::StreamFormat) -> Result<String, String> {
        let locked = Locked::of(format)?;
        let sig: Vec<String> = locked.signature.iter().map(|(_, s)| s.clone()).collect();
        let answers = self
            .solver
            .solve(&player.id, locked.n.as_slice(), &sig)
            .await?;
        locked.unlock(&answers)
    }
}

/// A stream address and the puzzles on it.
struct Locked {
    url: String,
    /// The address's field for the signature, and the scrambled signature,
    /// when the address is scrambled.
    signature: Option<(String, String)>,
    /// The `n` field's puzzle.
    n: Option<String>,
}

impl Locked {
    fn of(format: &read::StreamFormat) -> Result<Self, String> {
        let (url, signature) = match (&format.url, &format.signature_cipher) {
            (Some(url), _) => (url.clone(), None),
            (None, Some(cipher)) => {
                let fields = query_pairs(cipher);
                let url = fields
                    .get("url")
                    .ok_or("a scrambled address without an address")?;
                let s = fields
                    .get("s")
                    .ok_or("a scrambled address without a signature")?;
                let sp = fields.get("sp").map_or("signature", String::as_str);
                (url.clone(), Some((sp.to_string(), s.clone())))
            }
            (None, None) => return Err("no stream address".into()),
        };
        let n = query_pairs(url.split_once('?').map_or("", |(_, q)| q))
            .get("n")
            .cloned();
        Ok(Self { url, signature, n })
    }

    /// The address with the solver's `answers` put in.
    fn unlock(self, answers: &Answers) -> Result<String, String> {
        let mut url = self.url;
        if let Some((sp, s)) = &self.signature {
            let solved = answers.sig.get(s).ok_or("the signature was not solved")?;
            url = with_query(&url, sp, solved)?;
        }
        if let Some(n) = &self.n {
            let solved = answers.n.get(n).ok_or("n was not solved")?;
            if solved == n {
                return Err("n came back unchanged".into());
            }
            url = with_query(&url, "n", solved)?;
        }
        Ok(url)
    }
}

/// Writes `text` to `path` whole or not at all: into a file beside it, then
/// renamed into place, so an app closed while writing leaves no half a
/// player to be read next time.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(folder)?;
    file.write_all(text.as_bytes())?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Why none of a song's formats can be played, for the log: it tells
/// what to change when YouTube changes.
fn why_no_stream(formats: &[StreamFormat]) -> String {
    if formats.is_empty() {
        "YouTube's answer had no audio formats".into()
    } else if formats
        .iter()
        .all(|f| f.url.is_none() && f.signature_cipher.is_none())
    {
        "YouTube's answer had audio formats without addresses (its newer streaming method only)"
            .into()
    } else {
        let offered: Vec<String> = formats.iter().map(|f| f.itag.to_string()).collect();
        format!(
            "YouTube offered no plain AAC audio (formats {})",
            offered.join(", ")
        )
    }
}

/// `/s/player/0123abcd/player_ias.vflset/en_US/base.js` → `0123abcd`.
pub fn player_id(url: &str) -> Option<String> {
    let after = url.split("/s/player/").nth(1)?;
    let id = after.split('/').next()?;
    (id.len() >= 6
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| id.to_string())
}

/// The player's signature timestamp (`signatureTimestamp:20302` or
/// `sts:20302`), as yt-dlp finds it.
pub fn signature_timestamp(code: &str) -> Option<u64> {
    for key in ["signatureTimestamp", "sts"] {
        let mut rest = code;
        while let Some(at) = rest.find(key) {
            let after = rest[at + key.len()..].trim_start();
            if let Some(value) = after.strip_prefix(':') {
                let digits: String = value
                    .trim_start()
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                if digits.len() == 5 {
                    return digits.parse().ok();
                }
            }
            rest = &rest[at + key.len()..];
        }
    }
    None
}

/// A query string's fields, decoded.
fn query_pairs(query: &str) -> HashMap<String, String> {
    reqwest::Url::parse(&format!("https://x.invalid/?{query}"))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// `url` with field `name` set to `value`.
fn with_query(url: &str, name: &str, value: &str) -> Result<String, String> {
    let mut parsed =
        reqwest::Url::parse(url).map_err(|e| redact::urls(&format!("a broken address: {e}")))?;
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| k != name)
        .collect();
    parsed
        .query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair(name, value);
    Ok(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_player_address() {
        assert_eq!(
            player_id("/s/player/6f3c4a8e/player_ias.vflset/en_US/base.js").as_deref(),
            Some("6f3c4a8e")
        );
        assert_eq!(player_id("/s/other/base.js"), None);
    }

    #[test]
    fn finds_the_signature_timestamp() {
        assert_eq!(
            signature_timestamp("a,signatureTimestamp:20302,b"),
            Some(20302)
        );
        assert_eq!(signature_timestamp("x={sts: 19876};"), Some(19876));
        assert_eq!(signature_timestamp("nothing here"), None);
        assert_eq!(signature_timestamp("sts:12"), None);
    }

    #[test]
    fn rewrites_query_fields() {
        let cipher =
            "s=AB%3DC&sp=sig&url=https%3A%2F%2Fr1.example%2Fvideoplayback%3Fn%3Dxyz%26itag%3D141";
        let fields = query_pairs(cipher);
        assert_eq!(fields["s"], "AB=C");
        assert_eq!(fields["sp"], "sig");
        let url = &fields["url"];
        assert_eq!(query_pairs(url.split_once('?').unwrap().1)["n"], "xyz");
        let unlocked = with_query(url, "sig", "solved=").unwrap();
        let unlocked = with_query(&unlocked, "n", "abc").unwrap();
        let fields = query_pairs(unlocked.split_once('?').unwrap().1);
        assert_eq!(fields["n"], "abc");
        assert_eq!(fields["sig"], "solved=");
        assert_eq!(fields["itag"], "141");
    }

    fn answers(n: &[(&str, &str)], sig: &[(&str, &str)]) -> Answers {
        let map = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        Answers {
            n: map(n),
            sig: map(sig),
        }
    }

    #[test]
    fn unlocks_a_scrambled_address() {
        let reply = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000,
             "signatureCipher": "s=AB%3DC&sp=sig&url=https%3A%2F%2Fr1.example%2Fvideoplayback%3Fn%3Dxyz%26itag%3D141"}
        ]}});
        let formats = read::stream_formats(&reply);
        let locked = Locked::of(&formats[0]).unwrap();
        assert_eq!(locked.n.as_deref(), Some("xyz"));
        assert_eq!(
            locked.signature,
            Some(("sig".to_string(), "AB=C".to_string()))
        );
        let url = locked
            .unlock(&answers(&[("xyz", "solved-n")], &[("AB=C", "solved-sig")]))
            .unwrap();
        let fields = query_pairs(url.split_once('?').unwrap().1);
        assert_eq!(fields["n"], "solved-n");
        assert_eq!(fields["sig"], "solved-sig");
        assert_eq!(fields["itag"], "141");
        assert!(
            url.starts_with("https://r1.example/videoplayback?"),
            "{url}"
        );

        // Puzzles left unsolved, or `n` handed back as it was, are refused:
        // the stream would be refused or slowed to a crawl.
        let unsolved = Locked::of(&formats[0])
            .unwrap()
            .unlock(&answers(&[("xyz", "solved-n")], &[]));
        assert_eq!(unsolved, Err("the signature was not solved".into()));
        let unchanged = Locked::of(&formats[0])
            .unwrap()
            .unlock(&answers(&[("xyz", "xyz")], &[("AB=C", "solved-sig")]));
        assert_eq!(unchanged, Err("n came back unchanged".into()));
    }

    #[test]
    fn unlocks_a_plain_address() {
        let reply = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000,
             "url": "https://r1.example/videoplayback?itag=140&n=abc"},
            {"itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 50000,
             "url": "https://r1.example/videoplayback?itag=139"}
        ]}});
        let formats = read::stream_formats(&reply);
        let locked = Locked::of(&formats[0]).unwrap();
        assert!(locked.signature.is_none());
        let url = locked.unlock(&answers(&[("abc", "cba")], &[])).unwrap();
        assert_eq!(query_pairs(url.split_once('?').unwrap().1)["n"], "cba");
        // Nothing to solve: the address as it came.
        let plain = Locked::of(&formats[1]).unwrap();
        assert!(plain.n.is_none() && plain.signature.is_none());
        assert_eq!(
            plain.unlock(&Answers::default()).unwrap(),
            "https://r1.example/videoplayback?itag=139"
        );
    }

    #[test]
    fn writes_a_file_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("player.js");
        write_whole(&path, "first").unwrap();
        write_whole(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        // Nothing else is left beside it.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn says_why_no_stream_plays() {
        let reply = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000}
        ]}});
        let without_addresses = read::stream_formats(&reply);
        assert!(why_no_stream(&without_addresses).contains("without addresses"));
        assert!(why_no_stream(&[]).contains("no audio formats"));
        let reply = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 150000,
             "url": "https://r1.googlevideo.example/d"}
        ]}});
        let opus_only = read::stream_formats(&reply);
        let why = why_no_stream(&opus_only);
        assert!(why.contains("251") && !why.contains("googlevideo"), "{why}");
    }
}
