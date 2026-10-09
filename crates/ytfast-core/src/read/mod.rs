//! Reading YouTube Music's replies.
//!
//! YouTube changes the shape of its JSON often and runs experiments that
//! give different accounts different layouts. These readers therefore look
//! for the pieces they need (a song row, a play button, a flag) wherever
//! they are, instead of following one exact path, and return `None` or an
//! empty list rather than failing. All reading of YouTube's JSON lives in
//! this module, so a YouTube change means fixing this one folder
//! (`src/read/`) and nothing outside it.

use serde_json::Value;

mod edit;
mod formats;
mod page;
mod search;
mod song;
mod sort;

pub use edit::{created_playlist_id, edit_status, feedback_processed};
pub use formats::{StreamFormat, best_stream, stream_formats};
pub use page::{
    Card, CardButton, CardLook, Header, HeaderButtons, Item, Page, PageKind, Section, Shape,
    Target, Thumb, TopResult, more_items, page, queue_continuation, queue_title, up_next,
};
pub use search::{SuggestedWords, Suggestions, search_suggestions};
pub use song::{Rating, SongDetails, lyrics, song_details};
pub use sort::{SortMenu, SortOrder};

/// Where a browse link says what kind of page it opens.
const PAGE_TYPE: &str =
    "/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType";

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
    /// The account's photo, as the top bar shows it.
    pub photo: Option<Thumb>,
}

pub fn account(reply: &Value) -> Option<Account> {
    let header = find_key(reply, "activeAccountHeaderRenderer")?;
    let name = text(header.get("accountName")?).filter(|s| !s.is_empty())?;
    let handle = header
        .get("channelHandle")
        .and_then(text)
        .filter(|s| !s.is_empty());
    let photo = header.get("accountPhoto").and_then(Thumb::best);
    Some(Account {
        name,
        handle,
        photo,
    })
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

/// A song's row from a list (a playlist, Liked songs, History, search).
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
    pub thumbnail: Option<Thumb>,
    /// The first artist's page (`UC...`), for "Go to artist".
    pub artist_id: Option<String>,
    /// The album's page (`MPREb_...`), for "Go to album".
    pub album_id: Option<String>,
    /// False for a row YouTube shows greyed out: a song it no longer
    /// offers (taken down, or not offered in this country). The row stays
    /// on its page, and can still be taken out of a playlist, but it does
    /// not play.
    pub playable: bool,
    /// What its row says beyond that, when it says more.
    pub more: Option<Box<TrackMore>>,
}

/// What a song's row says beyond its artists, album and length, when it
/// says more: its place in a chart, and its plays or views. Kept apart,
/// as most rows say neither.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackMore {
    /// Its place in a chart ("1"), as Explore's Trending numbers its rows.
    pub rank: Option<String>,
    /// "53M plays" or "28M views", as YouTube writes it.
    pub count: Option<String>,
    /// Marked explicit (YouTube's "E").
    pub explicit: bool,
}

impl Track {
    /// Whether YouTube marks it explicit.
    pub fn explicit(&self) -> bool {
        self.more.as_ref().is_some_and(|m| m.explicit)
    }

    /// Its place in a chart, when it has one.
    pub fn rank(&self) -> Option<&str> {
        self.more.as_ref()?.rank.as_deref()
    }

    /// Its plays or views, when its row says them.
    pub fn count(&self) -> Option<&str> {
        self.more.as_ref()?.count.as_deref()
    }
}

impl Default for Track {
    /// An empty row that can be played.
    fn default() -> Self {
        Self {
            video_id: String::new(),
            set_video_id: None,
            title: String::new(),
            artists: String::new(),
            album: None,
            duration_seconds: None,
            kind: TrackKind::default(),
            thumbnail: None,
            artist_id: None,
            album_id: None,
            playable: true,
            more: None,
        }
    }
}

/// Every song's row in a reply, in YouTube's order. Rows without a video
/// (an artist or a playlist in search results) are skipped.
pub fn tracks(reply: &Value) -> Vec<Track> {
    let mut rows = Vec::new();
    collect(reply, "musicResponsiveListItemRenderer", &mut rows);
    rows.into_iter().filter_map(track).collect()
}

/// How YouTube marks a row it no longer offers (ytmusicapi reads it as
/// `isAvailable`).
const GREYED_OUT: &str = "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT";

pub(crate) fn track(row: &Value) -> Option<Track> {
    let (removes_video, removes_row) = removal(row);
    let video_id = row
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            // A row that opens a page (an album, an artist) is not a song,
            // even when its menu can play one.
            if row.pointer("/navigationEndpoint/browseEndpoint").is_some() {
                return None;
            }
            // The play button or the title; never the menu's radio and
            // "play next" entries, which name other things.
            ["overlay", "flexColumns"]
                .iter()
                .find_map(|part| {
                    row.get(*part)
                        .and_then(|n| find_key(n, "watchEndpoint"))
                        .and_then(|w| w.get("videoId"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                // A greyed-out row has no play button; its menu's "Remove
                // from playlist" still names its song.
                .or(removes_video)
        })?;
    let set_video_id = row
        .pointer("/playlistItemData/playlistSetVideoId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(removes_row);
    let playable = row
        .get("musicItemRendererDisplayPolicy")
        .and_then(Value::as_str)
        != Some(GREYED_OUT);

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
    // Playlists put artists, album and length in columns of their own;
    // search results and Quick picks put them in one ("Song • Artist •
    // Album • 3:45").
    let byline = Byline::parse(columns.get(1).map(String::as_str).unwrap_or_default());
    let artists = byline.artists.clone();
    // The duration is usually a fixed column; some lists (collaborative
    // playlists) put it in a flexible one instead.
    let duration_seconds = fixed
        .as_deref()
        .and_then(parse_duration)
        .or(byline.duration_seconds)
        .or_else(|| columns.iter().skip(1).find_map(|c| parse_duration(c)));
    // The artist and album pages the row links to: in its columns, or in
    // its menu's "Go to album" and "Go to artist".
    let mut links = Links::default();
    for part in ["flexColumns", "menu"] {
        if let Some(node) = row.get(part) {
            links.add(node);
        }
    }
    // On an album's page the third column is the play count; on an
    // artist's page the album is the fourth column. An album named with
    // four digits ("1989") is still found, by its link.
    let album = columns
        .get(2)
        .filter(|c| !c.is_empty() && parse_duration(c).is_none() && !is_count(c) && !is_year(c))
        .cloned()
        .or(byline.album)
        .or(links.album);
    let kind = TrackKind::from_music_video_type(
        find_key(row, "watchEndpointMusicConfig")
            .and_then(|c| c.get("musicVideoType"))
            .and_then(Value::as_str),
    );
    // A chart's place, and the plays or views wherever the row says them.
    let rank = row
        .pointer("/customIndexColumn/musicCustomIndexColumnRenderer/text")
        .and_then(text)
        .filter(|r| !r.trim().is_empty());
    let count = columns
        .iter()
        .skip(1)
        .flat_map(|c| c.split(" \u{2022} "))
        .find(|part| is_count(part))
        .map(str::to_string);
    let explicit = row.get("badges").is_some_and(is_explicit);
    let more = (rank.is_some() || count.is_some() || explicit).then(|| {
        Box::new(TrackMore {
            rank,
            count,
            explicit,
        })
    });
    Some(Track {
        video_id,
        set_video_id,
        title,
        artists,
        album,
        duration_seconds,
        kind,
        thumbnail: row.get("thumbnail").and_then(Thumb::best),
        artist_id: links.artist_id,
        album_id: links.album_id,
        playable,
        more,
    })
}

/// The song and the row's own ID from a playlist row's "Remove from
/// playlist" entry, as ytmusicapi reads them: a greyed-out row may name
/// them only there.
fn removal(row: &Value) -> (Option<String>, Option<String>) {
    let mut edits = Vec::new();
    if let Some(menu) = row.get("menu") {
        collect(menu, "playlistEditEndpoint", &mut edits);
    }
    let field =
        |action: &Value, key: &str| action.get(key).and_then(Value::as_str).map(str::to_string);
    edits
        .iter()
        .filter_map(|edit| edit.get("actions").and_then(Value::as_array))
        .flatten()
        .find(|action| action.get("action").and_then(Value::as_str) == Some("ACTION_REMOVE_VIDEO"))
        .map(|action| (field(action, "removedVideoId"), field(action, "setVideoId")))
        .unwrap_or_default()
}

/// The pages a song's text links to, for "Go to artist" and "Go to
/// album": its first artist and its album.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Links {
    /// The first artist's page (`UC...`).
    pub artist_id: Option<String>,
    /// The album's page (`MPREb_...`).
    pub album_id: Option<String>,
    /// The album's name: the words of the link to it.
    pub album: Option<String>,
}

impl Links {
    /// Fills in what is still missing from the links under `node` (a
    /// row's columns, a byline, a menu), taking the first of each kind.
    pub(crate) fn add(&mut self, node: &Value) {
        match node {
            Value::Object(map) => {
                if let Some(browse) = map
                    .get("navigationEndpoint")
                    .and_then(|e| e.get("browseEndpoint"))
                {
                    // A run of text carries its words next to its link.
                    self.add_link(browse, map.get("text").and_then(Value::as_str));
                }
                map.values().for_each(|v| self.add(v));
            }
            Value::Array(items) => items.iter().for_each(|v| self.add(v)),
            _ => {}
        }
    }

    fn add_link(&mut self, browse: &Value, words: Option<&str>) {
        let Some(id) = browse.get("browseId").and_then(Value::as_str) else {
            return;
        };
        let page_type = browse.pointer(PAGE_TYPE).and_then(Value::as_str);
        let kind = match page_type {
            Some(_) => PageKind::from_page_type(page_type),
            // Without the page type, the ID tells.
            None if id.starts_with("UC") || id.starts_with("MPLAUC") => PageKind::Artist,
            None => PageKind::Album,
        };
        match kind {
            PageKind::Artist if self.artist_id.is_none() => {
                // A library artist's page (`MPLAUC...`) is the artist's
                // own page with a prefix.
                self.artist_id = Some(id.strip_prefix("MPLA").unwrap_or(id).to_string());
            }
            // Only real album pages: a "Go to album" can also point at the
            // album's playlist.
            PageKind::Album if self.album_id.is_none() && id.starts_with("MPRE") => {
                self.album_id = Some(id.to_string());
                self.album = words
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_string);
            }
            _ => {}
        }
    }
}

/// Whether a like or save toggle is on (liked, saved), from its like
/// actions. The side a toggle shows is what clicking it does: like when it
/// is off, remove the like when it is on. Newer replies say which side
/// shows (`isToggled`); older ones swap the two sides instead.
pub(crate) fn toggle_on(toggle: &Value) -> Option<bool> {
    let side = if toggle.get("isToggled").and_then(Value::as_bool) == Some(true) {
        "toggledServiceEndpoint"
    } else {
        "defaultServiceEndpoint"
    };
    let status = find_key(toggle.get(side)?, "likeEndpoint")?
        .get("status")?
        .as_str()?;
    match status {
        "LIKE" => Some(false),
        "INDIFFERENT" => Some(true),
        _ => None,
    }
}

/// The parts of a "Song • Artist • Album • 3:45" line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Byline {
    pub artists: String,
    pub album: Option<String>,
    pub duration_seconds: Option<u32>,
}

impl Byline {
    /// Words YouTube puts first to say what a row is.
    const KINDS: [&'static str; 10] = [
        "Song", "Video", "Episode", "Single", "EP", "Album", "Playlist", "Podcast", "Artist",
        "Profile",
    ];

    pub(crate) fn parse(line: &str) -> Self {
        let mut parts: Vec<&str> = line
            .split(" • ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() > 1 && Self::KINDS.contains(&parts[0]) {
            parts.remove(0);
        }
        let mut byline = Self::default();
        let mut rest = Vec::new();
        for part in parts {
            if let Some(seconds) = parse_duration(part) {
                byline.duration_seconds = Some(seconds);
            } else if !is_count(part) {
                rest.push(part);
            }
        }
        // The year comes last ("Artist • Album • 2019"). Four digits first
        // are the artist's name (the band "1349"); an album named so
        // ("1989") is kept when a year follows it, and is otherwise found
        // by its link.
        if rest.len() > 1 && rest.last().is_some_and(|part| is_year(part)) {
            rest.pop();
        }
        byline.artists = rest.first().map(|s| s.to_string()).unwrap_or_default();
        byline.album = rest.get(1).map(|s| s.to_string());
        byline
    }
}

/// "1.2M plays", "35K views": not an artist or an album. The words are
/// English: requests ask YouTube for English (`Session::context`).
/// Whether badges (`badges`, `subtitleBadges`) hold YouTube's explicit "E".
fn is_explicit(badges: &Value) -> bool {
    badges.as_array().is_some_and(|all| {
        all.iter().any(|b| {
            b.pointer("/musicInlineBadgeRenderer/icon/iconType")
                .and_then(Value::as_str)
                == Some("MUSIC_EXPLICIT_BADGE")
        })
    })
}

fn is_count(part: &str) -> bool {
    let lower = part.to_ascii_lowercase();
    lower.ends_with(" plays")
        || lower.ends_with(" views")
        || lower.ends_with(" play")
        || lower.ends_with(" view")
}

/// "2019": four digits, which may be a year.
fn is_year(part: &str) -> bool {
    part.len() == 4 && part.bytes().all(|b| b.is_ascii_digit())
}

/// Where the next rows of a long list (a playlist, Liked Music) come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Continuation {
    /// Newer replies: the token goes in the request's body.
    Body(String),
    /// Older replies: the token goes in the address (`ctoken`).
    Address(String),
}

/// Where the next rows of a list come from, when there are more. Works on
/// a page's reply and on a reply with more rows.
pub fn track_continuation(reply: &Value) -> Option<Continuation> {
    list_continuation(reply, &["musicResponsiveListItemRenderer"])
}

/// Where the next items of a list of songs or tiles come from (the
/// library's playlists, albums and artists), when there are more. Works
/// on a page's reply and on a reply with more items ([`more_items`]).
pub fn item_continuation(reply: &Value) -> Option<Continuation> {
    list_continuation(
        reply,
        &["musicResponsiveListItemRenderer", "musicTwoRowItemRenderer"],
    )
}

/// The continuation of the first list in `reply` whose rows include one of
/// `kinds`.
fn list_continuation(reply: &Value, kinds: &[&str]) -> Option<Continuation> {
    fn search(node: &Value, kinds: &[&str]) -> Option<Continuation> {
        match node {
            Value::Object(map) => {
                // Rows are in `contents` (`items` in a grid), or in
                // `continuationItems` in a newer reply with more rows.
                let rows = ["contents", "items", "continuationItems"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(Value::as_array))
                    .filter(|rows| {
                        rows.iter()
                            .any(|r| kinds.iter().any(|kind| r.get(*kind).is_some()))
                    });
                if let Some(rows) = rows {
                    // Older replies: a `continuations` list next to the rows.
                    let old = map
                        .get("continuations")
                        .and_then(|c| find_key(c, "continuation"))
                        .and_then(Value::as_str)
                        .map(|t| Continuation::Address(t.to_string()));
                    // Newer replies: a final row that holds the token.
                    let new = rows
                        .last()
                        .and_then(|last| last.get("continuationItemRenderer"))
                        .and_then(|c| find_key(c, "token"))
                        .and_then(Value::as_str)
                        .map(|t| Continuation::Body(t.to_string()));
                    if let Some(next) = old.or(new) {
                        return Some(next);
                    }
                }
                map.values().find_map(|v| search(v, kinds))
            }
            Value::Array(items) => items.iter().find_map(|v| search(v, kinds)),
            _ => None,
        }
    }
    search(reply, kinds)
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
pub(crate) fn text(node: &Value) -> Option<String> {
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
pub(crate) fn find_key<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    match node {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

/// Every value under `key`, in document order, not looking inside a match.
pub(crate) fn collect<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
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
        assert!(rows.iter().all(|r| r.playable));
        // An older reply: the token goes in the address.
        assert!(matches!(
            track_continuation(&reply),
            Some(Continuation::Address(_))
        ));
    }

    #[test]
    fn rows_after_the_first() {
        // A newer reply with more rows: the rows, then where the next
        // ones come from.
        let reply = serde_json::json!({
            "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
                {"musicResponsiveListItemRenderer": {
                    "playlistItemData": {"videoId": "abcdefghijk"},
                    "flexColumns": [
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "A song"}]}}},
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "A singer"}]}}}
                    ]
                }},
                {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "NEXT"}}}}
            ]}}]
        });
        let rows = tracks(&reply);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "A song");
        assert_eq!(
            track_continuation(&reply),
            Some(Continuation::Body("NEXT".into()))
        );
        // The last rows: nothing after them.
        let last = serde_json::json!({
            "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
                {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "abcdefghijk"}}}
            ]}}]
        });
        assert_eq!(track_continuation(&last), None);
    }

    #[test]
    fn greyed_out_rows_stay_but_do_not_play() {
        // Rows of a playlist in the layout ytmusicapi reads: one as usual,
        // one greyed out (a song no longer offered), and one greyed out
        // that names its song only in its "Remove from playlist" entry.
        let flex = |words: &str| serde_json::json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": words}]}}});
        let reply = serde_json::json!({"musicPlaylistShelfRenderer": {"contents": [
            {"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "playable001", "playlistSetVideoId": "ROW1"},
                "flexColumns": [flex("Still here"), flex("A singer")]
            }},
            {"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "greyedOut01", "playlistSetVideoId": "ROW2"},
                "flexColumns": [flex("Gone"), flex("A singer")],
                "musicItemRendererDisplayPolicy": "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT"
            }},
            {"musicResponsiveListItemRenderer": {
                "flexColumns": [flex("Also gone"), flex("Another singer")],
                "menu": {"menuRenderer": {"items": [
                    {"menuNavigationItemRenderer": {
                        "text": {"runs": [{"text": "Start radio"}]},
                        "navigationEndpoint": {"watchEndpoint": {"videoId": "otherSong01", "playlistId": "RDAMVMotherSong01"}}
                    }},
                    {"menuServiceItemRenderer": {
                        "text": {"runs": [{"text": "Remove from playlist"}]},
                        "serviceEndpoint": {"playlistEditEndpoint": {
                            "playlistId": "PLmine",
                            "actions": [{"setVideoId": "ROW3", "action": "ACTION_REMOVE_VIDEO", "removedVideoId": "greyedOut02"}]
                        }}
                    }}
                ]}},
                "musicItemRendererDisplayPolicy": "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT"
            }}
        ]}});
        let rows = tracks(&reply);
        let ids: Vec<(&str, Option<&str>, bool)> = rows
            .iter()
            .map(|r| (r.video_id.as_str(), r.set_video_id.as_deref(), r.playable))
            .collect();
        assert_eq!(
            ids,
            [
                ("playable001", Some("ROW1"), true),
                ("greyedOut01", Some("ROW2"), false),
                ("greyedOut02", Some("ROW3"), false),
            ]
        );
        // The page keeps every row; what to play is the app's choice.
        assert_eq!(page(&reply).tracks().len(), 3);
        // A row made without a reply plays.
        assert!(Track::default().playable);
    }

    #[test]
    fn library_grids_say_where_more_comes_from() {
        // The library's playlists in the layout ytmusicapi reads: a grid,
        // and an older reply's token next to its items.
        let tile = |name: &str, id: &str| {
            serde_json::json!({"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": name}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": id}}
            }})
        };
        let grid = serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {
            "contents": [{"gridRenderer": {
                "items": [tile("Liked Music", "VLLM"), tile("Road trip", "VLPLroad")],
                "continuations": [{"nextContinuationData": {"continuation": "GRID2"}}]
            }}],
            "continuations": [{"nextContinuationData": {"continuation": "MORE_SHELVES"}}]
        }}}}]}}});
        assert_eq!(
            item_continuation(&grid),
            Some(Continuation::Address("GRID2".into()))
        );
        // The songs' reader does not take a grid of tiles for a list.
        assert_eq!(track_continuation(&grid), None);
        // A newer reply with more: the items, then the token.
        let newer = serde_json::json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
            tile("Gym", "VLPLgym"),
            {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "GRID3"}}}}
        ]}}]});
        assert_eq!(
            item_continuation(&newer),
            Some(Continuation::Body("GRID3".into()))
        );
        // The last items: nothing after them.
        let last = serde_json::json!({"continuationContents": {"gridContinuation": {"items": [tile("Sleep", "VLPLsleep")]}}});
        assert_eq!(item_continuation(&last), None);
        assert_eq!(item_continuation(&Value::Null), None);
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
        // The length's column is not taken for the album; the album is
        // the fourth column's link.
        assert_eq!(rows[0].album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(rows[0].album_id.as_deref(), Some("MPREb_dcYZhAh5urI"));
        // The same song twice: different rows, told apart by set_video_id.
        assert_eq!(rows[1].video_id, "dQw4w9WgXcQ");
        assert_eq!(rows[1].kind, TrackKind::MusicVideo);
        assert_ne!(rows[0].set_video_id, rows[1].set_video_id);
    }

    #[test]
    fn rows_link_their_artist_and_album() {
        // A playlist's row: the artist and album columns are links.
        let rows = tracks(&fixture("playlist_signed_in_premium.json"));
        assert_eq!(
            rows[0].artist_id.as_deref(),
            Some("UC2Eotb0QaPkaJI4Cw4oHZ6Q")
        );
        assert_eq!(rows[0].album_id.as_deref(), Some("MPREb_5tjDwqVSJCv"));
        // An artist's top song by several artists: the first of them, and
        // the album (with its name) from the fourth column.
        let top = tracks(&fixture("artist.json"));
        assert_eq!(
            top[0].artist_id.as_deref(),
            Some("UCFQwlbXuZQLsnUcL01Ip6_w")
        );
        assert_eq!(top[0].album_id.as_deref(), Some("MPREb_NGwQppgBRiS"));
        assert_eq!(top[0].album.as_deref(), Some("Mesmerizer"));
        // An album's songs link their artist; their album is the page.
        let songs = tracks(&fixture("album.json"));
        assert_eq!(
            songs[0].artist_id.as_deref(),
            Some("UCnAcxgRZ065f_eXK1o85c1w")
        );
        assert_eq!(songs[0].album_id, None);
    }

    #[test]
    fn links_from_the_menu_and_without_page_types() {
        // No links in the columns: the menu's "Go to album" and "Go to
        // artist". A library artist's page stands for the artist's own.
        let reply = serde_json::json!({"musicShelfRenderer": {"contents": [
            {"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "abcdefghijk"},
                "flexColumns": [
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "A song"}]}}},
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "A singer"}]}}}
                ],
                "menu": {"menuRenderer": {"items": [
                    {"menuNavigationItemRenderer": {
                        "text": {"runs": [{"text": "Start radio"}]},
                        "navigationEndpoint": {"watchEndpoint": {"videoId": "abcdefghijk", "playlistId": "RDAMVMabcdefghijk"}}
                    }},
                    {"menuNavigationItemRenderer": {
                        "text": {"runs": [{"text": "Go to album"}]},
                        "navigationEndpoint": {"browseEndpoint": {
                            "browseId": "MPREb_album01",
                            "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ALBUM"
                            }}
                        }}
                    }},
                    {"menuNavigationItemRenderer": {
                        "text": {"runs": [{"text": "Go to artist"}]},
                        "navigationEndpoint": {"browseEndpoint": {
                            "browseId": "MPLAUCsinger01",
                            "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_LIBRARY_ARTIST"
                            }}
                        }}
                    }}
                ]}}
            }},
            // Links without page types: the IDs tell. A "Go to album" to
            // the album's playlist is not an album page.
            {"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "bcdefghijkl"},
                "flexColumns": [
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Another"}]}}},
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                        {"text": "Someone", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCsomeone01"}}},
                        {"text": " • "},
                        {"text": "Their album", "navigationEndpoint": {"browseEndpoint": {"browseId": "OLAK5uy_list"}}}
                    ]}}}
                ]
            }}
        ]}});
        let rows = tracks(&reply);
        assert_eq!(rows[0].artist_id.as_deref(), Some("UCsinger01"));
        assert_eq!(rows[0].album_id.as_deref(), Some("MPREb_album01"));
        // The menu's words are not the album's name.
        assert_eq!(rows[0].album, None);
        assert_eq!(rows[1].artist_id.as_deref(), Some("UCsomeone01"));
        assert_eq!(rows[1].album_id, None);
        assert_eq!(rows[1].album.as_deref(), Some("Their album"));
    }

    #[test]
    fn like_and_save_toggles() {
        let toggle = |toggled: Option<bool>, default: &str, then: &str| {
            let mut t = serde_json::json!({
                "defaultServiceEndpoint": {"likeEndpoint": {"status": default}},
                "toggledServiceEndpoint": {"likeEndpoint": {"status": then}}
            });
            if let Some(on) = toggled {
                t["isToggled"] = on.into();
            }
            toggle_on(&t)
        };
        // Saying which side shows.
        assert_eq!(toggle(Some(false), "LIKE", "INDIFFERENT"), Some(false));
        assert_eq!(toggle(Some(true), "LIKE", "INDIFFERENT"), Some(true));
        // Swapping the sides.
        assert_eq!(toggle(None, "LIKE", "INDIFFERENT"), Some(false));
        assert_eq!(toggle(None, "INDIFFERENT", "LIKE"), Some(true));
        // Not a like toggle.
        assert_eq!(toggle_on(&serde_json::json!({"isToggled": true})), None);
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
                handle: Some("@samplelistener".into()),
                photo: Some(Thumb {
                    url: "https://yt3.ggpht.com/sample".into(),
                    width: 88
                })
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
    fn bylines() {
        let b = Byline::parse("Song • Daddy Yankee & Snow • Barrio Fino • 3:21");
        assert_eq!(b.artists, "Daddy Yankee & Snow");
        assert_eq!(b.album.as_deref(), Some("Barrio Fino"));
        assert_eq!(b.duration_seconds, Some(201));
        let b = Byline::parse("Eminem");
        assert_eq!(
            b,
            Byline {
                artists: "Eminem".into(),
                album: None,
                duration_seconds: None
            }
        );
        let b = Byline::parse("Video • Rick Astley • 1.6B views • 3:33");
        assert_eq!(b.artists, "Rick Astley");
        assert_eq!(b.album, None);
        let b = Byline::parse("Rick Astley • Whenever You Need Somebody • 1987");
        assert_eq!(b.album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(Byline::parse("Rick Astley • 1987").album, None);
        assert_eq!(
            Byline::parse("Daft Punk • Discovery • 2001 • 1.2M plays").album,
            Some("Discovery".into())
        );
    }

    #[test]
    fn names_of_four_digits_are_not_years() {
        // The band "1349": first, so the artist, not a year.
        let b = Byline::parse("Song • 1349 • 3:45");
        assert_eq!(b.artists, "1349");
        let b = Byline::parse("1349 • Demonoir • 2010");
        assert_eq!(b.artists, "1349");
        assert_eq!(b.album.as_deref(), Some("Demonoir"));
        // An album "1989" with its year after it.
        let b = Byline::parse("Taylor Swift • 1989 • 2014");
        assert_eq!(b.artists, "Taylor Swift");
        assert_eq!(b.album.as_deref(), Some("1989"));
        // Last, it may be a year; the row's link to it still names it.
        let reply = serde_json::json!({"musicShelfRenderer": {"contents": [
            {"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "abcdefghijk"},
                "flexColumns": [
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Style"}]}}},
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                        {"text": "Song"}, {"text": " • "},
                        {"text": "Taylor Swift", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCtaylor01"}}},
                        {"text": " • "},
                        {"text": "1989", "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_1989"}}},
                        {"text": " • "}, {"text": "3:51"}
                    ]}}}
                ]
            }}
        ]}});
        let rows = tracks(&reply);
        assert_eq!(rows[0].artists, "Taylor Swift");
        assert_eq!(rows[0].album.as_deref(), Some("1989"));
        assert_eq!(rows[0].duration_seconds, Some(231));
    }

    #[test]
    fn rows_have_thumbnails() {
        let rows = tracks(&fixture("playlist_signed_in_premium.json"));
        let thumb = rows[0].thumbnail.as_ref().expect("a thumbnail");
        assert!(thumb.url.starts_with("https://"));
        assert!(thumb.width > 0);
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

    #[test]
    fn a_filtered_search_and_its_next_results() {
        use serde_json::json;
        // A song row as YouTube sends it, in a filtered search's one list
        // and in the reply with the next ones (older replies: the token
        // beside the rows).
        let search = fixture("search.json");
        let mut rows = Vec::new();
        collect(&search, "musicResponsiveListItemRenderer", &mut rows);
        let row = json!({"musicResponsiveListItemRenderer": rows[0].clone()});
        let next = |token: &str| json!([{"nextContinuationData": {"continuation": token}}]);
        let first = json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {
            "content": {"sectionListRenderer": {"contents": [{"musicShelfRenderer": {
                "title": {"runs": [{"text": "Songs"}]},
                "contents": [row.clone(), row.clone()],
                "continuations": next("first")
            }}]}}
        }}]}}});
        assert_eq!(
            item_continuation(&first),
            Some(Continuation::Address("first".into()))
        );
        let more = json!({"continuationContents": {"musicShelfContinuation": {
            "contents": [row.clone(), row.clone(), row],
            "continuations": next("second")
        }}});
        assert_eq!(more_items(&more).len(), 3);
        assert_eq!(
            item_continuation(&more),
            Some(Continuation::Address("second".into()))
        );
        // The last batch says nothing more.
        let last = json!({"continuationContents": {"musicShelfContinuation": {"contents": []}}});
        assert_eq!(item_continuation(&last), None);
    }
}
