//! Finding a song's audio the way music.youtube.com does: one `player`
//! request, the stream address unlocked with YouTube's own player code
//! (kept ready in [`Solver`]), no yt-dlp. About half a second instead of
//! ten. yt-dlp stays as the fallback when this fails ([`crate::prepare`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::innertube::Session;
use crate::read::{self, PlayerInfo};
use crate::redact;
use crate::solver::{PlayerCode, Solver};
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
    pub took: Duration,
}

/// Stream addresses work for about six hours; found songs are reused for
/// less than that.
const KEEP_FOUND: Duration = Duration::from_secs(3 * 3600);
/// Found songs remembered at most (songs pointed at, top results).
const MAX_FOUND: usize = 40;

#[derive(Clone)]
struct Player {
    id: String,
    /// The signature timestamp the `player` request must carry for the
    /// addresses to unlock with this player.
    sts: u64,
}

pub struct Direct {
    session: Arc<Session>,
    solver: Solver,
    /// Where player code is kept between runs.
    cache: PathBuf,
    player: Mutex<Option<Player>>,
    found: std::sync::Mutex<HashMap<String, (Instant, Found)>>,
}

impl Direct {
    pub fn new(session: Arc<Session>, solver: Solver, cache: PathBuf) -> Self {
        Self {
            session,
            solver,
            cache,
            player: Mutex::new(None),
            found: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Gets YouTube's player code and the solver ready, so the first song
    /// is quick too. Called once after signing in; safe to call again.
    pub async fn warm_up(&self) -> Result<(), String> {
        self.player().await.map(|_| ())
    }

    /// Stops the solver when idle (see [`Solver::stop_if_idle`]).
    pub async fn rest_if_idle(&self, idle: Duration) {
        self.solver.stop_if_idle(idle).await;
    }

    /// The player, loaded in the solver.
    async fn player(&self) -> Result<Player, String> {
        let mut current = self.player.lock().await;
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

        std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        let code_path = self.cache.join(format!("{id}.js"));
        let ready_path = self.cache.join(format!("{id}.ready.js"));
        let code = match std::fs::read_to_string(&code_path) {
            Ok(code) => code,
            Err(_) => {
                let code = self
                    .session
                    .fetch_text(&url)
                    .await
                    .map_err(|e| format!("could not get YouTube's player: {e}"))?;
                // Older players go: only the current one is kept.
                if let Ok(entries) = std::fs::read_dir(&self.cache) {
                    for entry in entries.flatten() {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
                let _ = std::fs::write(&code_path, &code);
                code
            }
        };
        let sts = signature_timestamp(&code).ok_or("no signature timestamp in the player")?;
        let loaded = match std::fs::read_to_string(&ready_path) {
            Ok(ready) => self
                .solver
                .load(&id, PlayerCode::Preprocessed(ready))
                .await
                .map(|_| ()),
            Err(_) => Err(String::new()),
        };
        if loaded.is_err() {
            let ready = self.solver.load(&id, PlayerCode::Code(code)).await?;
            if let Some(ready) = ready {
                let _ = std::fs::write(&ready_path, ready);
            }
        }
        let player = Player { id, sts };
        *current = Some(player.clone());
        Ok(player)
    }

    /// Finds a song's audio. A song found in the last hours is reused.
    pub async fn find(&self, video_id: &str) -> Result<Found, String> {
        if let Some(found) = self.remembered(video_id) {
            return Ok(found);
        }
        let started = Instant::now();
        let player = self.player().await?;
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
        let format = read::best_stream(&formats).ok_or("no AAC audio offered")?;
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
        let sig: Vec<String> = signature.iter().map(|(_, s)| s.clone()).collect();
        let answers = self.solver.solve(&player.id, n.as_slice(), &sig).await?;
        let mut url = url;
        if let Some((sp, s)) = &signature {
            let solved = answers.sig.get(s).ok_or("the signature was not solved")?;
            url = with_query(&url, sp, solved)?;
        }
        if let Some(n) = &n {
            let solved = answers.n.get(n).ok_or("n was not solved")?;
            if solved == n {
                return Err("n came back unchanged".into());
            }
            url = with_query(&url, "n", solved)?;
        }
        Ok(url)
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
}
