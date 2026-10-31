//! Radar: YTFast's own recommendations, the left menu's Radar page and
//! its station ([`crate::station`]).
//!
//! YouTube finds the songs: a song's radio (what its Up next plays), the
//! artists an artist's page calls similar ("Fans might also like"), and an
//! artist's new albums and singles. Deezer's listeners give a second
//! opinion ([`crate::deezer`]). YTFast chooses among them by what the
//! listener does, on every device: what YouTube's account says of it
//! (History, Liked Music and Home's "Listen again", [`AccountPlays`]: the
//! listener's phone too) and what YTFast notes itself, on the computer
//! only ([`Listening`]): a song
//! finished, played again or liked counts for it and its artist; one
//! skipped in its first seconds, or disliked, counts against. Each song's
//! genre (Musixmatch's, [`crate::genre`]) says which genres the listener
//! plays, this week and this month ([`Listening::tastes`]), and Radar
//! mixes what it suggests in the same proportions, eased so that a
//! smaller genre keeps some room ([`eased`]). Only songs and artists new
//! to the listener are suggested, never anything marked "Not interested";
//! and a song that the radios of several favourites (or Deezer's too)
//! suggest ranks above one that only one suggests.
//!
//! [`gather`] asks YouTube (at most ten requests, one at a time, once a
//! day: as a listener opening a few radios and artists); [`rank`] chooses,
//! on the computer: the station's pool of new songs, "Artists to try" and
//! "New from your artists"; [`Radar::page`] lays the rows out as Home's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::deezer::{self, Opinion};
use crate::innertube::{ApiError, Session};
use crate::read::{
    Card, CardLook, Item, Page, PageKind, Rating, Section, Shape, Target, Thumb, Track, TrackKind,
};

/// Songs whose radios are asked for.
const SEEDS: usize = 4;
/// Artists whose pages are asked for: with History, Liked Music, Home and
/// the radios, ten requests at most.
const ARTISTS: usize = 3;
/// Seeds and artists are drawn from this many of the best, so that Radar
/// changes from day to day.
const DRAWN_FROM: usize = 12;
/// What a play on another device counts for (one heard to its end in
/// YTFast counts 1): History says it was played, not how much.
const ELSEWHERE: f64 = 0.7;
/// What a song of Home's "Listen again" counts for, fading by half in
/// [`HALF_LIFE_DAYS`] from when it was last listed there.
const OFTEN: f64 = 1.5;
/// The most songs the record keeps: the least recently played go first.
const MOST_SONGS: usize = 3000;
/// A play shorter than this (in seconds), neither finished nor skipped,
/// counts for nothing: a click around.
const BRIEF: f64 = 2.0;
/// Skipped in its first 30 seconds (or first half, for a short song), a
/// song counts against itself.
const EARLY: f64 = 30.0;
/// Before this many seconds (of a song `length` long; 0 when not known),
/// Next skips a song early.
pub fn early(length: f64) -> f64 {
    if length > 0.0 {
        EARLY.min(length / 2.0)
    } else {
        EARLY
    }
}

/// Heard to this part of its length, a song counts as finished.
const MOSTLY: f64 = 0.8;
/// How fast a play's weight fades: by half in 45 days.
const HALF_LIFE_DAYS: f64 = 45.0;
/// New songs kept for the station, and at most this many of one artist.
const POOL_KEPT: usize = 200;
const POOL_PER_ARTIST: usize = 4;
/// Cards kept for each row of the page.
const CARDS_KEPT: usize = 16;
/// Cards shown in a row (more come forward as some are marked Not
/// interested).
const CARDS_SHOWN: usize = 12;
/// At most this many of one artist's releases in "New from your artists".
const RELEASES_PER_ARTIST: usize = 1;
/// Plays are kept with their dates for this many days (for "this week"
/// and "this month"), and at most this many of them.
const LOG_DAYS: u64 = 35;
const LOG_MOST: usize = 6000;
/// Plays (or, lately, songs) of a known genre needed to say which genres
/// the listener plays.
const KNOWN_ENOUGH: usize = 6;
/// Genres the page names, and the mixes follow, at most.
const TOP_GENRES: usize = 4;
/// What [`Radar`] is made by: one made by an older YTFast is made again.
pub const RADAR_VERSION: u32 = 5;
/// Songs of one artist disliked or marked Not interested that leave the
/// artist out too.
const ARTIST_AFTER: usize = 3;
/// The pause between two requests, so that they come as a listener's do.
const PAUSE: Duration = Duration::from_millis(300);
/// Radar is made again after a day (by the computer's clock).
pub const FRESH_FOR: u64 = 24 * 60 * 60;

/// The computer's clock, in seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// How a song stopped playing, for [`Listening::played`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ending {
    /// It played to its end (or the next song followed on).
    Finished,
    /// The listener pressed Next.
    Skipped,
    /// Anything else: another song chosen, YTFast closed.
    #[default]
    Stopped,
}

/// One song the listener played, as the record keeps it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Heard {
    pub title: String,
    pub artists: String,
    pub artist_id: Option<String>,
    pub album_id: Option<String>,
    pub plays: u32,
    /// Of its plays: heard to the end (or nearly), and skipped early.
    pub finished: u32,
    pub skipped: u32,
    /// The account's rating, as last chosen in YTFast.
    pub rating: Option<Rating>,
    /// When it was last played, in seconds since 1970.
    pub last: u64,
    /// Its genre's family ([`crate::genre::family`]): empty when Musixmatch
    /// has none for it, `None` until asked.
    pub genre: Option<String>,
    /// Of its plays: those YouTube's History showed from the listener's
    /// other devices (their phone), heard for an unknown time.
    pub elsewhere: u32,
    /// When YouTube's Home last listed it under "Listen again" (a song the
    /// listener plays often, on any device).
    pub often: Option<u64>,
}

impl Heard {
    /// How much the listener seems to like the song now: finished plays,
    /// other plays a little, a like a lot, early skips against; fading by
    /// half in [`HALF_LIFE_DAYS`]. Nothing for a song disliked.
    fn weight(&self, now: u64) -> f64 {
        if self.rating == Some(Rating::Dislike) {
            return 0.0;
        }
        let other = self
            .plays
            .saturating_sub(self.finished + self.skipped + self.elsewhere);
        let liked = if self.rating == Some(Rating::Like) {
            2.0
        } else {
            0.0
        };
        let base = f64::from(self.finished)
            + ELSEWHERE * f64::from(self.elsewhere)
            + 0.3 * f64::from(other)
            + liked
            - 1.5 * f64::from(self.skipped);
        let fade = |since: u64| {
            let days = now.saturating_sub(since) as f64 / 86_400.0;
            0.5f64.powf(days / HALF_LIFE_DAYS)
        };
        base.max(0.0) * fade(self.last) + self.often.map_or(0.0, |at| OFTEN * fade(at))
    }
}

/// One play, with its date, for what was played when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// When it stopped, in seconds since 1970.
    pub at: u64,
    /// The song (its video ID).
    pub id: String,
    pub ending: Ending,
    /// How long it was heard, in whole seconds.
    pub heard: u32,
    /// How it counted (none for plays noted by 1.6.0: [`Play::outcome`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counted: Option<Outcome>,
    /// Played on another device (YouTube's History showed it), so heard
    /// for an unknown time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elsewhere: bool,
}

impl Play {
    /// How it counted: as noted, else (a play noted by 1.6.0) as its
    /// ending and the time heard say.
    pub fn outcome(&self) -> Outcome {
        self.counted.unwrap_or(match self.ending {
            Ending::Finished => Outcome::Finished,
            Ending::Skipped if f64::from(self.heard) < EARLY => Outcome::Skipped,
            _ => Outcome::Played,
        })
    }
}

/// A like or a dislike chosen in YTFast, with its date (for the listener's
/// numbers: [`crate::stats`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rated {
    /// When, in seconds since 1970.
    pub at: u64,
    /// The song (its video ID), its title and artists.
    pub id: String,
    pub title: String,
    pub artists: String,
    pub rating: Rating,
}

/// How a play counted ([`Listening::played`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Heard to its end, or nearly.
    Finished,
    /// Skipped in its first seconds.
    Skipped,
    /// Neither: stopped somewhere in the middle.
    Played,
}

/// Over which time the listener's genres are told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Week,
    Month,
    /// The record's songs, by how much the listener likes them now (older
    /// plays have no dates).
    Lately,
}

/// The genres the listener plays, over a [`Span`], best first; each with
/// its share of the plays of a known genre (they add up to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Tastes {
    pub span: Span,
    pub genres: Vec<(String, f64)>,
}

impl Tastes {
    /// What the page says of them: "This week you played mostly J-Pop,
    /// Rock and Hip-Hop". The genres of a tenth or more, at most three
    /// (the first one, whatever its share).
    pub fn line(&self) -> String {
        let named: Vec<String> = self
            .genres
            .iter()
            .enumerate()
            .filter(|(i, (_, share))| *i == 0 || *share >= 0.1)
            .take(3)
            .map(|(_, (genre, _))| genre.clone())
            .collect();
        let start = match self.span {
            Span::Week => "This week you played mostly",
            Span::Month => "This month you played mostly",
            Span::Lately => "Lately you play mostly",
        };
        format!("{start} {}", listed(&named))
    }
}

/// Counts as shares of their sum, best first.
fn shares(counts: HashMap<String, f64>) -> Vec<(String, f64)> {
    let sum: f64 = counts.values().sum();
    let mut shares: Vec<(String, f64)> = counts
        .into_iter()
        .filter(|(_, count)| *count > 0.0)
        .map(|(genre, count)| (genre, count / sum))
        .collect();
    shares.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    shares
}

/// The genres' shares eased by their square root, so that the main genre
/// leads and the others keep some room: of 62%, 21% and 9%, about 51%,
/// 30% and 19%. Only the first [`TOP_GENRES`] of a twentieth or more; they
/// add up to 1.
pub fn eased(genres: &[(String, f64)]) -> Vec<(String, f64)> {
    let roots: Vec<(String, f64)> = genres
        .iter()
        .filter(|(_, share)| *share >= 0.05)
        .take(TOP_GENRES)
        .map(|(genre, share)| (genre.clone(), share.sqrt()))
        .collect();
    let sum: f64 = roots.iter().map(|(_, root)| root).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    roots
        .into_iter()
        .map(|(genre, root)| (genre, root / sum))
        .collect()
}

/// `places` shared out by `shares` (the largest remainders get the places
/// left), each genre one at least while places last.
pub fn split(shares: &[(String, f64)], places: usize) -> Vec<(String, usize)> {
    let mut given: Vec<(String, usize, f64)> = shares
        .iter()
        .map(|(genre, share)| {
            let exact = share * places as f64;
            (genre.clone(), exact.floor() as usize, exact.fract())
        })
        .collect();
    let mut left = places.saturating_sub(given.iter().map(|(_, n, _)| n).sum());
    // One for each genre with none, first.
    for entry in given.iter_mut() {
        if left == 0 {
            break;
        }
        if entry.1 == 0 {
            entry.1 = 1;
            entry.2 = 0.0;
            left -= 1;
        }
    }
    let mut order: Vec<usize> = (0..given.len()).collect();
    order.sort_by(|&a, &b| given[b].2.total_cmp(&given[a].2).then(a.cmp(&b)));
    if !order.is_empty() {
        for i in order.into_iter().cycle().take(left) {
            given[i].1 += 1;
        }
    }
    given.into_iter().map(|(genre, n, _)| (genre, n)).collect()
}

/// What the listener marked "Not interested" on the Radar page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unwanted {
    Song(Track),
    Artist {
        id: Option<String>,
        name: String,
    },
    /// An album or single, by its page (`MPREb_...`).
    Release(String),
}

/// What the listener did, noted on the computer only: each song played
/// (at most [`MOST_SONGS`]), and what was disliked or marked "Not
/// interested".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Listening {
    songs: BTreeMap<String, Heard>,
    /// Songs disliked or marked Not interested, each with its artist's
    /// keys ([`artist_keys`]).
    unwanted_songs: BTreeMap<String, Vec<String>>,
    /// Artists' keys.
    unwanted_artists: BTreeSet<String>,
    /// Albums and singles, by their pages.
    unwanted_releases: BTreeSet<String>,
    /// The plays of the last [`LOG_DAYS`] days, oldest first.
    log: Vec<Play>,
    /// The likes and dislikes of the last [`LOG_DAYS`] days, oldest first.
    ratings: Vec<Rated>,
    /// When YouTube's Liked Music was last read
    /// ([`Listening::take_account`]): a like found there since is new.
    likes_read: Option<u64>,
}

/// The keys an artist is known by: its page (`id:UC...`), and its name
/// (`name:...`, the first artist's, in small letters), as some rows carry
/// only one of them.
pub(crate) fn artist_keys(id: Option<&str>, artists: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        keys.push(format!("id:{id}"));
    }
    let first = artists
        .split([',', '&'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !first.is_empty() {
        keys.push(format!("name:{first}"));
    }
    keys
}

pub(crate) fn track_keys(track: &Track) -> Vec<String> {
    artist_keys(track.artist_id.as_deref(), &track.artists)
}

impl Listening {
    /// A song stopped playing after `heard` seconds of its `length` (0 when
    /// not known), at `now` (seconds since 1970). How it counted, unless it
    /// was too short to count (a click around).
    pub fn played(
        &mut self,
        track: &Track,
        heard: f64,
        length: f64,
        ending: Ending,
        now: u64,
    ) -> Option<Outcome> {
        if track.video_id.is_empty() {
            return None;
        }
        let skipped = ending == Ending::Skipped && heard < early(length);
        let finished = ending == Ending::Finished || (length > 0.0 && heard >= length * MOSTLY);
        if !skipped && !finished && heard < BRIEF {
            return None;
        }
        let song = self.songs.entry(track.video_id.clone()).or_default();
        if !track.title.is_empty() {
            song.title.clone_from(&track.title);
        }
        if !track.artists.is_empty() {
            song.artists.clone_from(&track.artists);
        }
        if track.artist_id.is_some() {
            song.artist_id.clone_from(&track.artist_id);
        }
        if track.album_id.is_some() {
            song.album_id.clone_from(&track.album_id);
        }
        song.plays += 1;
        let outcome = if finished {
            song.finished += 1;
            Outcome::Finished
        } else if skipped {
            song.skipped += 1;
            Outcome::Skipped
        } else {
            Outcome::Played
        };
        song.last = now;
        self.log.push(Play {
            at: now,
            id: track.video_id.clone(),
            ending,
            heard: heard.max(0.0).round() as u32,
            counted: Some(outcome),
            elsewhere: false,
        });
        self.trim(now);
        Some(outcome)
    }

    /// Keeps at most [`MOST_SONGS`], letting go of those played longest
    /// ago, and the plays of the last [`LOG_DAYS`] (at most [`LOG_MOST`]).
    fn trim(&mut self, now: u64) {
        let since = now.saturating_sub(LOG_DAYS * 86_400);
        self.log.retain(|play| play.at >= since);
        let over_log = self.log.len().saturating_sub(LOG_MOST);
        self.log.drain(..over_log);
        self.ratings.retain(|rated| rated.at >= since);
        let over_ratings = self.ratings.len().saturating_sub(LOG_MOST);
        self.ratings.drain(..over_ratings);
        let over = self.songs.len().saturating_sub(MOST_SONGS);
        if over == 0 {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self
            .songs
            .iter()
            .map(|(id, song)| (song.last, id.clone()))
            .collect();
        by_age.sort();
        for (_, id) in by_age.into_iter().take(over) {
            self.songs.remove(&id);
        }
    }

    /// The account's rating of a song changed in YTFast, at `now`
    /// (seconds since 1970). Disliked, it is never suggested (and, with
    /// others of its artist, nor is the artist); liked or neither, it may
    /// be again.
    pub fn rated(&mut self, track: &Track, rating: Rating, now: u64) {
        let song = self.songs.get_mut(&track.video_id);
        let (title, artists) = match &song {
            Some(song) if track.title.is_empty() => (song.title.clone(), song.artists.clone()),
            _ => (track.title.clone(), track.artists.clone()),
        };
        if let Some(song) = song {
            song.rating = Some(rating);
        }
        if !track.video_id.is_empty() {
            self.ratings.push(Rated {
                at: now,
                id: track.video_id.clone(),
                title,
                artists,
                rating,
            });
            self.trim(now);
        }
        match rating {
            Rating::Dislike => self.unwant_song(track),
            Rating::Like | Rating::Indifferent => {
                self.unwanted_songs.remove(&track.video_id);
            }
        }
    }

    /// Takes in what YouTube's account says of the listener's playing on
    /// every device ([`AccountPlays`]), at `now`. Each song of History not
    /// yet noted in its group's time becomes a play from another device:
    /// so reading the same History again adds nothing, nor does a song
    /// YTFast played then (History lists those too). Likes and dislikes
    /// are noted; a like is dated (for the listener's numbers) only when
    /// it is new at the top of Liked Music, a dislike when History shows
    /// it (a song disliked as it played, most likely). A song of "Listen
    /// again" counts as one played often.
    pub fn take_account(&mut self, account: &AccountPlays, now: u64) {
        if account.tracks().next().is_none() {
            // Nothing read (every request failed): nothing to note, and
            // Liked Music still to be read for the first time.
            return;
        }
        // Liked Music's first reading holds old likes, not new ones.
        let likes_dated = self.likes_read.is_some();
        for track in &account.liked {
            self.account_rating(track, Rating::Like, now, likes_dated);
        }
        if !account.liked.is_empty() {
            self.likes_read = Some(now);
        }
        for (track, ago) in &account.history {
            let (newest, oldest, taken) = ago.hours();
            let since = now.saturating_sub(oldest * 3600);
            let until = now.saturating_sub(newest * 3600);
            let noted = self
                .log
                .iter()
                .any(|p| p.id == track.video_id && p.at >= since && p.at <= until);
            if !noted {
                let at = now.saturating_sub(taken * 3600);
                let song = self.heard_from(track, at);
                song.plays += 1;
                song.elsewhere += 1;
                song.last = song.last.max(at);
                self.log.push(Play {
                    at,
                    id: track.video_id.clone(),
                    ending: Ending::Stopped,
                    heard: 0,
                    counted: Some(Outcome::Played),
                    elsewhere: true,
                });
            }
            match track.like {
                // Liked songs are played again and again: when one was
                // liked, History cannot say.
                Some(Rating::Like) => self.account_rating(track, Rating::Like, now, false),
                Some(Rating::Dislike) => self.account_rating(track, Rating::Dislike, now, true),
                _ => {}
            }
        }
        for track in &account.often {
            self.heard_from(track, now).often = Some(now);
        }
        // The plays from History went in at their own times.
        self.log.sort_by_key(|play| play.at);
        self.trim(now);
    }

    /// The record's entry for a song the account names (made when new, as
    /// last played `at`), with what it lacks of its name filled in.
    fn heard_from(&mut self, track: &Track, at: u64) -> &mut Heard {
        let song = self
            .songs
            .entry(track.video_id.clone())
            .or_insert_with(|| Heard {
                last: at,
                ..Heard::default()
            });
        if song.title.is_empty() {
            song.title.clone_from(&track.title);
        }
        if song.artists.is_empty() {
            song.artists.clone_from(&track.artists);
        }
        if song.artist_id.is_none() {
            song.artist_id.clone_from(&track.artist_id);
        }
        if song.album_id.is_none() {
            song.album_id.clone_from(&track.album_id);
        }
        song
    }

    /// A like or dislike the account holds, unless the record has it
    /// already: `dated` as new at `now` (for the listener's numbers), or
    /// only noted.
    fn account_rating(&mut self, track: &Track, rating: Rating, now: u64, dated: bool) {
        if self.songs.get(&track.video_id).and_then(|s| s.rating) == Some(rating) {
            return;
        }
        self.heard_from(track, now);
        if dated {
            self.rated(track, rating, now);
            return;
        }
        if let Some(song) = self.songs.get_mut(&track.video_id) {
            song.rating = Some(rating);
        }
        match rating {
            Rating::Dislike => self.unwant_song(track),
            Rating::Like | Rating::Indifferent => {
                self.unwanted_songs.remove(&track.video_id);
            }
        }
    }

    /// "Not interested", on the Radar page.
    pub fn not_interested(&mut self, what: &Unwanted) {
        match what {
            Unwanted::Song(track) => self.unwant_song(track),
            Unwanted::Artist { id, name } => {
                self.unwanted_artists
                    .extend(artist_keys(id.as_deref(), name));
            }
            Unwanted::Release(id) => {
                self.unwanted_releases.insert(id.clone());
            }
        }
    }

    fn unwant_song(&mut self, track: &Track) {
        let mut keys = track_keys(track);
        if keys.is_empty()
            && let Some(song) = self.songs.get(&track.video_id)
        {
            keys = artist_keys(song.artist_id.as_deref(), &song.artists);
        }
        self.unwanted_songs
            .insert(track.video_id.clone(), keys.clone());
        // Enough of one artist's songs unwanted: the artist too.
        for key in keys {
            let count = self
                .unwanted_songs
                .values()
                .filter(|other| other.contains(&key))
                .count();
            if count >= ARTIST_AFTER {
                self.unwanted_artists.insert(key);
            }
        }
    }

    /// Forgets everything noted (Settings).
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
            && self.unwanted_songs.is_empty()
            && self.unwanted_artists.is_empty()
            && self.unwanted_releases.is_empty()
            && self.log.is_empty()
            && self.ratings.is_empty()
    }

    /// The plays of the last [`LOG_DAYS`] days, oldest first.
    pub fn log(&self) -> &[Play] {
        &self.log
    }

    /// The likes and dislikes chosen in YTFast in the last [`LOG_DAYS`]
    /// days, oldest first.
    pub fn ratings(&self) -> &[Rated] {
        &self.ratings
    }

    /// Each song's genre (its own, else its artist's), by its video ID:
    /// the songs of the record whose genre is known.
    pub fn song_genres(&self) -> HashMap<String, String> {
        let artists = self.artist_genres();
        self.songs
            .iter()
            .filter_map(|(id, song)| {
                let genre =
                    self.genre_with(&artists, id, song.artist_id.as_deref(), &song.artists)?;
                Some((id.clone(), genre))
            })
            .collect()
    }

    /// A song's genre, as Musixmatch gave it (`None`: it has none). True
    /// when the record changed. A song not in the record is left alone.
    pub fn set_genre(&mut self, video_id: &str, genre: Option<String>) -> bool {
        let Some(song) = self.songs.get_mut(video_id) else {
            return false;
        };
        let genre = Some(genre.unwrap_or_default());
        if song.genre == genre {
            return false;
        }
        song.genre = genre;
        true
    }

    /// Whether a song's genre is still to be asked for.
    pub fn wants_genre(&self, video_id: &str) -> bool {
        self.songs
            .get(video_id)
            .is_some_and(|song| song.genre.is_none())
    }

    /// The songs liked best whose genre is still to be asked for, at most
    /// `count` (for songs played before YTFast asked for genres, and on the
    /// listener's other devices): one of each artist, and none of an
    /// artist whose genre is known (their songs take it).
    pub fn missing_genres(&self, count: usize, now: u64) -> Vec<Track> {
        let known = self.artist_genres();
        let mut missing: Vec<(f64, &String, &Heard)> = self
            .songs
            .iter()
            .filter(|(_, song)| song.genre.is_none() && !song.title.is_empty())
            .map(|(id, song)| (song.weight(now), id, song))
            .filter(|(weight, _, _)| *weight > 0.0)
            .collect();
        missing.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
        let mut chosen: HashSet<String> = HashSet::new();
        missing
            .into_iter()
            .filter(|(_, _, song)| {
                let keys = artist_keys(song.artist_id.as_deref(), &song.artists);
                let taken = keys
                    .iter()
                    .any(|k| known.contains_key(k) || chosen.contains(k));
                chosen.extend(keys);
                !taken
            })
            .take(count)
            .map(|(_, id, song)| Track {
                video_id: id.clone(),
                title: song.title.clone(),
                artists: song.artists.clone(),
                artist_id: song.artist_id.clone(),
                album_id: song.album_id.clone(),
                ..Track::default()
            })
            .collect()
    }

    /// Each artist's genre, by its keys ([`artist_keys`]): the one most of
    /// its songs in the record have.
    fn artist_genres(&self) -> HashMap<String, String> {
        let mut counts: HashMap<String, HashMap<&str, usize>> = HashMap::new();
        for song in self.songs.values() {
            let Some(genre) = song.genre.as_deref().filter(|g| !g.is_empty()) else {
                continue;
            };
            for key in artist_keys(song.artist_id.as_deref(), &song.artists) {
                *counts.entry(key).or_default().entry(genre).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .filter_map(|(key, genres)| {
                let best = genres
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))?;
                Some((key, best.0.to_string()))
            })
            .collect()
    }

    /// A song's genre: its own, else the one its artist's other songs have.
    pub fn genre_for(&self, track: &Track) -> Option<String> {
        self.genre_with(
            &self.artist_genres(),
            &track.video_id,
            track.artist_id.as_deref(),
            &track.artists,
        )
    }

    fn genre_with(
        &self,
        artists: &HashMap<String, String>,
        video_id: &str,
        artist_id: Option<&str>,
        names: &str,
    ) -> Option<String> {
        let own = self
            .songs
            .get(video_id)
            .and_then(|song| song.genre.clone())
            .filter(|genre| !genre.is_empty());
        own.or_else(|| {
            artist_keys(artist_id, names)
                .iter()
                .find_map(|key| artists.get(key).cloned())
        })
    }

    /// The genres the listener plays: this week's when enough of its plays
    /// have a known genre, else this month's, else the record's songs by
    /// how much the listener likes them now. Early skips do not count.
    /// `None` until enough is known.
    pub fn tastes(&self, now: u64) -> Option<Tastes> {
        let artists = self.artist_genres();
        let genre_of = |id: &str| {
            let song = self.songs.get(id)?;
            self.genre_with(&artists, id, song.artist_id.as_deref(), &song.artists)
        };
        for (span, days) in [(Span::Week, 7), (Span::Month, 30)] {
            let since = now.saturating_sub(days * 86_400);
            let mut counts: HashMap<String, f64> = HashMap::new();
            let mut known = 0;
            for play in self.log.iter().filter(|p| p.at >= since) {
                let early = play.ending == Ending::Skipped && f64::from(play.heard) < EARLY;
                if early {
                    continue;
                }
                if let Some(genre) = genre_of(&play.id) {
                    *counts.entry(genre).or_insert(0.0) += 1.0;
                    known += 1;
                }
            }
            if known >= KNOWN_ENOUGH {
                return Some(Tastes {
                    span,
                    genres: shares(counts),
                });
            }
        }
        let mut weights: HashMap<String, f64> = HashMap::new();
        let mut known = 0;
        for (id, song) in &self.songs {
            let weight = song.weight(now);
            if weight <= 0.0 {
                continue;
            }
            if let Some(genre) = genre_of(id) {
                *weights.entry(genre).or_insert(0.0) += weight;
                known += 1;
            }
        }
        (known >= KNOWN_ENOUGH).then(|| Tastes {
            span: Span::Lately,
            genres: shares(weights),
        })
    }

    /// The songs noted, by ID.
    pub fn songs(&self) -> &BTreeMap<String, Heard> {
        &self.songs
    }

    /// The listener has played this song in YTFast.
    pub fn knows(&self, video_id: &str) -> bool {
        self.songs.contains_key(video_id)
    }

    /// Every song the record holds, by its name and first artist
    /// ([`deezer::song_key`]): another version of one is no new song.
    pub fn song_keys(&self) -> HashSet<String> {
        self.songs
            .values()
            .filter(|song| !song.title.is_empty())
            .map(|song| deezer::song_key(&song.title, &song.artists))
            .collect()
    }

    /// The song, or its artist, is never to be suggested.
    pub fn unwanted(&self, track: &Track) -> bool {
        self.unwanted_songs.contains_key(&track.video_id)
            || track_keys(track)
                .iter()
                .any(|key| self.unwanted_artists.contains(key))
    }

    fn unwanted_artist(&self, id: Option<&str>, name: &str) -> bool {
        artist_keys(id, name)
            .iter()
            .any(|key| self.unwanted_artists.contains(key))
    }

    /// The songs whose radios are asked for, drawn from the listener's
    /// favourites (the day's draw: Radar changes from day to day), one
    /// per artist, each genre the listener plays its share
    /// ([`draw_by_genre`]); filled from `history` and `liked` (YouTube's)
    /// when the record has too few.
    pub fn seeds(&self, history: &[Track], liked: &[Track], now: u64) -> Vec<Seed> {
        let artists = self.artist_genres();
        let mut best: Vec<Seed> = self
            .songs
            .iter()
            .filter(|(id, _)| !self.unwanted_songs.contains_key(*id))
            .map(|(id, song)| Seed {
                video_id: id.clone(),
                title: song.title.clone(),
                keys: artist_keys(song.artist_id.as_deref(), &song.artists),
                weight: song.weight(now),
                genre: self.genre_with(&artists, id, song.artist_id.as_deref(), &song.artists),
            })
            .filter(|seed| seed.weight > 0.0)
            .collect();
        best.sort_by(|a, b| {
            b.weight
                .total_cmp(&a.weight)
                .then(a.video_id.cmp(&b.video_id))
        });
        let tastes = self.tastes(now);
        let mut drawn = draw_by_genre(
            one_per_artist(best),
            SEEDS,
            now,
            tastes.as_ref(),
            |s| s.weight,
            |s| s.genre.as_deref(),
        );
        // YouTube's, newest first, each less than the last.
        let mut extra = 0.5;
        for track in history.iter().chain(liked) {
            if drawn.len() >= SEEDS {
                break;
            }
            let keys = track_keys(track);
            let taken = drawn
                .iter()
                .any(|s| s.video_id == track.video_id || s.keys.iter().any(|k| keys.contains(k)));
            if taken
                || !track.playable
                || track.like == Some(Rating::Dislike)
                || self.unwanted(track)
            {
                continue;
            }
            drawn.push(Seed {
                video_id: track.video_id.clone(),
                title: track.title.clone(),
                keys,
                weight: extra,
                genre: self.genre_with(
                    &artists,
                    &track.video_id,
                    track.artist_id.as_deref(),
                    &track.artists,
                ),
            });
            extra *= 0.9;
        }
        drawn
    }

    /// The artists whose pages are asked for: those the listener plays
    /// most (the day's draw, each genre its share, as
    /// [`Listening::seeds`]), filled from YouTube's `history` and `liked`.
    /// Only artists with a page.
    pub fn top_artists(&self, history: &[Track], liked: &[Track], now: u64) -> Vec<ArtistSeed> {
        let genres = self.artist_genres();
        let genre_of = |id: &str, names: &str| {
            artist_keys(Some(id), names)
                .iter()
                .find_map(|key| genres.get(key).cloned())
        };
        let mut by_artist: HashMap<String, ArtistSeed> = HashMap::new();
        for song in self.songs.values() {
            let Some(id) = song.artist_id.as_deref().filter(|id| !id.is_empty()) else {
                continue;
            };
            if self.unwanted_artist(Some(id), &song.artists) {
                continue;
            }
            let weight = song.weight(now);
            let artist = by_artist
                .entry(id.to_string())
                .or_insert_with(|| ArtistSeed {
                    id: id.to_string(),
                    name: first_artist(&song.artists),
                    weight: 0.0,
                    genre: genre_of(id, &song.artists),
                });
            artist.weight += weight;
        }
        let mut best: Vec<ArtistSeed> =
            by_artist.into_values().filter(|a| a.weight > 0.0).collect();
        best.sort_by(|a, b| b.weight.total_cmp(&a.weight).then(a.id.cmp(&b.id)));
        let tastes = self.tastes(now);
        let mut drawn = draw_by_genre(
            best,
            ARTISTS,
            now.wrapping_add(1),
            tastes.as_ref(),
            |a| a.weight,
            |a| a.genre.as_deref(),
        );
        let mut extra = 0.5;
        for track in history.iter().chain(liked) {
            if drawn.len() >= ARTISTS {
                break;
            }
            let Some(id) = track.artist_id.as_deref().filter(|id| !id.is_empty()) else {
                continue;
            };
            if drawn.iter().any(|a| a.id == id) || self.unwanted_artist(Some(id), &track.artists) {
                continue;
            }
            drawn.push(ArtistSeed {
                id: id.to_string(),
                name: first_artist(&track.artists),
                weight: extra,
                genre: genre_of(id, &track.artists),
            });
            extra *= 0.9;
        }
        drawn
    }
}

/// The first of a row's artists ("YOASOBI" of "YOASOBI, Ayase").
fn first_artist(artists: &str) -> String {
    artists
        .split([',', '&'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The best of `best` (best first), one per artist.
fn one_per_artist(best: Vec<Seed>) -> Vec<Seed> {
    let mut seen: HashSet<String> = HashSet::new();
    best.into_iter()
        .filter(|seed| {
            let new = seed.keys.iter().all(|k| !seen.contains(k));
            seen.extend(seed.keys.iter().cloned());
            new
        })
        .collect()
}

/// The places (in `weights`, best first) of `count` of the first
/// [`DRAWN_FROM`], each as likely as its weight (Efraimidis and Spirakis'
/// draw), in the order drawn; the same draw for the same `day`.
fn draw_places(weights: &[f64], count: usize, day: u64) -> Vec<usize> {
    let mut random = Random::new(day);
    let mut keyed: Vec<(f64, usize)> = weights
        .iter()
        .take(DRAWN_FROM)
        .enumerate()
        .map(|(i, w)| (random.unit().powf(1.0 / w.max(1e-9)), i))
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    keyed.into_iter().take(count).map(|(_, i)| i).collect()
}

/// `count` of the first [`DRAWN_FROM`] of `best`, each as likely as its
/// weight, in the order drawn; the same draw all day, another the next.
fn draw<T>(best: Vec<T>, count: usize, now: u64, weight: impl Fn(&T) -> f64) -> Vec<T> {
    let weights: Vec<f64> = best.iter().map(&weight).collect();
    let places = draw_places(&weights, count, now / 86_400);
    let mut slots: Vec<Option<T>> = best.into_iter().map(Some).collect();
    places
        .into_iter()
        .filter_map(|i| slots.get_mut(i).and_then(Option::take))
        .collect()
}

/// As [`draw`], but each genre the listener plays gets its share of the
/// `count` places ([`eased`], [`split`]), drawn from its own best; places
/// a genre cannot fill go to the best of the rest. Without `tastes`, a
/// plain [`draw`].
fn draw_by_genre<T>(
    best: Vec<T>,
    count: usize,
    now: u64,
    tastes: Option<&Tastes>,
    weight: impl Fn(&T) -> f64,
    genre: impl Fn(&T) -> Option<&str>,
) -> Vec<T> {
    let targets = tastes
        .map(|t| split(&eased(&t.genres), count))
        .unwrap_or_default();
    if targets.is_empty() {
        return draw(best, count, now, weight);
    }
    let mut slots: Vec<Option<T>> = best.into_iter().map(Some).collect();
    let mut drawn = Vec::new();
    for (n, (wanted, places)) in targets.iter().enumerate() {
        let of_genre: Vec<(usize, f64)> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.as_ref()
                    .filter(|item| genre(item) == Some(wanted.as_str()))
                    .map(|item| (i, weight(item)))
            })
            .collect();
        let weights: Vec<f64> = of_genre.iter().map(|(_, w)| *w).collect();
        // Another draw for each genre, the same all day.
        let day = now / 86_400 + (n as u64 + 1) * 1_000_003;
        for place in draw_places(&weights, *places, day) {
            if let Some(item) = slots[of_genre[place].0].take() {
                drawn.push(item);
            }
        }
    }
    let left = count.saturating_sub(drawn.len());
    let rest: Vec<T> = slots.into_iter().flatten().collect();
    drawn.extend(draw(rest, left, now, weight));
    drawn
}

/// A small random number maker (xorshift), so that a day's draw is always
/// the same.
pub(crate) struct Random(u64);

impl Random {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// A number in (0, 1).
    pub(crate) fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
}

/// A song whose radio Radar asks for, how much the listener likes it, and
/// its genre.
#[derive(Clone, Debug, PartialEq)]
pub struct Seed {
    pub video_id: String,
    pub title: String,
    /// Its artist's keys ([`artist_keys`]).
    keys: Vec<String>,
    pub weight: f64,
    pub genre: Option<String>,
}

/// An artist whose page Radar asks for, how much the listener plays them,
/// and their genre.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtistSeed {
    pub id: String,
    pub name: String,
    pub weight: f64,
    pub genre: Option<String>,
}

/// What the listener's YouTube account says of their listening on every
/// device (their phone too), read when Radar is made ([`gather`]):
/// History's songs, each with when it was last played (History's groups:
/// it lists a song once, at its latest play), the newest of Liked Music,
/// and Home's "Listen again" (songs played often).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountPlays {
    pub history: Vec<(Track, Ago)>,
    pub liked: Vec<Track>,
    pub often: Vec<Track>,
}

impl AccountPlays {
    /// History's songs from its page, each with its group; songs of older
    /// groups, and podcasts' episodes, are left out.
    pub fn history_of(page: &Page) -> Vec<(Track, Ago)> {
        let episode = |track: &Track| matches!(&track.kind, TrackKind::Other(kind) if kind.contains("PODCAST"));
        page.sections
            .iter()
            .filter_map(|section| Some((Ago::of(&section.title)?, section)))
            .flat_map(|(ago, section)| {
                section.items.iter().filter_map(move |item| match item {
                    Item::Track(track) if !track.video_id.is_empty() && !episode(track) => {
                        Some((track.clone(), ago))
                    }
                    _ => None,
                })
            })
            .collect()
    }

    /// The songs of Home's "Listen again" (its albums, artists and
    /// podcasts left out).
    pub fn often_of(home: &Page) -> Vec<Track> {
        home.sections
            .iter()
            .filter(|section| section.title.eq_ignore_ascii_case("Listen again"))
            .flat_map(|section| &section.items)
            .filter_map(|item| match item {
                Item::Card(Card {
                    open:
                        Some(Target::Browse {
                            kind: PageKind::Episode | PageKind::Podcast,
                            ..
                        }),
                    ..
                }) => None,
                Item::Card(card) => card.song(),
                Item::Track(track) => Some(track.clone()),
            })
            .collect()
    }

    /// Every song it names, for their names' fonts.
    pub fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.history
            .iter()
            .map(|(track, _)| track)
            .chain(&self.liked)
            .chain(&self.often)
    }
}

/// When History says a song was last played: its group, by the account's
/// own clock (which may be a day off this computer's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ago {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
}

impl Ago {
    /// The group a History section's title names (English, as every
    /// request asks for it).
    pub fn of(title: &str) -> Option<Self> {
        match title.trim().to_lowercase().as_str() {
            "today" => Some(Self::Today),
            "yesterday" => Some(Self::Yesterday),
            "this week" => Some(Self::ThisWeek),
            "last week" => Some(Self::LastWeek),
            _ => None,
        }
    }

    /// Hours ago: the newest and the oldest the play may be (a day's
    /// leeway either way), and when it is taken to have been.
    fn hours(self) -> (u64, u64, u64) {
        match self {
            Self::Today => (0, 30, 2),
            Self::Yesterday => (18, 54, 24),
            Self::ThisWeek => (36, 180, 96),
            Self::LastWeek => (156, 348, 240),
        }
    }
}

/// What YouTube said, for [`rank`].
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// The account's History, newest first, and its Liked Music's first
    /// page.
    pub history: Vec<Track>,
    pub liked: Vec<Track>,
    /// What the account says of the listener's playing on every device,
    /// taken into the record ([`Listening::take_account`]).
    pub account: AccountPlays,
    /// The songs whose radios were asked for, each with its radio (empty
    /// when it did not load).
    pub radios: Vec<(Seed, Vec<Track>)>,
    /// The artists whose pages were asked for, each with its page.
    pub artists: Vec<(ArtistSeed, Page)>,
    /// What Deezer's listeners say about those artists (nothing when it
    /// did not answer).
    pub opinion: Opinion,
}

/// Asks YouTube for what Radar chooses from: the account's History, Liked
/// Music and Home (what the listener plays on every device, taken into
/// `listening` first, [`Listening::take_account`]), then the radios of a
/// few favourite songs and the pages of a few favourite artists. At most
/// ten requests, one at a time. A part that does not load is left out;
/// only when nothing does is it an error.
pub async fn gather(
    session: &Session,
    listening: &mut Listening,
    now: u64,
) -> Result<Found, ApiError> {
    let mut found = Found::default();
    let mut failed: Option<ApiError> = None;
    let mut loaded = false;
    let mut note = |result: Result<(), ApiError>| match result {
        Ok(()) => loaded = true,
        Err(e) => {
            log::info!("Radar: part of it did not load: {e}");
            failed.get_or_insert(e);
        }
    };
    note(session.history_page().await.map(|page| {
        found.account.history = AccountPlays::history_of(&page);
        found.history = page.tracks();
    }));
    tokio::time::sleep(PAUSE).await;
    note(session.liked_songs().await.map(|page| {
        found.account.liked.clone_from(&page.tracks);
        found.liked = page.tracks;
    }));
    tokio::time::sleep(PAUSE).await;
    note(
        session
            .home()
            .await
            .map(|page| found.account.often = AccountPlays::often_of(&page)),
    );
    // What the listener plays on every device, before choosing from it.
    listening.take_account(&found.account, now);
    for seed in listening.seeds(&found.history, &found.liked, now) {
        tokio::time::sleep(PAUSE).await;
        let radio = session.radio_songs(&seed.video_id).await;
        let (tracks, result) = match radio {
            Ok(tracks) => (tracks, Ok(())),
            Err(e) => (Vec::new(), Err(e)),
        };
        note(result);
        found.radios.push((seed, tracks));
    }
    for artist in listening.top_artists(&found.history, &found.liked, now) {
        tokio::time::sleep(PAUSE).await;
        let page = session.page(&artist.id, None).await;
        let (page, result) = match page {
            Ok(page) => (page, Ok(())),
            Err(e) => (Page::default(), Err(e)),
        };
        note(result);
        found.artists.push((artist, page));
    }
    match failed {
        Some(error) if !loaded => Err(error),
        _ => Ok(found),
    }
}

/// An artist suggested in "Artists to try".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistPick {
    /// Its page (`UC...`).
    pub id: String,
    pub name: String,
    /// As YouTube writes it ("2.61M monthly audience").
    pub subtitle: String,
    pub thumbnail: Option<Thumb>,
    /// The genre of the favourites it is like.
    pub genre: Option<String>,
    /// Its "Shuffle play" and "Start mix", YouTube's own lists, as the card
    /// it came from offered them.
    pub shuffle: Option<String>,
    pub mix: Option<String>,
}

/// A new song for the station ([`crate::station`]), and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pick {
    pub track: Track,
    /// How well it suits the listener, before the station's own reckoning.
    pub score: f64,
    /// The genre it is played for: that of the favourite whose radio
    /// suggested it most.
    pub genre: Option<String>,
    /// The songs whose radios suggested it (their video IDs).
    pub seeds: Vec<String>,
}

/// An album or single in "New from your artists".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleasePick {
    /// Its page (`MPREb_...`).
    pub id: String,
    pub title: String,
    /// "Single", "EP" or "Album".
    pub kind: String,
    pub artist: String,
    pub year: i32,
    pub thumbnail: Option<Thumb>,
    /// What its play button plays (`OLAK5uy_...`).
    pub playlist_id: Option<String>,
}

/// What Radar suggests, as chosen on the day it was made; [`Radar::page`]
/// shows it, leaving out what has since been marked unwanted, and the
/// station ([`crate::station`]) plays from its pool.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Radar {
    /// When it was made, in seconds since 1970.
    pub made: u64,
    /// [`RADAR_VERSION`], when made by this YTFast.
    pub version: u32,
    /// The station's new songs, best first.
    pub pool: Vec<Pick>,
    /// The songs the listener knew from YouTube (History, Liked Music)
    /// when it was made: the station leaves them out of new radios too.
    pub known: Vec<String>,
    /// "Artists to try", mixed by the genres the listener plays.
    pub artists: Vec<ArtistPick>,
    /// The artists they are like (for its line).
    pub like: Vec<String>,
    /// "New from your artists", newest first.
    pub releases: Vec<ReleasePick>,
    /// Deezer's IDs of favourite artists (by [`deezer::name_key`]), kept
    /// for the next Radar.
    pub deezer_ids: BTreeMap<String, u64>,
    /// What the account said of the listener's playing on every device
    /// when it was made, for the window's own record (not kept).
    #[serde(skip)]
    pub account: AccountPlays,
}

/// What the listener knows already: the songs and artists noted, and
/// those of YouTube's History and Liked Music, the songs by their names
/// too.
struct Known {
    songs: HashSet<String>,
    names: HashSet<String>,
    artists: HashSet<String>,
}

impl Known {
    fn of(listening: &Listening, found: &Found) -> Self {
        let mut songs: HashSet<String> = listening.songs.keys().cloned().collect();
        let mut names = listening.song_keys();
        let mut artists: HashSet<String> = HashSet::new();
        for song in listening.songs.values() {
            artists.extend(artist_keys(song.artist_id.as_deref(), &song.artists));
        }
        for track in found.history.iter().chain(&found.liked) {
            songs.insert(track.video_id.clone());
            if !track.title.is_empty() {
                names.insert(deezer::song_key(&track.title, &track.artists));
            }
            artists.extend(track_keys(track));
        }
        Self {
            songs,
            names,
            artists,
        }
    }

    fn song(&self, track: &Track) -> bool {
        knows_song(track, |id| self.songs.contains(id), &self.names)
    }

    fn artist(&self, id: Option<&str>, name: &str) -> bool {
        artist_keys(id, name)
            .iter()
            .any(|key| self.artists.contains(key))
    }
}

/// Whether `track` is a song the listener knows: by its ID (`ids`), by
/// its pair's (the music video of a song known, or the song of a video
/// known), or by its name and first artist among `names`
/// ([`Listening::song_keys`]: another version of a song known, as a
/// single's copy on an album).
pub(crate) fn knows_song(
    track: &Track,
    ids: impl Fn(&str) -> bool,
    names: &HashSet<String>,
) -> bool {
    ids(&track.video_id)
        || track.video().is_some_and(&ids)
        || (!track.title.is_empty()
            && names.contains(&deezer::song_key(&track.title, &track.artists)))
}

/// How much more a song counts when Deezer's listeners agree: the song
/// in a favourite's Deezer radio, its artist related to one.
const DEEZER_SONG: f64 = 1.6;
const DEEZER_ARTIST: f64 = 1.25;

/// Chooses what Radar suggests from what YouTube (and Deezer) said
/// ([`gather`]), at `now` (seconds since 1970).
pub fn rank(listening: &Listening, found: &Found, now: u64) -> Radar {
    let known = Known::of(listening, found);
    let tastes = listening.tastes(now);
    let pool = new_songs(listening, found, &known);
    let (artists, like) = artists_to_try(listening, found, &known, tastes.as_ref());
    let releases = new_releases(listening, found, now);
    let mut known_songs: Vec<String> = found
        .history
        .iter()
        .chain(&found.liked)
        .map(|t| t.video_id.clone())
        .filter(|id| !id.is_empty())
        .collect();
    known_songs.sort();
    known_songs.dedup();
    Radar {
        made: now,
        version: RADAR_VERSION,
        pool,
        known: known_songs,
        artists,
        like,
        releases,
        deezer_ids: found.opinion.ids.clone(),
        account: found.account.clone(),
    }
}

/// The station's pool: the songs the seeds' radios suggest (songs only:
/// no music videos or fans' uploads), new to the listener, each scored by
/// its seed's weight and how near the radio's start it stands; a song several radios suggest gains half again for
/// each more, and more again when Deezer's listeners agree. Each is
/// played for the genre of the seed that suggested it most.
fn new_songs(listening: &Listening, found: &Found, known: &Known) -> Vec<Pick> {
    struct Candidate {
        track: Track,
        score: f64,
        /// Each seed that suggested it (its place in `found.radios`), and
        /// how much.
        seeds: Vec<(usize, f64)>,
    }
    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    for (index, (seed, radio)) in found.radios.iter().enumerate() {
        for (place, track) in radio.iter().enumerate() {
            let new = track.video_id != seed.video_id && !known.song(track);
            let wanted = track.kind == TrackKind::Song
                && track.playable
                && track
                    .like
                    .is_none_or(|rating| rating == Rating::Indifferent)
                && !listening.unwanted(track);
            if !new || !wanted || track.video_id.is_empty() {
                continue;
            }
            let gain = seed.weight / (1.0 + place as f64 / 10.0);
            let candidate = candidates
                .entry(track.video_id.clone())
                .or_insert_with(|| Candidate {
                    track: track.clone(),
                    score: 0.0,
                    seeds: Vec::new(),
                });
            candidate.score += gain;
            match candidate.seeds.iter_mut().find(|(i, _)| *i == index) {
                Some((_, part)) => *part += gain,
                None => candidate.seeds.push((index, gain)),
            }
        }
    }
    let mut ranked: Vec<Candidate> = candidates
        .into_values()
        .map(|mut c| {
            c.score *= 1.0 + 0.5 * (c.seeds.len() as f64 - 1.0);
            let (artist, song) = found.opinion.agrees(&c.track.title, &c.track.artists);
            if song {
                c.score *= DEEZER_SONG;
            }
            if artist {
                c.score *= DEEZER_ARTIST;
            }
            c
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.track.video_id.cmp(&b.track.video_id))
    });
    few_per_artist(ranked, POOL_KEPT, POOL_PER_ARTIST, |c| track_keys(&c.track))
        .into_iter()
        .map(|c| {
            let most = c
                .seeds
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(i, _)| *i);
            Pick {
                genre: most.and_then(|i| found.radios[i].0.genre.clone()),
                seeds: c
                    .seeds
                    .iter()
                    .map(|(i, _)| found.radios[*i].0.video_id.clone())
                    .collect(),
                score: c.score,
                track: c.track,
            }
        })
        .collect()
}

/// The first `count` of `ranked`, at most `per` of one artist.
fn few_per_artist<T>(
    ranked: Vec<T>,
    count: usize,
    per: usize,
    keys: impl Fn(&T) -> Vec<String>,
) -> Vec<T> {
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut picked = Vec::new();
    for item in ranked {
        let keys = keys(&item);
        if keys
            .iter()
            .any(|k| per_artist.get(k).copied().unwrap_or(0) >= per)
        {
            continue;
        }
        for key in keys {
            *per_artist.entry(key).or_insert(0) += 1;
        }
        picked.push(item);
        if picked.len() >= count {
            break;
        }
    }
    picked
}

/// The section of an artist's page whose cards are other artists ("Fans
/// might also like"): by its title, as every request asks for English,
/// else the first of round artist cards.
fn similar_artists(page: &Page) -> Vec<&Card> {
    fn artist_cards(section: &Section) -> Vec<&Card> {
        section
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Card(card)
                    if matches!(
                        card.open,
                        Some(Target::Browse {
                            kind: PageKind::Artist,
                            ..
                        })
                    ) =>
                {
                    Some(card)
                }
                _ => None,
            })
            .collect()
    }
    let named = page
        .sections
        .iter()
        .find(|s| s.title.eq_ignore_ascii_case("Fans might also like"));
    match named {
        Some(section) => artist_cards(section),
        None => page
            .sections
            .iter()
            .map(artist_cards)
            .find(|cards| !cards.is_empty())
            .unwrap_or_default(),
    }
}

/// "Artists to try": artists the favourite artists' pages call similar,
/// new to the listener, scored by the favourite's weight and how early its
/// page lists them; one several pages list gains half again for each
/// more, and half again for each favourite Deezer's listeners relate it
/// to. One row, mixed as the listener's genres are ([`eased`], [`split`]):
/// each artist counts for the genre of the favourite it is most like.
fn artists_to_try(
    listening: &Listening,
    found: &Found,
    known: &Known,
    tastes: Option<&Tastes>,
) -> (Vec<ArtistPick>, Vec<String>) {
    struct Candidate {
        pick: ArtistPick,
        score: f64,
        from: Vec<(usize, f64)>,
    }
    let seeds: HashSet<&str> = found.artists.iter().map(|(a, _)| a.id.as_str()).collect();
    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    for (index, (artist, page)) in found.artists.iter().enumerate() {
        for (place, card) in similar_artists(page).into_iter().enumerate() {
            let Some(Target::Browse { id, .. }) = &card.open else {
                continue;
            };
            if seeds.contains(id.as_str())
                || known.artist(Some(id.as_str()), &card.title)
                || listening.unwanted_artist(Some(id.as_str()), &card.title)
            {
                continue;
            }
            let candidate = candidates.entry(id.clone()).or_insert_with(|| Candidate {
                pick: ArtistPick {
                    id: id.clone(),
                    name: card.title.clone(),
                    subtitle: card.subtitle.clone(),
                    thumbnail: card.thumbnail.clone(),
                    genre: None,
                    shuffle: card.menu.as_ref().and_then(|m| m.shuffle.clone()),
                    mix: card.menu.as_ref().and_then(|m| m.mix.clone()),
                },
                score: 0.0,
                from: Vec::new(),
            });
            let gain = artist.weight / (1.0 + place as f64 / 5.0);
            candidate.score += gain;
            match candidate.from.iter_mut().find(|(i, _)| *i == index) {
                Some((_, part)) => *part += gain,
                None => candidate.from.push((index, gain)),
            }
        }
    }
    let mut ranked: Vec<Candidate> = candidates
        .into_values()
        .map(|mut c| {
            c.score *= 1.0 + 0.5 * (c.from.len() as f64 - 1.0);
            c.score *= 1.0 + 0.5 * f64::from(found.opinion.names(&c.pick.name));
            let most = c
                .from
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(i, _)| *i);
            c.pick.genre = most.and_then(|i| found.artists[i].0.genre.clone());
            c
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.pick.id.cmp(&b.pick.id))
    });
    let ranked = mixed(ranked, CARDS_KEPT, tastes, |c| c.pick.genre.as_deref());
    let mut credit = vec![0usize; found.artists.len()];
    for candidate in &ranked {
        for (from, _) in &candidate.from {
            credit[*from] += 1;
        }
    }
    let mut order: Vec<usize> = (0..credit.len()).filter(|&i| credit[i] > 0).collect();
    order.sort_by(|&a, &b| credit[b].cmp(&credit[a]).then(a.cmp(&b)));
    let like = order
        .into_iter()
        .take(2)
        .map(|i| found.artists[i].0.name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    (ranked.into_iter().map(|c| c.pick).collect(), like)
}

/// The first `count` of `ranked` (best first), mixed as the listener's
/// genres are: each genre its share of the places ([`eased`], [`split`]),
/// taken in turn, a genre at a time; places a genre cannot fill go to the
/// best of the rest. Without `tastes`, the first `count`.
fn mixed<T>(
    ranked: Vec<T>,
    count: usize,
    tastes: Option<&Tastes>,
    genre: impl Fn(&T) -> Option<&str>,
) -> Vec<T> {
    let targets = tastes
        .map(|t| split(&eased(&t.genres), count))
        .unwrap_or_default();
    if targets.is_empty() {
        return ranked.into_iter().take(count).collect();
    }
    let mut slots: Vec<Option<T>> = ranked.into_iter().map(Some).collect();
    // Each genre's best, as many as its places.
    let mut groups: Vec<Vec<T>> = targets
        .iter()
        .map(|(wanted, places)| {
            let mut group = Vec::new();
            for slot in slots.iter_mut() {
                if group.len() >= *places {
                    break;
                }
                if slot
                    .as_ref()
                    .is_some_and(|item| genre(item) == Some(wanted.as_str()))
                    && let Some(item) = slot.take()
                {
                    group.push(item);
                }
            }
            group.reverse();
            group
        })
        .collect();
    let mut picked = Vec::new();
    while groups.iter().any(|g| !g.is_empty()) {
        for group in groups.iter_mut() {
            if let Some(item) = group.pop() {
                picked.push(item);
            }
        }
    }
    let left = count.saturating_sub(picked.len());
    picked.extend(slots.into_iter().flatten().take(left));
    picked
}

/// "New from your artists": the favourite artists' albums, EPs and
/// singles from this year or last, that the listener has not played from,
/// newest first, then by how much the artist is played; one of each
/// artist ([`RELEASES_PER_ARTIST`]), and one of a title (an album over an
/// EP or a single of the same name).
fn new_releases(listening: &Listening, found: &Found, now: u64) -> Vec<ReleasePick> {
    let this_year = year_of(now);
    let played_albums: HashSet<&str> = listening
        .songs
        .values()
        .filter_map(|song| song.album_id.as_deref())
        .collect();
    let mut ranked: Vec<(f64, ReleasePick)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (artist, page) in &found.artists {
        let releases = page.sections.iter().filter(|section| {
            let title = section.title.to_lowercase();
            title.contains("single") || title.contains("album")
        });
        for section in releases {
            for item in &section.items {
                let Item::Card(card) = item else { continue };
                let Some(Target::Browse {
                    id,
                    kind: PageKind::Album,
                    ..
                }) = &card.open
                else {
                    continue;
                };
                let Some(year) = year_in(&card.subtitle) else {
                    continue;
                };
                if year + 1 < this_year
                    || played_albums.contains(id.as_str())
                    || listening.unwanted_releases.contains(id)
                    || !seen.insert(id.clone())
                {
                    continue;
                }
                let playlist_id = match &card.play {
                    Some(Target::Watch {
                        playlist_id: Some(list),
                        ..
                    }) => Some(list.clone()),
                    _ => None,
                };
                ranked.push((
                    artist.weight,
                    ReleasePick {
                        id: id.clone(),
                        title: card.title.clone(),
                        kind: release_kind(&card.subtitle).to_string(),
                        artist: artist.name.clone(),
                        year,
                        thumbnail: card.thumbnail.clone(),
                        playlist_id,
                    },
                ));
            }
        }
    }
    let kind_order = |kind: &str| match kind {
        "Album" => 0,
        "EP" => 1,
        _ => 2,
    };
    ranked.sort_by(|(wa, a), (wb, b)| {
        b.year
            .cmp(&a.year)
            .then(wb.total_cmp(wa))
            .then(kind_order(&a.kind).cmp(&kind_order(&b.kind)))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut titles: HashSet<String> = HashSet::new();
    ranked.retain(|(_, pick)| titles.insert(deezer::song_key(&pick.title, &pick.artist)));
    few_per_artist(ranked, CARDS_KEPT, RELEASES_PER_ARTIST, |(_, pick)| {
        artist_keys(None, &pick.artist)
    })
    .into_iter()
    .map(|(_, pick)| pick)
    .collect()
}

/// What a release card's subtitle calls it ("Single • 2026"): a single,
/// an EP, else an album.
fn release_kind(subtitle: &str) -> &'static str {
    let first = subtitle.split('\u{2022}').next().unwrap_or_default().trim();
    if first.eq_ignore_ascii_case("single") {
        "Single"
    } else if first.eq_ignore_ascii_case("ep") {
        "EP"
    } else {
        "Album"
    }
}

/// The year in a card's subtitle ("Single • 2026", "2025"), the last one
/// when there are several.
fn year_in(subtitle: &str) -> Option<i32> {
    subtitle
        .split(|c: char| !c.is_ascii_digit())
        .filter(|word| word.len() == 4)
        .filter_map(|word| word.parse::<i32>().ok())
        .rfind(|year| (1900..=2200).contains(year))
}

/// The year (by the calendar, UTC) of a moment in seconds since 1970.
pub fn year_of(seconds: u64) -> i32 {
    // Howard Hinnant's days-to-civil.
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let year = year_of_era + era * 400;
    (if month_index >= 10 { year + 1 } else { year }) as i32
}

/// "A", "A and B", "A, B and C".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Radar {
    /// Made less than a day before `now`, by this YTFast.
    pub fn fresh(&self, now: u64) -> bool {
        self.version == RADAR_VERSION && now >= self.made && now - self.made < FRESH_FOR
    }

    /// The page: its rows, leaving out what has since been marked
    /// unwanted (what was kept beyond a row's first ones comes forward).
    /// An empty row is left out. The station's songs are not shown: the
    /// page's button plays them.
    pub fn page(&self, listening: &Listening) -> Page {
        let mut sections = Vec::new();
        let shown: Vec<&ArtistPick> = self
            .artists
            .iter()
            .filter(|a| !listening.unwanted_artist(Some(a.id.as_str()), &a.name))
            .take(CARDS_SHOWN)
            .collect();
        if !shown.is_empty() {
            let mut genres: Vec<String> = Vec::new();
            for artist in &shown {
                if let Some(genre) = &artist.genre
                    && !genres.contains(genre)
                {
                    genres.push(genre.clone());
                }
            }
            genres.truncate(3);
            let strapline = if !genres.is_empty() {
                format!("In {}", listed(&genres))
            } else if !self.like.is_empty() {
                format!("Like {}", listed(&self.like))
            } else {
                String::new()
            };
            let cards = shown
                .into_iter()
                .map(|a| {
                    Item::Card(Card {
                        // Its Shuffle play and Start mix, kept from its card.
                        menu: (a.shuffle.is_some() || a.mix.is_some()).then(|| {
                            Box::new(crate::read::Menu {
                                shuffle: a.shuffle.clone(),
                                mix: a.mix.clone(),
                                ..crate::read::Menu::default()
                            })
                        }),
                        title: a.name.clone(),
                        subtitle: a.subtitle.clone(),
                        thumbnail: a.thumbnail.clone(),
                        round: true,
                        open: Some(Target::Browse {
                            id: a.id.clone(),
                            kind: PageKind::Artist,
                            params: None,
                        }),
                        play: None,
                        podcast: None,
                        look: CardLook::default(),
                    })
                })
                .collect();
            sections.push(row("Artists to try", strapline, cards));
        }
        let releases: Vec<Item> = self
            .releases
            .iter()
            .filter(|r| {
                !listening.unwanted_releases.contains(&r.id)
                    && !listening.unwanted_artist(None, &r.artist)
            })
            .take(CARDS_SHOWN)
            .map(|r| {
                Item::Card(Card {
                    menu: None,
                    title: r.title.clone(),
                    subtitle: format!("{} \u{2022} {}", r.kind, r.artist),
                    thumbnail: r.thumbnail.clone(),
                    round: false,
                    open: Some(Target::Browse {
                        id: r.id.clone(),
                        kind: PageKind::Album,
                        params: None,
                    }),
                    play: r.playlist_id.as_ref().map(|list| Target::Watch {
                        video_id: None,
                        playlist_id: Some(list.clone()),
                    }),
                    podcast: None,
                    look: CardLook::default(),
                })
            })
            .collect();
        if !releases.is_empty() {
            sections.push(row(
                "New from your artists",
                "From artists you play".to_string(),
                releases,
            ));
        }
        Page {
            header: None,
            sections,
            sort: None,
        }
    }

    /// The station's songs still new to the listener.
    pub fn songs_left(&self, listening: &Listening) -> usize {
        self.pool
            .iter()
            .filter(|p| !listening.knows(&p.track.video_id) && !listening.unwanted(&p.track))
            .count()
    }
}

/// A row of the page: songs as Home's Quick picks, cards as its shelves.
fn row(title: &str, strapline: String, items: Vec<Item>) -> Section {
    Section {
        title: title.to_string(),
        items,
        shape: Shape::Carousel,
        strapline,
        ..Section::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 20 October 2026, 12:00 UTC.
    const NOW: u64 = 1_792_497_600;
    const DAY: u64 = 86_400;

    fn song(id: &str, artist: &str) -> Track {
        Track {
            video_id: id.to_string(),
            title: id.to_uppercase(),
            artists: artist.to_string(),
            artist_id: Some(format!("UC{artist}")),
            album_id: Some(format!("MPREb_{id}")),
            kind: TrackKind::Song,
            playable: true,
            ..Track::default()
        }
    }

    fn finished(listening: &mut Listening, track: &Track, times: u32, when: u64) {
        for _ in 0..times {
            listening.played(track, 200.0, 200.0, Ending::Finished, when);
        }
    }

    fn artist_card(name: &str) -> Item {
        Item::Card(Card {
            menu: None,
            title: name.to_string(),
            subtitle: "1M monthly audience".into(),
            thumbnail: None,
            round: true,
            open: Some(Target::Browse {
                id: format!("UC{name}"),
                kind: PageKind::Artist,
                params: None,
            }),
            play: None,
            podcast: None,
            look: CardLook::default(),
        })
    }

    fn release_card(title: &str, subtitle: &str) -> Item {
        Item::Card(Card {
            menu: None,
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            thumbnail: None,
            round: false,
            open: Some(Target::Browse {
                id: format!("MPREb_{title}"),
                kind: PageKind::Album,
                params: None,
            }),
            play: Some(Target::Watch {
                video_id: None,
                playlist_id: Some(format!("OLAK5uy_{title}")),
            }),
            podcast: None,
            look: CardLook::default(),
        })
    }

    fn seed(id: &str, artist: &str, weight: f64) -> Seed {
        Seed {
            video_id: id.to_string(),
            title: id.to_uppercase(),
            keys: artist_keys(Some(format!("UC{artist}").as_str()), artist),
            weight,
            genre: None,
        }
    }

    /// A listener who finished songs of these genres this week, each of
    /// another artist (`{genre}{n}`).
    fn listener(genres: &[(&str, usize)]) -> Listening {
        let mut listening = Listening::default();
        for (genre, count) in genres {
            for n in 0..*count {
                let track = song(&format!("{genre}-song{n}"), &format!("{genre}{n}"));
                finished(&mut listening, &track, 1, NOW - DAY);
                listening.set_genre(&track.video_id, Some(genre.to_string()));
            }
        }
        listening
    }

    #[test]
    fn plays_are_noted_as_finished_skipped_or_neither() {
        let mut listening = Listening::default();
        let lemon = song("lemon", "Kenshi");
        listening.played(&lemon, 200.0, 210.0, Ending::Stopped, NOW);
        listening.played(&lemon, 5.0, 210.0, Ending::Skipped, NOW);
        listening.played(&lemon, 40.0, 210.0, Ending::Skipped, NOW);
        listening.played(&lemon, 120.0, 210.0, Ending::Stopped, NOW);
        // A click around: not a play.
        listening.played(&lemon, 1.0, 210.0, Ending::Stopped, NOW);
        // Played to its end, whatever was heard (a jump near the end).
        listening.played(&lemon, 10.0, 210.0, Ending::Finished, NOW);
        let heard = &listening.songs()["lemon"];
        assert_eq!(
            (heard.plays, heard.finished, heard.skipped),
            (5, 2, 1),
            "{heard:?}"
        );
        assert_eq!(heard.title, "LEMON");
        // A short song's early skip is its first half.
        let short = song("short", "Kenshi");
        listening.played(&short, 25.0, 40.0, Ending::Skipped, NOW);
        assert_eq!(listening.songs()["short"].skipped, 0);
    }

    #[test]
    fn the_record_keeps_the_songs_played_last() {
        let mut listening = Listening::default();
        for n in 0..(MOST_SONGS + 5) {
            let track = song(&format!("s{n}"), "A");
            listening.played(&track, 100.0, 100.0, Ending::Finished, n as u64);
        }
        assert_eq!(listening.songs().len(), MOST_SONGS);
        assert!(!listening.knows("s0") && !listening.knows("s4"));
        assert!(listening.knows("s5") && listening.knows(&format!("s{}", MOST_SONGS + 4)));
    }

    #[test]
    fn weight_counts_finishing_liking_and_skipping_and_fades() {
        let mut listening = Listening::default();
        let loved = song("loved", "A");
        let skipped = song("skipped", "B");
        finished(&mut listening, &loved, 3, NOW);
        listening.rated(&loved, Rating::Like, NOW);
        listening.played(&skipped, 3.0, 200.0, Ending::Skipped, NOW);
        let songs = listening.songs();
        assert_eq!(songs["loved"].weight(NOW), 5.0);
        assert_eq!(songs["skipped"].weight(NOW), 0.0);
        let faded = songs["loved"].weight(NOW + 45 * DAY);
        assert!((faded - 2.5).abs() < 1e-9, "{faded}");
    }

    #[test]
    fn seeds_are_favourites_one_per_artist_then_youtubes() {
        let mut listening = Listening::default();
        let a1 = song("a1", "A");
        let a2 = song("a2", "A");
        let b1 = song("b1", "B");
        finished(&mut listening, &a1, 5, NOW);
        finished(&mut listening, &a2, 4, NOW);
        finished(&mut listening, &b1, 2, NOW);
        let history = vec![
            song("h1", "A"),
            song("h2", "C"),
            song("h3", "D"),
            song("h4", "E"),
        ];
        let seeds = listening.seeds(&history, &[], NOW);
        let ids: HashSet<&str> = seeds.iter().map(|s| s.video_id.as_str()).collect();
        assert_eq!(seeds.len(), SEEDS);
        // One of A's (not both), B's, then YouTube's for other artists.
        assert_eq!(
            ids.contains("a1") as u8 + ids.contains("a2") as u8,
            1,
            "{ids:?}"
        );
        assert!(
            ids.contains("b1") && ids.contains("h2") && ids.contains("h3"),
            "{ids:?}"
        );
        assert!(!ids.contains("h1"), "A has a seed already");
        // The same songs all day.
        let later: Vec<String> = listening
            .seeds(&history, &[], NOW + 3600)
            .into_iter()
            .map(|s| s.video_id)
            .collect();
        let now: Vec<String> = seeds.into_iter().map(|s| s.video_id).collect();
        assert_eq!(later, now);
    }

    #[test]
    fn the_draw_changes_from_day_to_day_but_favours_the_best() {
        let mut listening = Listening::default();
        for n in 0..10 {
            let track = song(&format!("s{n}"), &format!("Artist{n}"));
            // The first far ahead of the rest.
            finished(&mut listening, &track, if n == 0 { 40 } else { 1 }, NOW);
        }
        let days: Vec<Vec<String>> = (0..20)
            .map(|day| {
                listening
                    .seeds(&[], &[], NOW + day * DAY)
                    .into_iter()
                    .map(|s| s.video_id)
                    .collect()
            })
            .collect();
        let distinct: HashSet<&Vec<String>> = days.iter().collect();
        assert!(distinct.len() > 5, "the seeds change: {days:?}");
        let with_best = days
            .iter()
            .filter(|d| d.contains(&"s0".to_string()))
            .count();
        assert!(
            with_best >= 18,
            "the favourite is nearly always drawn: {with_best}"
        );
    }

    #[test]
    fn the_pool_is_new_official_and_agreed_on() {
        let mut listening = Listening::default();
        let known = song("known", "Z");
        finished(&mut listening, &known, 1, NOW);
        let mut fan = song("fan", "F");
        fan.kind = TrackKind::Other("MUSIC_VIDEO_TYPE_UGC".into());
        let mut video = song("video", "V");
        video.kind = TrackKind::MusicVideo;
        // The song of a music video the listener played (on their phone).
        let seen = song("seenmv", "K");
        finished(&mut listening, &seen, 1, NOW);
        let mut its_song = song("seen", "K");
        its_song.more = Some(Box::new(crate::read::TrackMore {
            counterpart: Some("seenmv".into()),
            ..crate::read::TrackMore::default()
        }));
        // A known song's copy on another release: the same name and artist.
        let copy = Track {
            video_id: "knowncopy".into(),
            ..known.clone()
        };
        let mut liked = song("liked", "L");
        liked.like = Some(Rating::Like);
        let mut gone = song("gone", "G");
        gone.playable = false;
        let mut first = seed("s1", "S", 1.0);
        first.genre = Some("J-Pop".into());
        let found = Found {
            history: vec![song("history", "H")],
            radios: vec![
                (
                    first,
                    vec![
                        song("s1", "S"),
                        song("only1", "X"),
                        known.clone(),
                        song("history", "H"),
                        fan.clone(),
                        video.clone(),
                        its_song.clone(),
                        copy.clone(),
                        liked.clone(),
                        gone.clone(),
                        song("both", "Y"),
                    ],
                ),
                (
                    seed("s2", "T", 1.0),
                    vec![song("both", "Y"), song("only2", "W")],
                ),
            ],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let ids: Vec<&str> = radar
            .pool
            .iter()
            .map(|p| p.track.video_id.as_str())
            .collect();
        // Suggested by both radios: first, though late in the first.
        assert_eq!(ids[0], "both", "{ids:?}");
        assert_eq!(radar.pool[0].seeds, ["s1", "s2"]);
        // Songs only, and none the listener knows in another version.
        for left_out in [
            "s1",
            "known",
            "history",
            "fan",
            "video",
            "seen",
            "knowncopy",
            "liked",
            "gone",
        ] {
            assert!(!ids.contains(&left_out), "{left_out}: {ids:?}");
        }
        // Each is played for its seed's genre.
        let only1 = radar.pool.iter().find(|p| p.track.video_id == "only1");
        assert_eq!(only1.unwrap().genre.as_deref(), Some("J-Pop"));
        assert_eq!(radar.known, ["history"]);
        assert_eq!(radar.version, RADAR_VERSION);
        assert_eq!(radar.songs_left(&listening), ids.len());
    }

    #[test]
    fn deezers_listeners_agreeing_count() {
        let listening = Listening::default();
        let radio = vec![song("plain", "P"), song("artist", "Q"), song("both", "R")];
        let opinion = Opinion {
            artists: HashMap::from([(deezer::name_key("Q"), 1), (deezer::name_key("R"), 1)]),
            songs: HashSet::from([deezer::song_key("BOTH", "R")]),
            ids: BTreeMap::from([("s".to_string(), 7)]),
        };
        let found = Found {
            radios: vec![(seed("s", "S", 1.0), radio)],
            opinion,
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let ids: Vec<&str> = radar
            .pool
            .iter()
            .map(|p| p.track.video_id.as_str())
            .collect();
        // Later in YouTube's radio, but Deezer's listeners agree.
        assert_eq!(ids, ["both", "artist", "plain"]);
        assert_eq!(radar.deezer_ids.get("s"), Some(&7));
    }

    #[test]
    fn the_pool_keeps_a_few_of_each_artist() {
        let listening = Listening::default();
        let radio: Vec<Track> = (0..8).map(|n| song(&format!("x{n}"), "X")).collect();
        let found = Found {
            radios: vec![(seed("s", "S", 1.0), radio)],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        assert_eq!(radar.pool.len(), POOL_PER_ARTIST);
    }

    #[test]
    fn songs_left_leave_out_what_is_played_or_unwanted() {
        let mut listening = Listening::default();
        let radio: Vec<Track> = (0..4)
            .map(|n| song(&format!("x{n}"), &format!("X{}", n / 2)))
            .chain((0..3).map(|n| song(&format!("y{n}"), "Y")))
            .collect();
        let found = Found {
            radios: vec![(seed("s", "S", 1.0), radio)],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        assert_eq!(radar.songs_left(&listening), 7);
        listening.not_interested(&Unwanted::Song(song("x0", "X0")));
        assert_eq!(radar.songs_left(&listening), 6, "its artist stays");
        // Three of one artist's songs disliked or unwanted: the artist goes.
        for n in 0..ARTIST_AFTER {
            listening.rated(&song(&format!("old{n}"), "Y"), Rating::Dislike, NOW);
        }
        assert_eq!(radar.songs_left(&listening), 3);
        // A song played since is no longer new.
        listening.played(&song("x2", "X1"), 100.0, 100.0, Ending::Finished, NOW);
        assert_eq!(radar.songs_left(&listening), 2);
    }

    #[test]
    fn artists_to_try_are_new_and_agreed_on() {
        let mut listening = Listening::default();
        finished(&mut listening, &song("k", "Known"), 1, NOW);
        let page = |names: &[&str]| Page {
            sections: vec![Section {
                title: "Fans might also like".into(),
                items: names.iter().map(|n| artist_card(n)).collect(),
                shape: Shape::Carousel,
                ..Section::default()
            }],
            ..Page::default()
        };
        let artist = |name: &str| ArtistSeed {
            id: format!("UC{name}"),
            name: name.to_string(),
            weight: 1.0,
            genre: None,
        };
        let found = Found {
            artists: vec![
                (artist("A"), page(&["Known", "Solo", "Both", "B"])),
                (artist("B"), page(&["Both", "A", "Other"])),
            ],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let names: Vec<&str> = radar.artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names[0], "Both", "{names:?}");
        assert!(
            names.contains(&"Solo") && names.contains(&"Other"),
            "{names:?}"
        );
        for left_out in ["Known", "A", "B"] {
            assert!(!names.contains(&left_out), "{left_out}: {names:?}");
        }
        assert_eq!(radar.like, ["A", "B"]);
        listening.not_interested(&Unwanted::Artist {
            id: Some("UCBoth".into()),
            name: "Both".into(),
        });
        let shown = radar.page(&listening);
        let cards = shown
            .sections
            .iter()
            .find(|s| s.title == "Artists to try")
            .unwrap();
        assert!(
            cards
                .items
                .iter()
                .all(|i| !matches!(i, Item::Card(c) if c.title == "Both"))
        );
    }

    #[test]
    fn new_releases_are_recent_and_not_yet_played() {
        let mut listening = Listening::default();
        let mut heard = song("heard", "A");
        heard.album_id = Some("MPREb_Played".into());
        finished(&mut listening, &heard, 1, NOW);
        let found = Found {
            artists: vec![(
                ArtistSeed {
                    id: "UCA".into(),
                    name: "A".into(),
                    weight: 1.0,
                    genre: None,
                },
                Page {
                    sections: vec![
                        Section {
                            title: "Albums".into(),
                            items: vec![
                                release_card("Fresh", "2026"),
                                release_card("Played", "2026"),
                                release_card("Old", "2019"),
                            ],
                            ..Section::default()
                        },
                        Section {
                            title: "Singles & EPs".into(),
                            items: vec![
                                release_card("Last year", "Single \u{2022} 2025"),
                                release_card("Short", "EP \u{2022} 2026"),
                            ],
                            ..Section::default()
                        },
                    ],
                    ..Page::default()
                },
            )],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let titles: Vec<&str> = radar.releases.iter().map(|r| r.title.as_str()).collect();
        // The newest; one of each artist.
        assert_eq!(titles, ["Fresh"]);
        assert_eq!(
            radar.releases[0].playlist_id.as_deref(),
            Some("OLAK5uy_Fresh")
        );
        let page = radar.page(&listening);
        let row = page
            .sections
            .iter()
            .find(|s| s.title == "New from your artists")
            .unwrap();
        assert!(
            matches!(&row.items[0], Item::Card(c) if c.subtitle == "Album \u{2022} A"),
            "{row:?}"
        );
    }

    #[test]
    fn one_release_of_a_title() {
        let listening = Listening::default();
        let releases = |name: &str, cards: Vec<Item>| {
            (
                ArtistSeed {
                    id: format!("UC{name}"),
                    name: name.to_string(),
                    weight: 1.0,
                    genre: None,
                },
                Page {
                    sections: vec![Section {
                        title: "Singles & EPs".into(),
                        items: cards,
                        ..Section::default()
                    }],
                    ..Page::default()
                },
            )
        };
        let mut single = release_card("Same Song", "Single \u{2022} 2026");
        if let Item::Card(card) = &mut single {
            card.open = Some(Target::Browse {
                id: "MPREb_single".into(),
                kind: PageKind::Album,
                params: None,
            });
        }
        let found = Found {
            artists: vec![
                releases(
                    "A",
                    vec![
                        single,
                        release_card("Same Song (Deluxe)", "EP \u{2022} 2026"),
                    ],
                ),
                releases("B", vec![release_card("Other", "Single \u{2022} 2026")]),
            ],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let shown: Vec<(&str, &str)> = radar
            .releases
            .iter()
            .map(|r| (r.title.as_str(), r.kind.as_str()))
            .collect();
        // The EP of the same name stands for its single.
        assert_eq!(shown, [("Same Song (Deluxe)", "EP"), ("Other", "Single")]);
    }

    /// An artist to try keeps its card's Shuffle play and Start mix lists,
    /// and Radar's page offers them again.
    #[test]
    fn artists_to_try_keep_their_lists() {
        let listening = listener(&[("J-Pop", 2)]);
        let mut card = match artist_card("New") {
            Item::Card(card) => card,
            Item::Track(_) => unreachable!(),
        };
        card.menu = Some(Box::new(crate::read::Menu {
            shuffle: Some("RDAONew".into()),
            mix: Some("RDEMNew".into()),
            ..crate::read::Menu::default()
        }));
        let favourite = Page {
            sections: vec![Section {
                title: "Fans might also like".into(),
                items: vec![Item::Card(card)],
                shape: Shape::Carousel,
                ..Section::default()
            }],
            ..Page::default()
        };
        let seed = ArtistSeed {
            id: "UCJ-Pop0".into(),
            name: "J-Pop0".into(),
            weight: 1.0,
            genre: Some("J-Pop".into()),
        };
        let found = Found {
            artists: vec![(seed, favourite)],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let new = radar.artists.iter().find(|a| a.id == "UCNew").unwrap();
        assert_eq!(new.shuffle.as_deref(), Some("RDAONew"));
        assert_eq!(new.mix.as_deref(), Some("RDEMNew"));
        let shown = radar.page(&listening);
        let menu = shown
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find_map(|item| match item {
                Item::Card(card) if card.title == "New" => card.menu.as_deref(),
                _ => None,
            })
            .unwrap();
        assert_eq!(menu.shuffle.as_deref(), Some("RDAONew"));
        assert_eq!(menu.mix.as_deref(), Some("RDEMNew"));
    }

    #[test]
    fn the_page_says_why_and_leaves_out_empty_rows() {
        let pick = |name: &str, genre: Option<&str>| ArtistPick {
            id: format!("UC{name}"),
            name: name.to_string(),
            genre: genre.map(str::to_string),
            ..ArtistPick::default()
        };
        let radar = Radar {
            made: NOW,
            version: RADAR_VERSION,
            artists: vec![pick("N", None)],
            like: vec!["Lemon".into(), "Idol".into()],
            ..Radar::default()
        };
        let page = radar.page(&Listening::default());
        assert_eq!(page.sections.len(), 1);
        assert_eq!(page.sections[0].title, "Artists to try");
        assert_eq!(page.sections[0].strapline, "Like Lemon and Idol");
        assert_eq!(page.sections[0].shape, Shape::Carousel);
        let genres = Radar {
            artists: vec![
                pick("A", Some("J-Pop")),
                pick("B", Some("Rock")),
                pick("C", Some("J-Pop")),
            ],
            ..radar.clone()
        };
        assert_eq!(
            genres.page(&Listening::default()).sections[0].strapline,
            "In J-Pop and Rock"
        );
        assert!(radar.fresh(NOW + FRESH_FOR - 1));
        assert!(!radar.fresh(NOW + FRESH_FOR));
        // One made by an older YTFast is made again.
        let older = Radar {
            version: 0,
            ..radar.clone()
        };
        assert!(!older.fresh(NOW));
        assert!(
            Radar::default()
                .page(&Listening::default())
                .sections
                .is_empty()
        );
    }

    #[test]
    fn plays_are_kept_with_their_dates_for_a_while() {
        let mut listening = Listening::default();
        let a = song("a", "A");
        assert_eq!(
            listening.played(&a, 30.0, 200.0, Ending::Stopped, NOW - 40 * DAY),
            Some(Outcome::Played)
        );
        assert_eq!(
            listening.played(&a, 3.0, 200.0, Ending::Skipped, NOW - DAY),
            Some(Outcome::Skipped)
        );
        assert_eq!(
            listening.played(&a, 200.0, 200.0, Ending::Finished, NOW),
            Some(Outcome::Finished)
        );
        assert_eq!(listening.played(&a, 1.0, 200.0, Ending::Stopped, NOW), None);
        // The play 40 days old is let go of; the song's counts stay.
        let log: Vec<(u64, Ending, u32)> = listening
            .log()
            .iter()
            .map(|p| (p.at, p.ending, p.heard))
            .collect();
        assert_eq!(
            log,
            [
                (NOW - DAY, Ending::Skipped, 3),
                (NOW, Ending::Finished, 200)
            ]
        );
        assert_eq!(listening.songs()["a"].plays, 3);
    }

    #[test]
    fn genres_are_noted_and_asked_for() {
        let mut listening = Listening::default();
        let loved = song("loved", "A");
        let other = song("other", "A");
        let fresh = song("fresh", "B");
        finished(&mut listening, &loved, 3, NOW);
        finished(&mut listening, &other, 1, NOW);
        finished(&mut listening, &fresh, 2, NOW);
        assert!(listening.wants_genre("loved"));
        let missing: Vec<String> = listening
            .missing_genres(2, NOW)
            .into_iter()
            .map(|t| t.video_id)
            .collect();
        assert_eq!(missing, ["loved", "fresh"], "the best liked first");
        assert!(listening.set_genre("loved", Some("J-Pop".into())));
        assert!(!listening.set_genre("loved", Some("J-Pop".into())));
        // None known: asked no more, and of no genre.
        assert!(listening.set_genre("fresh", None));
        assert!(!listening.wants_genre("loved") && !listening.wants_genre("fresh"));
        assert!(!listening.set_genre("unknown", Some("Rock".into())));
        // A song of no genre takes its artist's.
        assert_eq!(listening.genre_for(&other).as_deref(), Some("J-Pop"));
        assert_eq!(listening.genre_for(&fresh), None);
    }

    #[test]
    fn tastes_say_this_week_else_this_month_else_lately() {
        let mut listening = listener(&[("J-Pop", 6), ("Rock", 2)]);
        let tastes = listening.tastes(NOW).unwrap();
        assert_eq!(tastes.span, Span::Week);
        assert_eq!(tastes.genres[0], ("J-Pop".to_string(), 0.75));
        assert_eq!(tastes.line(), "This week you played mostly J-Pop and Rock");
        // Ten days on, the plays are this month's.
        assert_eq!(listening.tastes(NOW + 10 * DAY).unwrap().span, Span::Month);
        // Forty days on, the record's songs by how much they are liked.
        assert_eq!(listening.tastes(NOW + 40 * DAY).unwrap().span, Span::Lately);
        // Early skips do not count.
        for n in 0..10 {
            let track = song(&format!("skip{n}"), &format!("Skip{n}"));
            listening.played(&track, 2.0, 200.0, Ending::Skipped, NOW);
            listening.set_genre(&track.video_id, Some("Metal".into()));
        }
        assert_eq!(listening.tastes(NOW).unwrap().genres.len(), 2);
        // Too little known: nothing said.
        assert_eq!(listener(&[("J-Pop", 3)]).tastes(NOW), None);
        let one = Tastes {
            span: Span::Lately,
            genres: vec![("J-Pop".into(), 0.95), ("Rock".into(), 0.05)],
        };
        assert_eq!(one.line(), "Lately you play mostly J-Pop");
    }

    #[test]
    fn shares_are_eased_and_split() {
        let genres = vec![
            ("J-Pop".to_string(), 0.62),
            ("Rock".to_string(), 0.21),
            ("Hip-Hop".to_string(), 0.09),
            ("Jazz".to_string(), 0.04),
            ("Metal".to_string(), 0.04),
        ];
        let mix = eased(&genres);
        let shares: Vec<i64> = mix
            .iter()
            .map(|(_, s)| (s * 100.0).round() as i64)
            .collect();
        assert_eq!(shares, [51, 30, 19], "{mix:?}");
        assert_eq!(
            split(&mix, 12),
            [
                ("J-Pop".to_string(), 6),
                ("Rock".to_string(), 4),
                ("Hip-Hop".to_string(), 2)
            ]
        );
        // Few places: one each while they last.
        assert_eq!(
            split(&mix, 2),
            [
                ("J-Pop".to_string(), 1),
                ("Rock".to_string(), 1),
                ("Hip-Hop".to_string(), 0)
            ]
        );
        assert!(eased(&[]).is_empty());
        assert!(split(&[], 4).is_empty());
    }

    #[test]
    fn seeds_and_artists_follow_the_genres() {
        // Mostly J-Pop, some Rock (eased: 61% and 39%): of four seeds, two
        // of each, the larger remainder rounded up.
        let listening = listener(&[("J-Pop", 10), ("Rock", 4)]);
        let seeds = listening.seeds(&[], &[], NOW);
        let genre_count = |genre: &str| {
            seeds
                .iter()
                .filter(|s| s.genre.as_deref() == Some(genre))
                .count()
        };
        assert_eq!((genre_count("J-Pop"), genre_count("Rock")), (2, 2));
        let artists = listening.top_artists(&[], &[], NOW);
        let rock = artists
            .iter()
            .filter(|a| a.genre.as_deref() == Some("Rock"))
            .count();
        // Of three artists, two J-Pop and one Rock.
        assert_eq!((artists.len(), rock), (ARTISTS, 1));
    }

    #[test]
    fn artists_to_try_are_mixed_by_genre() {
        let listening = listener(&[("J-Pop", 8), ("Rock", 2)]);
        let page = |names: Vec<String>| Page {
            sections: vec![Section {
                title: "Fans might also like".into(),
                items: names.iter().map(|n| artist_card(n)).collect(),
                shape: Shape::Carousel,
                ..Section::default()
            }],
            ..Page::default()
        };
        let artist = |name: &str, weight: f64, genre: &str| ArtistSeed {
            id: format!("UC{name}"),
            name: name.to_string(),
            weight,
            genre: Some(genre.to_string()),
        };
        // The J-Pop favourite weighs far more: alone, it would fill the row.
        let found = Found {
            artists: vec![
                (
                    artist("Jfav", 10.0, "J-Pop"),
                    page((0..20).map(|n| format!("NewJ{n}")).collect()),
                ),
                (
                    artist("Rfav", 1.0, "Rock"),
                    page((0..20).map(|n| format!("NewR{n}")).collect()),
                ),
            ],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        let genres: Vec<&str> = radar
            .artists
            .iter()
            .map(|a| a.genre.as_deref().unwrap_or("?"))
            .collect();
        assert_eq!(radar.artists.len(), CARDS_KEPT);
        let rock = genres.iter().filter(|g| **g == "Rock").count();
        // J-Pop 80%, Rock 20%: eased, 67% and 33%, of 16 places.
        assert_eq!(rock, 5, "{genres:?}");
        // Taken in turn, so the row is mixed from its start.
        assert_eq!(genres[..2], ["J-Pop", "Rock"]);
    }

    #[test]
    fn years_are_read_from_subtitles_and_clocks() {
        assert_eq!(year_in("Single \u{2022} 2026"), Some(2026));
        assert_eq!(year_in("2025"), Some(2025));
        assert_eq!(year_in("Album \u{2022} 12 songs"), None);
        assert_eq!(year_of(NOW), 2026);
        assert_eq!(year_of(0), 1970);
        // 31 December 2024, 23:59:59 and the next second.
        assert_eq!(year_of(1_735_689_599), 2024);
        assert_eq!(year_of(1_735_689_600), 2025);
        assert_eq!(listed(&["A".into(), "B".into(), "C".into()]), "A, B and C");
    }

    #[test]
    fn the_record_keeps_through_saving() {
        let mut listening = Listening::default();
        finished(&mut listening, &song("a", "A"), 2, NOW);
        listening.set_genre("a", Some("J-Pop".into()));
        listening.not_interested(&Unwanted::Release("MPREb_x".into()));
        let saved = serde_json::to_string(&listening).unwrap();
        let read: Listening = serde_json::from_str(&saved).unwrap();
        assert_eq!(read, listening);
        // An older or damaged file starts empty rather than failing.
        let empty: Listening = serde_json::from_str("{}").unwrap();
        assert!(empty.is_empty());
        listening.forget();
        assert!(listening.is_empty());
    }

    /// History's songs become plays on other devices, each once: reading
    /// the same History again adds nothing, nor does a song YTFast played
    /// then (History lists those too), nor History a day on, its groups
    /// moved on.
    #[test]
    fn the_account_adds_plays_from_other_devices_once() {
        let mut listening = Listening::default();
        let here = song("here", "A");
        let phone = song("phone", "B");
        let older = song("older", "C");
        finished(&mut listening, &here, 1, NOW - 3 * 3600);
        let account = AccountPlays {
            history: vec![
                (here.clone(), Ago::Today),
                (phone.clone(), Ago::Today),
                (older.clone(), Ago::ThisWeek),
            ],
            ..AccountPlays::default()
        };
        listening.take_account(&account, NOW);
        let songs = listening.songs();
        assert_eq!((songs["here"].plays, songs["here"].elsewhere), (1, 0));
        assert_eq!((songs["phone"].plays, songs["phone"].elsewhere), (1, 1));
        assert_eq!(songs["phone"].last, NOW - 2 * 3600);
        assert_eq!(songs["phone"].title, "PHONE");
        assert_eq!(songs["older"].last, NOW - 4 * DAY);
        // The record's plays stay oldest first.
        let log: Vec<(u64, bool)> = listening
            .log()
            .iter()
            .map(|p| (p.at, p.elsewhere))
            .collect();
        assert_eq!(
            log,
            [
                (NOW - 4 * DAY, true),
                (NOW - 3 * 3600, false),
                (NOW - 2 * 3600, true)
            ]
        );
        let before = listening.clone();
        listening.take_account(&account, NOW + 3600);
        let next_day = AccountPlays {
            history: vec![
                (here.clone(), Ago::Yesterday),
                (phone.clone(), Ago::Yesterday),
                (older.clone(), Ago::ThisWeek),
            ],
            ..AccountPlays::default()
        };
        listening.take_account(&next_day, NOW + DAY);
        assert_eq!(listening.log(), before.log());
        assert_eq!(listening.songs(), before.songs());
        // Played again two days on: one play more.
        let again = AccountPlays {
            history: vec![(phone.clone(), Ago::Today)],
            ..AccountPlays::default()
        };
        listening.take_account(&again, NOW + 2 * DAY);
        let phone = &listening.songs()["phone"];
        assert_eq!((phone.plays, phone.elsewhere), (2, 2));
    }

    /// Likes and dislikes the account holds are noted. A like is dated
    /// (for the listener's numbers) only when new at the top of Liked
    /// Music: its first reading holds old likes, and History cannot say
    /// when a song played again was liked. A dislike History shows is.
    #[test]
    fn the_accounts_likes_are_dated_only_when_new() {
        let mut listening = Listening::default();
        let rated = |id: &str, artist: &str, like: Rating| Track {
            like: Some(like),
            ..song(id, artist)
        };
        let first = AccountPlays {
            history: vec![
                (rated("old", "A", Rating::Like), Ago::Today),
                (rated("bad", "B", Rating::Dislike), Ago::Today),
            ],
            liked: vec![song("a", "C"), song("b", "D")],
            often: Vec::new(),
        };
        listening.take_account(&first, NOW);
        let rating = |l: &Listening, id: &str| l.songs()[id].rating;
        for id in ["old", "a", "b"] {
            assert_eq!(rating(&listening, id), Some(Rating::Like), "{id}");
        }
        assert_eq!(rating(&listening, "bad"), Some(Rating::Dislike));
        assert!(listening.unwanted(&song("bad", "B")));
        let dated = |l: &Listening| -> Vec<(String, Rating)> {
            l.ratings()
                .iter()
                .map(|r| (r.id.clone(), r.rating))
                .collect()
        };
        assert_eq!(dated(&listening), [("bad".to_string(), Rating::Dislike)]);
        // The next day: a song newly liked (on the phone, and played
        // there), and an old like played again.
        let next = AccountPlays {
            history: vec![
                (rated("new", "F", Rating::Like), Ago::Today),
                (rated("again", "E", Rating::Like), Ago::Today),
            ],
            liked: vec![song("new", "F"), song("a", "C"), song("b", "D")],
            often: Vec::new(),
        };
        listening.take_account(&next, NOW + DAY);
        listening.take_account(&next, NOW + DAY + 3600);
        assert_eq!(
            dated(&listening),
            [
                ("bad".to_string(), Rating::Dislike),
                ("new".to_string(), Rating::Like)
            ]
        );
        assert_eq!(rating(&listening, "again"), Some(Rating::Like));
    }

    /// When nothing could be read, nothing is noted, and Liked Music's
    /// first reading (of old likes) is still to come.
    #[test]
    fn an_account_not_read_changes_nothing() {
        let mut listening = Listening::default();
        listening.take_account(&AccountPlays::default(), NOW);
        assert_eq!(listening, Listening::default());
        let liked = AccountPlays {
            liked: vec![song("a", "A")],
            ..AccountPlays::default()
        };
        listening.take_account(&liked, NOW + DAY);
        assert!(listening.ratings().is_empty());
        assert_eq!(listening.songs()["a"].rating, Some(Rating::Like));
    }

    /// A play on another device counts less than one heard to its end in
    /// YTFast (History says it was played, not how much), and a song of
    /// "Listen again" as one played often, fading as songs do.
    #[test]
    fn plays_elsewhere_and_listen_again_count() {
        let mut listening = Listening::default();
        finished(&mut listening, &song("here", "A"), 1, NOW);
        let account = AccountPlays {
            history: vec![(song("phone", "B"), Ago::Today)],
            often: vec![song("often", "C")],
            ..AccountPlays::default()
        };
        listening.take_account(&account, NOW);
        let weight = |id: &str, at: u64| listening.songs()[id].weight(at);
        assert!((weight("here", NOW) - 1.0).abs() < 1e-9);
        assert!(weight("phone", NOW) > 0.6 && weight("phone", NOW) < 0.7);
        assert!((weight("often", NOW) - OFTEN).abs() < 1e-9);
        assert!((weight("often", NOW + 45 * DAY) - OFTEN / 2.0).abs() < 1e-9);
        // Known, so never suggested as new.
        assert!(listening.knows("phone") && listening.knows("often"));
    }

    /// History's songs by its groups (only songs; older groups left out),
    /// and Home's "Listen again" (only songs).
    #[test]
    fn the_account_is_read_from_history_and_home() {
        let reply =
            serde_json::from_str(include_str!("../tests/fixtures/history_synthetic.json")).unwrap();
        let history = AccountPlays::history_of(&crate::read::page(&reply));
        let read: Vec<(&str, Ago)> = history
            .iter()
            .map(|(t, ago)| (t.video_id.as_str(), *ago))
            .collect();
        assert_eq!(
            read,
            [
                ("histSong001", Ago::Today),
                ("histSong002", Ago::Today),
                ("histSong003", Ago::Yesterday)
            ]
        );
        assert_eq!(Ago::of("This week"), Some(Ago::ThisWeek));
        assert_eq!(Ago::of("Last week"), Some(Ago::LastWeek));
        assert_eq!(Ago::of("March 2026"), None);
        // A podcast's episode is no song.
        let episode = Track {
            kind: TrackKind::Other("MUSIC_VIDEO_TYPE_PODCAST_EPISODE".into()),
            ..song("episode", "Show")
        };
        let today = Page {
            sections: vec![Section {
                title: "Today".into(),
                items: vec![Item::Track(episode), Item::Track(song("tune", "A"))],
                ..Section::default()
            }],
            ..Page::default()
        };
        let ids: Vec<String> = AccountPlays::history_of(&today)
            .into_iter()
            .map(|(t, _)| t.video_id)
            .collect();
        assert_eq!(ids, ["tune"]);
        // Home's "Listen again" there holds an album and an artist.
        let home =
            serde_json::from_str(include_str!("../tests/fixtures/home_synthetic.json")).unwrap();
        assert!(AccountPlays::often_of(&crate::read::page(&home)).is_empty());
        let song_tile = Item::Card(Card {
            menu: None,
            title: "Lemon".into(),
            subtitle: "Song \u{2022} Kenshi Yonezu".into(),
            thumbnail: None,
            round: false,
            open: Some(Target::Watch {
                video_id: Some("lemon".into()),
                playlist_id: Some("RDAMVMlemon".into()),
            }),
            play: None,
            podcast: None,
            look: CardLook::default(),
        });
        let episode_tile = Item::Card(Card {
            menu: None,
            title: "Episode 12".into(),
            subtitle: "2d ago \u{2022} 15 min".into(),
            thumbnail: None,
            round: false,
            open: Some(Target::Browse {
                id: "MPEDepisode".into(),
                kind: PageKind::Episode,
                params: None,
            }),
            play: Some(Target::Watch {
                video_id: Some("episode".into()),
                playlist_id: None,
            }),
            podcast: Some("A Show".into()),
            look: CardLook::default(),
        });
        let shelf = |title: &str| Section {
            title: title.into(),
            items: vec![
                song_tile.clone(),
                artist_card("Kenshi"),
                episode_tile.clone(),
            ],
            ..Section::default()
        };
        let home = Page {
            sections: vec![shelf("Quick picks"), shelf("Listen again")],
            ..Page::default()
        };
        let often: Vec<String> = AccountPlays::often_of(&home)
            .into_iter()
            .map(|t| t.video_id)
            .collect();
        assert_eq!(often, ["lemon"]);
    }

    /// Genres still to ask for: one song of each artist, and none of an
    /// artist whose genre is known (their songs take it).
    #[test]
    fn missing_genres_are_one_per_artist() {
        let mut listening = Listening::default();
        finished(&mut listening, &song("a1", "A"), 3, NOW);
        finished(&mut listening, &song("a2", "A"), 2, NOW);
        finished(&mut listening, &song("b1", "B"), 1, NOW);
        finished(&mut listening, &song("c1", "C"), 2, NOW);
        finished(&mut listening, &song("c2", "C"), 1, NOW);
        listening.set_genre("c1", Some("Rock".into()));
        let missing: Vec<String> = listening
            .missing_genres(5, NOW)
            .into_iter()
            .map(|t| t.video_id)
            .collect();
        assert_eq!(missing, ["a1", "b1"]);
    }
}
