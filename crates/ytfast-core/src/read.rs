//! Reading YouTube Music's replies.
//!
//! YouTube changes the shape of its JSON often and runs experiments that
//! give different accounts different layouts. These readers therefore look
//! for the pieces they need (a song row, a play button, a flag) wherever
//! they are, instead of following one exact path, and return `None` or an
//! empty list rather than failing. All reading of YouTube's JSON lives in
//! this module, so a YouTube change means fixing one file.

use serde_json::Value;

/// Two flags YouTube puts in every reply (`GFEEDBACK` tracking params).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccountFlags {
    pub logged_in: Option<bool>,
    /// `has_unlimited_entitlement`: true for YouTube Music Premium.
    pub premium: Option<bool>,
}

/// Reads [`AccountFlags`] from any reply.
pub fn account_flags(reply: &Value) -> AccountFlags {
    let mut flags = AccountFlags::default();
    let services = reply
        .pointer("/responseContext/serviceTrackingParams")
        .and_then(Value::as_array);
    for service in services.into_iter().flatten() {
        if service.get("service").and_then(Value::as_str) != Some("GFEEDBACK") {
            continue;
        }
        for param in service
            .get("params")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let value = param
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match param.get("key").and_then(Value::as_str) {
                Some("logged_in") => flags.logged_in = Some(value == "1"),
                Some("has_unlimited_entitlement") => {
                    flags.premium = Some(value.eq_ignore_ascii_case("true"));
                }
                _ => {}
            }
        }
    }
    flags
}

/// The signed-in account, from `account/account_menu`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub handle: Option<String>,
}

pub fn account(reply: &Value) -> Option<Account> {
    let header = find_key(reply, "activeAccountHeaderRenderer")?;
    let name = text(header.get("accountName")?).filter(|s| !s.is_empty())?;
    let handle = header
        .get("channelHandle")
        .and_then(text)
        .filter(|s| !s.is_empty());
    Some(Account { name, handle })
}

/// What kind of video a row plays. YouTube Music's "songs" are audio tracks
/// (`ATV`); music videos and user uploads play the same way without the
/// picture.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TrackKind {
    Song,
    MusicVideo,
    Other(String),
    #[default]
    Unknown,
}

impl TrackKind {
    fn from_music_video_type(kind: Option<&str>) -> Self {
        match kind {
            Some("MUSIC_VIDEO_TYPE_ATV") => Self::Song,
            Some("MUSIC_VIDEO_TYPE_OMV") => Self::MusicVideo,
            Some(other) => Self::Other(other.to_string()),
            None => Self::Unknown,
        }
    }
}

/// A playable row from a list (a playlist, Liked songs, History, search).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub video_id: String,
    /// This row's own ID within a playlist. Two copies of a song in one
    /// playlist share `video_id` but not this; editing a playlist must use it.
    pub set_video_id: Option<String>,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub duration_seconds: Option<u32>,
    pub kind: TrackKind,
}

/// Every playable row in a reply, in YouTube's order. Rows without a video
/// (an artist or a playlist in search results) are skipped.
pub fn tracks(reply: &Value) -> Vec<Track> {
    let mut rows = Vec::new();
    collect(reply, "musicResponsiveListItemRenderer", &mut rows);
    rows.into_iter().filter_map(track).collect()
}

fn track(row: &Value) -> Option<Track> {
    let video_id = row
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            find_key(row, "watchEndpoint")
                .and_then(|w| w.get("videoId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    let set_video_id = row
        .pointer("/playlistItemData/playlistSetVideoId")
        .and_then(Value::as_str)
        .map(str::to_string);

    let columns: Vec<String> = row
        .get("flexColumns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|column| {
            column
                .get("musicResponsiveListItemFlexColumnRenderer")
                .and_then(|r| r.get("text"))
                .and_then(text)
                .unwrap_or_default()
        })
        .collect();
    let fixed = row
        .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
        .and_then(text);

    let title = columns.first().cloned().unwrap_or_default();
    let artists = columns.get(1).cloned().unwrap_or_default();
    // The duration is usually a fixed column; some lists (collaborative
    // playlists) put it in a flexible one instead.
    let duration_seconds = fixed
        .as_deref()
        .and_then(parse_duration)
        .or_else(|| columns.iter().skip(1).find_map(|c| parse_duration(c)));
    let album = columns
        .get(2)
        .filter(|c| !c.is_empty() && parse_duration(c).is_none())
        .cloned();
    let kind = TrackKind::from_music_video_type(
        find_key(row, "watchEndpointMusicConfig")
            .and_then(|c| c.get("musicVideoType"))
            .and_then(Value::as_str),
    );
    Some(Track {
        video_id,
        set_video_id,
        title,
        artists,
        album,
        duration_seconds,
        kind,
    })
}

/// The token for the next page of a list's rows, when there are more.
pub fn track_continuation(reply: &Value) -> Option<String> {
    fn search(node: &Value) -> Option<String> {
        match node {
            Value::Object(map) => {
                let has_rows = map
                    .get("contents")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| {
                        rows.iter()
                            .any(|r| r.get("musicResponsiveListItemRenderer").is_some())
                    });
                if has_rows {
                    // Older replies: a `continuations` list next to the rows.
                    let old = map
                        .get("continuations")
                        .and_then(|c| find_key(c, "continuation"))
                        .and_then(Value::as_str);
                    // Newer replies: a final row that holds the token.
                    let new = map
                        .get("contents")
                        .and_then(Value::as_array)
                        .and_then(|rows| rows.last())
                        .and_then(|last| last.get("continuationItemRenderer"))
                        .and_then(|c| find_key(c, "token"))
                        .and_then(Value::as_str);
                    if let Some(token) = old.or(new) {
                        return Some(token.to_string());
                    }
                }
                map.values().find_map(search)
            }
            Value::Array(items) => items.iter().find_map(search),
            _ => None,
        }
    }
    search(reply)
}

/// What the `player` endpoint says about one song.
#[derive(Clone, Default, PartialEq)]
pub struct PlayerInfo {
    /// `OK` when the song can play for this account.
    pub status: Option<String>,
    pub reason: Option<String>,
    pub video_id: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub length_seconds: Option<u32>,
    pub kind: TrackKind,
    /// How much louder than YouTube's target the song is, in dB. YouTube
    /// turns loud songs down by this much, so YtFast does too.
    pub loudness_db: Option<f64>,
    /// Where to report that the song started playing (History).
    pub playback_url: Option<String>,
    /// Where to report how long it was listened to.
    pub watchtime_url: Option<String>,
}

impl std::fmt::Debug for PlayerInfo {
    // The report addresses carry session details; they are not shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerInfo")
            .field("status", &self.status)
            .field("reason", &self.reason)
            .field("video_id", &self.video_id)
            .field("length_seconds", &self.length_seconds)
            .field("kind", &self.kind)
            .field("loudness_db", &self.loudness_db)
            .field("playback_url", &self.playback_url.is_some())
            .field("watchtime_url", &self.watchtime_url.is_some())
            .finish()
    }
}

pub fn player_info(reply: &Value) -> PlayerInfo {
    let s = |pointer: &str| {
        reply
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let length_seconds = s("/videoDetails/lengthSeconds").and_then(|l| l.parse().ok());
    let loudness_db = reply
        .pointer("/playerConfig/audioConfig/loudnessDb")
        .and_then(Value::as_f64)
        .or_else(|| {
            reply
                .pointer("/streamingData/adaptiveFormats")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|f| f.get("loudnessDb").and_then(Value::as_f64))
        });
    PlayerInfo {
        status: s("/playabilityStatus/status"),
        reason: s("/playabilityStatus/reason"),
        video_id: s("/videoDetails/videoId"),
        title: s("/videoDetails/title"),
        author: s("/videoDetails/author"),
        length_seconds,
        kind: TrackKind::from_music_video_type(s("/videoDetails/musicVideoType").as_deref()),
        loudness_db,
        playback_url: s("/playbackTracking/videostatsPlaybackUrl/baseUrl"),
        watchtime_url: s("/playbackTracking/videostatsWatchtimeUrl/baseUrl"),
    }
}

/// `"3:45"` or `"1:02:03"` in seconds.
pub fn parse_duration(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.contains(':') {
        return None;
    }
    let mut total: u32 = 0;
    for part in text.split(':') {
        if part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        total = total.checked_mul(60)?.checked_add(part.parse().ok()?)?;
    }
    Some(total)
}

/// The text of a YouTube text object: `{"runs":[{"text":..}]}` or
/// `{"simpleText":..}`.
fn text(node: &Value) -> Option<String> {
    if let Some(simple) = node.get("simpleText").and_then(Value::as_str) {
        return Some(simple.to_string());
    }
    let runs = node.get("runs")?.as_array()?;
    Some(
        runs.iter()
            .filter_map(|r| r.get("text").and_then(Value::as_str))
            .collect(),
    )
}

/// The first value under `key`, searching depth first.
fn find_key<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    match node {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

/// Every value under `key`, in document order, not looking inside a match.
fn collect<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    out.push(v);
                } else {
                    collect(v, key, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect(v, key, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn real_playlist_rows() {
        let reply = fixture("playlist_signed_in_premium.json");
        let rows = tracks(&reply);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].video_id, "3_R4ulvg8OY");
        assert_eq!(rows[0].set_video_id.as_deref(), Some("56B44F6D10557CC6"));
        assert_eq!(rows[0].title, "I Hate Everything (feat. Action Bronson)");
        assert_eq!(rows[0].artists, "The Alchemist");
        assert_eq!(rows[0].album.as_deref(), Some("The Food Villain"));
        assert_eq!(rows[0].duration_seconds, Some(83));
        assert_eq!(rows[0].kind, TrackKind::Song);
        assert!(track_continuation(&reply).is_some());
    }

    #[test]
    fn real_flags_signed_in_with_premium() {
        let flags = account_flags(&fixture("playlist_signed_in_premium.json"));
        assert_eq!(
            flags,
            AccountFlags {
                logged_in: Some(true),
                premium: Some(true)
            }
        );
        let flags = account_flags(&fixture("playlist_signed_out.json"));
        assert_eq!(
            flags,
            AccountFlags {
                logged_in: Some(false),
                premium: Some(false)
            }
        );
        assert_eq!(account_flags(&Value::Null), AccountFlags::default());
    }

    #[test]
    fn duration_in_a_flexible_column_and_music_videos() {
        let rows = tracks(&fixture("playlist_collaborative.json"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].duration_seconds, Some(214));
        assert_eq!(rows[0].album, None);
        // The same song twice: different rows, told apart by set_video_id.
        assert_eq!(rows[1].video_id, "dQw4w9WgXcQ");
        assert_eq!(rows[1].kind, TrackKind::MusicVideo);
        assert_ne!(rows[0].set_video_id, rows[1].set_video_id);
    }

    #[test]
    fn history_rows() {
        let rows = tracks(&fixture("history_synthetic.json"));
        let ids: Vec<&str> = rows.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, ["histSong001", "histSong002", "histSong003"]);
        assert_eq!(rows[0].set_video_id, None);
    }

    #[test]
    fn account_menu() {
        let reply = fixture("account_menu_synthetic.json");
        assert_eq!(
            account(&reply),
            Some(Account {
                name: "Sample Listener".into(),
                handle: Some("@samplelistener".into())
            })
        );
        assert_eq!(account(&Value::Null), None);
    }

    #[test]
    fn player_reply() {
        let info = player_info(&fixture("player_synthetic.json"));
        assert_eq!(info.status.as_deref(), Some("OK"));
        assert_eq!(info.video_id.as_deref(), Some("AjXQiKP5kMs"));
        assert_eq!(info.length_seconds, Some(246));
        assert_eq!(info.kind, TrackKind::Song);
        assert_eq!(info.loudness_db, Some(-1.3));
        assert!(
            info.playback_url
                .as_deref()
                .unwrap()
                .starts_with("https://s.youtube.com/api/stats/playback")
        );
        assert!(
            info.watchtime_url
                .as_deref()
                .unwrap()
                .starts_with("https://s.youtube.com/api/stats/watchtime")
        );
        assert!(!format!("{info:?}").contains("s.youtube.com"));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3:45"), Some(225));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("0:07"), Some(7));
        assert_eq!(parse_duration("Revival"), None);
        assert_eq!(parse_duration("12"), None);
        assert_eq!(parse_duration("1:2x"), None);
        assert_eq!(parse_duration("Song • 3:45"), None);
    }
}
