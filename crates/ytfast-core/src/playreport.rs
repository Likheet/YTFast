//! Telling YouTube what was listened to, so History, Home, mixes and Recap
//! keep learning from YtFast plays.
//!
//! The YouTube Music player sends two kinds of report for each song, to the
//! addresses the `player` reply gives (`playbackTracking`): one when a song
//! starts (it lands in History), and how long it was listened to. YtFast
//! sends the same two, with the parameters yt-dlp's `_mark_watched` uses:
//! a random playback ID (`cpn`), the position (`cmt`), and for listening
//! time the listened span (`st`, `et`).

use rand::Rng;

use crate::read::PlayerInfo;

const CPN_ALPHABET: &[u8; 64] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

/// The reports for one play of one song.
#[derive(Clone)]
pub struct PlayReport {
    cpn: String,
    playback: Option<String>,
    watchtime: Option<String>,
}

impl std::fmt::Debug for PlayReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayReport")
            .field("playback", &self.playback.is_some())
            .field("watchtime", &self.watchtime.is_some())
            .finish()
    }
}

impl PlayReport {
    pub fn new(info: &PlayerInfo) -> Self {
        let mut rng = rand::rng();
        let cpn = (0..16)
            .map(|_| CPN_ALPHABET[rng.random_range(0..CPN_ALPHABET.len())] as char)
            .collect();
        Self {
            cpn,
            playback: info.playback_url.clone(),
            watchtime: info.watchtime_url.clone(),
        }
    }

    /// Whether YouTube gave the addresses to report to.
    pub fn is_possible(&self) -> bool {
        self.playback.is_some()
    }

    /// The "started playing" report, at `position` seconds.
    pub fn started(&self, position: f64) -> Option<String> {
        with_params(
            self.playback.as_deref()?,
            &[
                ("ver", "2".into()),
                ("c", "WEB_REMIX".into()),
                ("cpn", self.cpn.clone()),
                ("cmt", seconds(position)),
            ],
        )
    }

    /// The "listened from `start` to `end`" report.
    pub fn listened(&self, start: f64, end: f64) -> Option<String> {
        self.listened_to(&[(start, end)])
    }

    /// The "listened to these stretches" report, each from and to, in
    /// order: after a jump in the song there are several, which YouTube's
    /// player sends as lists.
    pub fn listened_to(&self, stretches: &[(f64, f64)]) -> Option<String> {
        let last = stretches.last()?.1;
        let list = |pick: fn(&(f64, f64)) -> f64| {
            stretches
                .iter()
                .map(|s| seconds(pick(s)))
                .collect::<Vec<_>>()
                .join(",")
        };
        with_params(
            self.watchtime.as_deref()?,
            &[
                ("ver", "2".into()),
                ("c", "WEB_REMIX".into()),
                ("cpn", self.cpn.clone()),
                ("cmt", seconds(last)),
                ("st", list(|s| s.0)),
                ("et", list(|s| s.1)),
            ],
        )
    }
}

fn seconds(value: f64) -> String {
    format!("{:.3}", value.max(0.0))
}

/// `base` with `params` set, replacing any existing values of those keys.
/// `el` defaults to `detailpage`, as yt-dlp does (the default otherwise is
/// Shorts).
fn with_params(base: &str, params: &[(&str, String)]) -> Option<String> {
    let mut url = reqwest::Url::parse(base).ok()?;
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !params.iter().any(|(p, _)| p == k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if !pairs.iter().any(|(k, _)| k == "el") {
        pairs.push(("el".into(), "detailpage".into()));
    }
    pairs.extend(params.iter().map(|(k, v)| (k.to_string(), v.clone())));
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::TrackKind;

    fn info() -> PlayerInfo {
        PlayerInfo {
            status: Some("OK".into()),
            reason: None,
            video_id: Some("abc".into()),
            title: None,
            author: None,
            length_seconds: Some(200),
            kind: TrackKind::Song,
            loudness_db: None,
            playback_url: Some("https://s.youtube.com/api/stats/playback?cl=1&docid=abc&el=detailpage&len=200&cmt=9".into()),
            watchtime_url: Some("https://s.youtube.com/api/stats/watchtime?cl=1&docid=abc&len=200".into()),
        }
    }

    fn query(url: &str) -> Vec<(String, String)> {
        reqwest::Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn started_report() {
        let report = PlayReport::new(&info());
        let q = query(&report.started(1.5).unwrap());
        let get = |k: &str| q.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("docid"), Some("abc"));
        assert_eq!(get("ver"), Some("2"));
        assert_eq!(get("c"), Some("WEB_REMIX"));
        assert_eq!(get("cmt"), Some("1.500"));
        assert_eq!(get("el"), Some("detailpage"));
        assert_eq!(get("cpn").unwrap().len(), 16);
        // The old position was replaced, not repeated.
        assert_eq!(q.iter().filter(|(k, _)| k == "cmt").count(), 1);
    }

    #[test]
    fn listened_report() {
        let report = PlayReport::new(&info());
        let q = query(&report.listened(0.0, 61.25).unwrap());
        let get = |k: &str| q.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("st"), Some("0.000"));
        assert_eq!(get("et"), Some("61.250"));
        assert_eq!(get("cmt"), Some("61.250"));
        assert_eq!(get("el"), Some("detailpage"));
    }

    #[test]
    fn stretches_listened_to_after_a_jump() {
        let report = PlayReport::new(&info());
        let url = report.listened_to(&[(0.0, 30.0), (90.0, 120.5)]).unwrap();
        let q = query(&url);
        let get = |k: &str| q.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("st"), Some("0.000,90.000"));
        assert_eq!(get("et"), Some("30.000,120.500"));
        assert_eq!(get("cmt"), Some("120.500"));
        assert_eq!(report.listened_to(&[]), None);
    }

    #[test]
    fn one_play_uses_one_playback_id() {
        let report = PlayReport::new(&info());
        let a = query(&report.started(0.0).unwrap());
        let b = query(&report.listened(0.0, 5.0).unwrap());
        let cpn = |q: &[(String, String)]| q.iter().find(|(k, _)| k == "cpn").unwrap().1.clone();
        assert_eq!(cpn(&a), cpn(&b));
    }

    #[test]
    fn nothing_without_addresses() {
        let mut i = info();
        i.playback_url = None;
        i.watchtime_url = None;
        let report = PlayReport::new(&i);
        assert!(!report.is_possible());
        assert_eq!(report.started(0.0), None);
        assert_eq!(report.listened(0.0, 1.0), None);
    }
}
