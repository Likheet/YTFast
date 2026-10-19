//! Radar: YTFast's own recommendations, the left menu's Radar page.
//!
//! YouTube finds the songs: a song's radio (what its Up next plays), the
//! artists an artist's page calls similar ("Fans might also like"), and an
//! artist's new albums and singles. YTFast chooses among them by what the
//! listener does, noted on the computer only ([`Listening`]): a song
//! finished, played again or liked counts for it and its artist; one
//! skipped in its first seconds, or disliked, counts against. Only songs
//! and artists new to the listener are suggested, at most two of one
//! artist in a row of the page, never anything marked "Not interested";
//! and a song that the radios of several favourites suggest ranks above
//! one that only one suggests.
//!
//! [`gather`] asks YouTube (at most ten requests, one at a time, once a
//! day: as a listener opening a few radios and artists); [`rank`] chooses,
//! on the computer; [`Radar::page`] lays the result out as rows, as Home's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::innertube::{ApiError, Session};
use crate::read::{
    Card, CardLook, Item, Page, PageKind, Rating, Section, Shape, Target, Thumb, Track, TrackKind,
};

/// Songs whose radios are asked for.
const SEEDS: usize = 4;
/// Artists whose pages are asked for.
const ARTISTS: usize = 4;
/// Seeds and artists are drawn from this many of the best, so that Radar
/// changes from day to day.
const DRAWN_FROM: usize = 12;
/// Below this many songs finished, YouTube's Liked Music helps say what
/// the listener likes.
const THIN: usize = 8;
/// The most songs the record keeps: the least recently played go first.
const MOST_SONGS: usize = 3000;
/// A play shorter than this (in seconds), neither finished nor skipped,
/// counts for nothing: a click around.
const BRIEF: f64 = 2.0;
/// Skipped in its first 30 seconds (or first half, for a short song), a
/// song counts against itself.
const EARLY: f64 = 30.0;
/// Heard to this part of its length, a song counts as finished.
const MOSTLY: f64 = 0.8;
/// How fast a play's weight fades: by half in 45 days.
const HALF_LIFE_DAYS: f64 = 45.0;
/// Songs, and cards, kept for each row of the page.
const SONGS_KEPT: usize = 32;
const CARDS_KEPT: usize = 16;
/// Songs, and cards, shown in a row (more come forward as some are played
/// or marked Not interested).
const SONGS_SHOWN: usize = 20;
const CARDS_SHOWN: usize = 12;
/// At most this many of one artist in a row.
const PER_ARTIST: usize = 2;
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}

impl Heard {
    /// How much the listener seems to like the song now: finished plays,
    /// other plays a little, a like a lot, early skips against; fading by
    /// half in [`HALF_LIFE_DAYS`]. Nothing for a song disliked.
    fn weight(&self, now: u64) -> f64 {
        if self.rating == Some(Rating::Dislike) {
            return 0.0;
        }
        let other = self.plays.saturating_sub(self.finished + self.skipped);
        let liked = if self.rating == Some(Rating::Like) {
            2.0
        } else {
            0.0
        };
        let base = f64::from(self.finished) + 0.3 * f64::from(other) + liked
            - 1.5 * f64::from(self.skipped);
        let days = now.saturating_sub(self.last) as f64 / 86_400.0;
        base.max(0.0) * 0.5f64.powf(days / HALF_LIFE_DAYS)
    }
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
}

/// The keys an artist is known by: its page (`id:UC...`), and its name
/// (`name:...`, the first artist's, in small letters), as some rows carry
/// only one of them.
fn artist_keys(id: Option<&str>, artists: &str) -> Vec<String> {
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

fn track_keys(track: &Track) -> Vec<String> {
    artist_keys(track.artist_id.as_deref(), &track.artists)
}

impl Listening {
    /// A song stopped playing after `heard` seconds of its `length` (0 when
    /// not known), at `now` (seconds since 1970).
    pub fn played(&mut self, track: &Track, heard: f64, length: f64, ending: Ending, now: u64) {
        if track.video_id.is_empty() {
            return;
        }
        let early = if length > 0.0 {
            EARLY.min(length / 2.0)
        } else {
            EARLY
        };
        let skipped = ending == Ending::Skipped && heard < early;
        let finished = ending == Ending::Finished || (length > 0.0 && heard >= length * MOSTLY);
        if !skipped && !finished && heard < BRIEF {
            return;
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
        if finished {
            song.finished += 1;
        } else if skipped {
            song.skipped += 1;
        }
        song.last = now;
        self.trim();
    }

    /// Keeps at most [`MOST_SONGS`], letting go of those played longest
    /// ago.
    fn trim(&mut self) {
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

    /// The account's rating of a song changed in YTFast. Disliked, it is
    /// never suggested (and, with others of its artist, nor is the
    /// artist); liked or neither, it may be again.
    pub fn rated(&mut self, track: &Track, rating: Rating) {
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
    }

    /// The songs noted, by ID.
    pub fn songs(&self) -> &BTreeMap<String, Heard> {
        &self.songs
    }

    /// The listener has played this song in YTFast.
    pub fn knows(&self, video_id: &str) -> bool {
        self.songs.contains_key(video_id)
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

    /// Too few songs finished to say much yet: YouTube's Liked Music helps.
    fn is_thin(&self) -> bool {
        self.songs.values().filter(|s| s.finished > 0).count() < THIN
    }

    /// The songs whose radios are asked for, drawn from the listener's
    /// favourites (the day's draw: Radar changes from day to day), one
    /// per artist; filled from `history` and `liked` (YouTube's) when
    /// the record has too few.
    pub fn seeds(&self, history: &[Track], liked: &[Track], now: u64) -> Vec<Seed> {
        let mut best: Vec<Seed> = self
            .songs
            .iter()
            .filter(|(id, _)| !self.unwanted_songs.contains_key(*id))
            .map(|(id, song)| Seed {
                video_id: id.clone(),
                title: song.title.clone(),
                keys: artist_keys(song.artist_id.as_deref(), &song.artists),
                weight: song.weight(now),
            })
            .filter(|seed| seed.weight > 0.0)
            .collect();
        best.sort_by(|a, b| {
            b.weight
                .total_cmp(&a.weight)
                .then(a.video_id.cmp(&b.video_id))
        });
        let mut drawn = draw(one_per_artist(best), SEEDS, now, |s| s.weight);
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
            });
            extra *= 0.9;
        }
        drawn
    }

    /// The artists whose pages are asked for: those the listener plays
    /// most (the day's draw, as [`Listening::seeds`]), filled from
    /// YouTube's `history` and `liked`. Only artists with a page.
    pub fn top_artists(&self, history: &[Track], liked: &[Track], now: u64) -> Vec<ArtistSeed> {
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
                });
            artist.weight += weight;
        }
        let mut best: Vec<ArtistSeed> =
            by_artist.into_values().filter(|a| a.weight > 0.0).collect();
        best.sort_by(|a, b| b.weight.total_cmp(&a.weight).then(a.id.cmp(&b.id)));
        let mut drawn = draw(best, ARTISTS, now.wrapping_add(1), |a| a.weight);
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

/// `count` of the first [`DRAWN_FROM`] of `best`, each as likely as its
/// weight (Efraimidis and Spirakis' draw), in the order drawn; the same
/// draw all day, another the next.
fn draw<T>(best: Vec<T>, count: usize, now: u64, weight: impl Fn(&T) -> f64) -> Vec<T> {
    let mut random = Random::new(now / 86_400);
    let mut keyed: Vec<(f64, T)> = best
        .into_iter()
        .take(DRAWN_FROM)
        .map(|item| {
            let w = weight(&item).max(1e-9);
            (random.unit().powf(1.0 / w), item)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    keyed
        .into_iter()
        .take(count)
        .map(|(_, item)| item)
        .collect()
}

/// A small random number maker (xorshift), so that a day's draw is always
/// the same.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// A number in (0, 1).
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
}

/// A song whose radio Radar asks for, and how much the listener likes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Seed {
    pub video_id: String,
    pub title: String,
    /// Its artist's keys ([`artist_keys`]).
    keys: Vec<String>,
    pub weight: f64,
}

/// An artist whose page Radar asks for, and how much the listener plays
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtistSeed {
    pub id: String,
    pub name: String,
    pub weight: f64,
}

/// What YouTube said, for [`rank`].
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// The account's History, newest first, and (while the record is thin)
    /// its Liked Music's first page.
    pub history: Vec<Track>,
    pub liked: Vec<Track>,
    /// The songs whose radios were asked for, each with its radio (empty
    /// when it did not load).
    pub radios: Vec<(Seed, Vec<Track>)>,
    /// The artists whose pages were asked for, each with its page.
    pub artists: Vec<(ArtistSeed, Page)>,
}

/// Asks YouTube for what Radar chooses from: the account's History (and,
/// while the record is thin, Liked Music), the radios of a few favourite
/// songs and the pages of a few favourite artists. At most ten requests,
/// one at a time. A part that does not load is left out; only when nothing
/// does is it an error.
pub async fn gather(session: &Session, listening: &Listening, now: u64) -> Result<Found, ApiError> {
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
    note(
        session
            .history()
            .await
            .map(|page| found.history = page.tracks),
    );
    if listening.is_thin() {
        tokio::time::sleep(PAUSE).await;
        note(
            session
                .liked_songs()
                .await
                .map(|page| found.liked = page.tracks),
        );
    }
    for seed in listening.seeds(&found.history, &found.liked, now) {
        tokio::time::sleep(PAUSE).await;
        let radio = session.up_next(&seed.video_id, None).await;
        let (tracks, result) = match radio {
            Ok((tracks, _)) => (tracks, Ok(())),
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
/// shows it, leaving out what has since been played or marked unwanted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Radar {
    /// When it was made, in seconds since 1970.
    pub made: u64,
    /// "New to you", best first.
    pub songs: Vec<Track>,
    /// The songs whose radios suggested them most (for its line).
    pub because: Vec<String>,
    /// "Artists to try", best first.
    pub artists: Vec<ArtistPick>,
    /// The artists they are like (for its line).
    pub like: Vec<String>,
    /// "New from your artists", newest first.
    pub releases: Vec<ReleasePick>,
}

/// What the listener knows already: the songs and artists noted, and
/// those of YouTube's History and Liked Music.
struct Known {
    songs: HashSet<String>,
    artists: HashSet<String>,
}

impl Known {
    fn of(listening: &Listening, found: &Found) -> Self {
        let mut songs: HashSet<String> = listening.songs.keys().cloned().collect();
        let mut artists: HashSet<String> = HashSet::new();
        for song in listening.songs.values() {
            artists.extend(artist_keys(song.artist_id.as_deref(), &song.artists));
        }
        for track in found.history.iter().chain(&found.liked) {
            songs.insert(track.video_id.clone());
            artists.extend(track_keys(track));
        }
        Self { songs, artists }
    }

    fn artist(&self, id: Option<&str>, name: &str) -> bool {
        artist_keys(id, name)
            .iter()
            .any(|key| self.artists.contains(key))
    }
}

/// How much a kind of row counts: official songs first, then music
/// videos, fans' uploads (covers, "slowed + reverb") last.
fn kind_factor(kind: &TrackKind) -> f64 {
    match kind {
        TrackKind::Song => 1.0,
        TrackKind::MusicVideo => 0.8,
        TrackKind::Unknown => 0.7,
        TrackKind::Other(_) => 0.3,
    }
}

/// Chooses what Radar suggests from what YouTube said ([`gather`]), at
/// `now` (seconds since 1970).
pub fn rank(listening: &Listening, found: &Found, now: u64) -> Radar {
    let known = Known::of(listening, found);
    let (songs, because) = new_songs(listening, found, &known);
    let (artists, like) = artists_to_try(listening, found, &known);
    let releases = new_releases(listening, found, now);
    Radar {
        made: now,
        songs,
        because,
        artists,
        like,
        releases,
    }
}

/// "New to you": the songs the seeds' radios suggest, new to the listener,
/// each scored by its seed's weight and how near the radio's start it
/// stands; a song several radios suggest gains half again for each more.
fn new_songs(listening: &Listening, found: &Found, known: &Known) -> (Vec<Track>, Vec<String>) {
    struct Candidate {
        track: Track,
        score: f64,
        seeds: Vec<usize>,
    }
    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    for (index, (seed, radio)) in found.radios.iter().enumerate() {
        for (place, track) in radio.iter().enumerate() {
            let new = track.video_id != seed.video_id && !known.songs.contains(&track.video_id);
            let wanted = track.playable
                && track
                    .like
                    .is_none_or(|rating| rating == Rating::Indifferent)
                && !listening.unwanted(track);
            if !new || !wanted || track.video_id.is_empty() {
                continue;
            }
            let gain = seed.weight * kind_factor(&track.kind) / (1.0 + place as f64 / 10.0);
            let candidate = candidates
                .entry(track.video_id.clone())
                .or_insert_with(|| Candidate {
                    track: track.clone(),
                    score: 0.0,
                    seeds: Vec::new(),
                });
            candidate.score += gain;
            if !candidate.seeds.contains(&index) {
                candidate.seeds.push(index);
            }
        }
    }
    let mut ranked: Vec<Candidate> = candidates
        .into_values()
        .map(|mut c| {
            c.score *= 1.0 + 0.5 * (c.seeds.len() as f64 - 1.0);
            c
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.track.video_id.cmp(&b.track.video_id))
    });
    let picked = few_per_artist(ranked, SONGS_KEPT, |c| track_keys(&c.track));
    // The seeds behind most of what was picked, for the row's line.
    let mut credit = vec![0usize; found.radios.len()];
    for candidate in &picked {
        for &seed in &candidate.seeds {
            credit[seed] += 1;
        }
    }
    let mut order: Vec<usize> = (0..credit.len()).filter(|&i| credit[i] > 0).collect();
    order.sort_by(|&a, &b| credit[b].cmp(&credit[a]).then(a.cmp(&b)));
    let because = order
        .into_iter()
        .take(2)
        .map(|i| found.radios[i].0.title.clone())
        .filter(|title| !title.is_empty())
        .collect();
    (picked.into_iter().map(|c| c.track).collect(), because)
}

/// The first `count` of `ranked`, at most [`PER_ARTIST`] of one artist.
fn few_per_artist<T>(ranked: Vec<T>, count: usize, keys: impl Fn(&T) -> Vec<String>) -> Vec<T> {
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut picked = Vec::new();
    for item in ranked {
        let keys = keys(&item);
        if keys
            .iter()
            .any(|k| per_artist.get(k).copied().unwrap_or(0) >= PER_ARTIST)
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
/// page lists them; one several pages list gains half again for each more.
fn artists_to_try(
    listening: &Listening,
    found: &Found,
    known: &Known,
) -> (Vec<ArtistPick>, Vec<String>) {
    struct Candidate {
        pick: ArtistPick,
        score: f64,
        from: Vec<usize>,
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
                },
                score: 0.0,
                from: Vec::new(),
            });
            candidate.score += artist.weight / (1.0 + place as f64 / 5.0);
            if !candidate.from.contains(&index) {
                candidate.from.push(index);
            }
        }
    }
    let mut ranked: Vec<Candidate> = candidates
        .into_values()
        .map(|mut c| {
            c.score *= 1.0 + 0.5 * (c.from.len() as f64 - 1.0);
            c
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.pick.id.cmp(&b.pick.id))
    });
    let mut credit = vec![0usize; found.artists.len()];
    for candidate in ranked.iter().take(CARDS_KEPT) {
        for &from in &candidate.from {
            credit[from] += 1;
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
    let picks = ranked
        .into_iter()
        .take(CARDS_KEPT)
        .map(|c| c.pick)
        .collect();
    (picks, like)
}

/// "New from your artists": the favourite artists' albums, EPs and
/// singles from this year or last, that the listener has not played from,
/// newest first, then by how much the artist is played; at most
/// [`PER_ARTIST`] of each.
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
    ranked.sort_by(|(wa, a), (wb, b)| {
        b.year
            .cmp(&a.year)
            .then(wb.total_cmp(wa))
            .then_with(|| a.id.cmp(&b.id))
    });
    few_per_artist(ranked, CARDS_KEPT, |(_, pick)| {
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
    /// Made less than a day before `now`.
    pub fn fresh(&self, now: u64) -> bool {
        now >= self.made && now - self.made < FRESH_FOR
    }

    /// The page: its rows, leaving out what has since been played in
    /// YTFast or marked unwanted (what was kept beyond a row's first ones
    /// comes forward). An empty row is left out.
    pub fn page(&self, listening: &Listening) -> Page {
        let mut sections = Vec::new();
        let songs: Vec<Item> = self
            .songs
            .iter()
            .filter(|track| !listening.knows(&track.video_id) && !listening.unwanted(track))
            .take(SONGS_SHOWN)
            .map(|track| Item::Track(track.clone()))
            .collect();
        if !songs.is_empty() {
            let strapline = if self.because.is_empty() {
                String::new()
            } else {
                format!("Because you play {}", listed(&self.because))
            };
            sections.push(row("New to you", strapline, songs));
        }
        let artists: Vec<Item> = self
            .artists
            .iter()
            .filter(|a| !listening.unwanted_artist(Some(a.id.as_str()), &a.name))
            .take(CARDS_SHOWN)
            .map(|a| {
                Item::Card(Card {
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
        if !artists.is_empty() {
            let strapline = if self.like.is_empty() {
                String::new()
            } else {
                format!("Like {}", listed(&self.like))
            };
            sections.push(row("Artists to try", strapline, artists));
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
        }
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
        listening.rated(&loved, Rating::Like);
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
    fn new_to_you_is_new_official_and_agreed_on() {
        let mut listening = Listening::default();
        let known = song("known", "Z");
        finished(&mut listening, &known, 1, NOW);
        let mut fan = song("fan", "F");
        fan.kind = TrackKind::Other("MUSIC_VIDEO_TYPE_UGC".into());
        let mut liked = song("liked", "L");
        liked.like = Some(Rating::Like);
        let mut gone = song("gone", "G");
        gone.playable = false;
        let found = Found {
            history: vec![song("history", "H")],
            radios: vec![
                (
                    seed("s1", "S", 1.0),
                    vec![
                        song("s1", "S"),
                        song("only1", "X"),
                        known.clone(),
                        song("history", "H"),
                        fan.clone(),
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
        let ids: Vec<&str> = radar.songs.iter().map(|t| t.video_id.as_str()).collect();
        // Suggested by both radios: first, though late in the first.
        assert_eq!(ids[0], "both", "{ids:?}");
        for left_out in ["s1", "known", "history", "liked", "gone"] {
            assert!(!ids.contains(&left_out), "{left_out}: {ids:?}");
        }
        // A fan's upload last.
        assert_eq!(ids.last(), Some(&"fan"), "{ids:?}");
        assert_eq!(radar.because, ["S1", "S2"]);
    }

    #[test]
    fn at_most_two_of_one_artist_in_a_row() {
        let listening = Listening::default();
        let radio: Vec<Track> = (0..6).map(|n| song(&format!("x{n}"), "X")).collect();
        let found = Found {
            radios: vec![(seed("s", "S", 1.0), radio)],
            ..Found::default()
        };
        let radar = rank(&listening, &found, NOW);
        assert_eq!(radar.songs.len(), PER_ARTIST);
    }

    #[test]
    fn not_interested_leaves_out_the_song_then_its_artist() {
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
        let shown = |listening: &Listening| -> Vec<String> {
            radar.page(listening).sections[0]
                .tracks()
                .into_iter()
                .map(|t| t.video_id)
                .collect()
        };
        assert!(shown(&listening).contains(&"x0".to_string()));
        listening.not_interested(&Unwanted::Song(song("x0", "X0")));
        assert!(!shown(&listening).contains(&"x0".to_string()));
        assert!(
            shown(&listening).contains(&"x1".to_string()),
            "its artist stays"
        );
        // Three of one artist's songs disliked or unwanted: the artist goes.
        for n in 0..ARTIST_AFTER {
            listening.rated(&song(&format!("old{n}"), "Y"), Rating::Dislike);
        }
        assert!(!shown(&listening).iter().any(|id| id.starts_with('y')));
        // A song played since is no longer new.
        listening.played(&song("x2", "X1"), 100.0, 100.0, Ending::Finished, NOW);
        assert!(!shown(&listening).contains(&"x2".to_string()));
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
        // Newest first; two of one artist at most.
        assert_eq!(titles, ["Fresh", "Short"]);
        assert_eq!(radar.releases[1].kind, "EP");
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
    fn the_page_says_why_and_leaves_out_empty_rows() {
        let radar = Radar {
            made: NOW,
            songs: vec![song("n", "N")],
            because: vec!["Lemon".into(), "Idol".into()],
            ..Radar::default()
        };
        let page = radar.page(&Listening::default());
        assert_eq!(page.sections.len(), 1);
        assert_eq!(page.sections[0].title, "New to you");
        assert_eq!(
            page.sections[0].strapline,
            "Because you play Lemon and Idol"
        );
        assert_eq!(page.sections[0].shape, Shape::Carousel);
        assert!(radar.fresh(NOW + FRESH_FOR - 1));
        assert!(!radar.fresh(NOW + FRESH_FOR));
        assert!(
            Radar::default()
                .page(&Listening::default())
                .sections
                .is_empty()
        );
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
}
