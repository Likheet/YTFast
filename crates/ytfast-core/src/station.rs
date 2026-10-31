//! Radar's station: the Radar page's one button plays new songs, chosen
//! one at a time from Radar's pool ([`Radar::pool`]), and what the
//! listener does with each steers what comes next.
//!
//! Each pick is first a genre: the one furthest behind its share of the
//! listener's genres ([`crate::radar::eased`]), each share leaned by the
//! session (a like raises a genre, a dislike lowers it). Then the song of
//! that genre that suits best now: its score in the pool, times the
//! session's leanings toward its artist and the songs whose radios
//! suggested it. An artist just played waits a few songs. Every fourth
//! pick is a surprise, of any genre, drawn from further down the ranking,
//! so the station keeps finding what the listener did not know they
//! liked, and learns from it.
//!
//! A song liked in the station brings its own radio (one request, asked
//! by the app), whose songs count as much as a favourite's; when few songs
//! are left, the radio of a song liked or heard to its end in the station
//! refills it, at most once every few songs.

use std::collections::{HashMap, HashSet};

use crate::radar::{Listening, Pick, Radar, Random, eased, knows_song, track_keys};
use crate::read::{Rating, Track, TrackKind};

/// How the listener took a song the station played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    Liked,
    Disliked,
    /// Skipped in its first seconds.
    Skipped,
    /// Heard to its end, or nearly.
    Finished,
}

/// Every this many picks, one is a surprise...
const SURPRISE_EVERY: u32 = 4;
/// ...drawn from these places of the ranking, each as likely as the
/// square root of its score.
const SURPRISE_FROM: usize = 5;
const SURPRISE_TO: usize = 40;
/// An artist waits this many picks after one of theirs.
const ARTIST_GAP: usize = 3;
/// With fewer songs than this left, the station asks for a radio...
const LOW: usize = 30;
/// ...and not again before this many more picks.
const ASK_EVERY: u32 = 3;
/// What a radio of a song liked in the station counts, as a favourite's
/// weight counts for its radio.
const LIKED_WEIGHT: f64 = 3.0;
/// How far one reaction moves the station, on the song's artist, its
/// genre, and the songs whose radios suggested it.
const LIKED: [f64; 3] = [1.6, 1.25, 1.3];
const DISLIKED: [f64; 3] = [0.15, 0.75, 0.6];
const SKIPPED: [f64; 3] = [0.6, 0.9, 0.85];
const FINISHED: [f64; 3] = [1.15, 1.05, 1.1];
/// Leanings stay within these.
const LEAST: f64 = 0.05;
const MOST: f64 = 5.0;
/// The share of songs of genres the listener does not play (or of no
/// known genre), when the listener's genres are known.
const OTHERS: f64 = 0.1;
/// An artist leaned below this (a dislike, or a few early skips) is left
/// out of the station from then on.
const SHUT_OUT: f64 = 0.3;

pub struct Station {
    pool: Vec<Pick>,
    /// Songs the station has queued (or played), and songs the listener
    /// knew from YouTube when Radar was made.
    used: HashSet<String>,
    known: HashSet<String>,
    /// The listener's genres, eased, and how many picks each has had
    /// (the others under "").
    targets: Vec<(String, f64)>,
    picked: HashMap<String, u32>,
    count: u32,
    /// The session's leanings: by artist key, by genre, by seed song.
    artists: HashMap<String, f64>,
    genres: HashMap<String, f64>,
    seeds: HashMap<String, f64>,
    /// The artists of the last picks, newest last.
    recent: Vec<Vec<String>>,
    random: Random,
    /// Songs whose radio was asked for; songs liked, and heard to their
    /// end, in the station (newest last); picks since a radio was asked.
    asked: HashSet<String>,
    liked: Vec<String>,
    finished: Vec<String>,
    since_asked: u32,
}

impl Station {
    /// A station of `radar`'s pool, mixed as the listener's genres are now
    /// (`now`, seconds since 1970). `seed` makes its surprises: another
    /// each time.
    pub fn new(radar: &Radar, listening: &Listening, now: u64, seed: u64) -> Self {
        Self {
            pool: radar.pool.clone(),
            used: HashSet::new(),
            known: radar.known.iter().cloned().collect(),
            targets: listening
                .tastes(now)
                .map(|t| eased(&t.genres))
                .unwrap_or_default(),
            picked: HashMap::new(),
            count: 0,
            artists: HashMap::new(),
            genres: HashMap::new(),
            seeds: HashMap::new(),
            recent: Vec::new(),
            random: Random::new(seed),
            asked: HashSet::new(),
            liked: Vec::new(),
            finished: Vec::new(),
            since_asked: ASK_EVERY,
        }
    }

    /// Whether a song of the pool may still play: not played or queued
    /// by the station, new to the listener, not unwanted, and not of an
    /// artist the station turned away from ([`SHUT_OUT`]).
    fn open(&self, pick: &Pick, listening: &Listening) -> bool {
        let id = &pick.track.video_id;
        pick.track.playable
            && !self.used.contains(id)
            && !self.known.contains(id)
            && !listening.knows(id)
            && !listening.unwanted(&pick.track)
            && self.leaning(&track_keys(&pick.track)) >= SHUT_OUT
    }

    /// The session's leaning toward an artist (by its keys): 1 until a
    /// reaction moves it.
    fn leaning(&self, keys: &[String]) -> f64 {
        keys.iter()
            .find_map(|k| self.artists.get(k))
            .copied()
            .unwrap_or(1.0)
    }

    /// The songs that may still play.
    pub fn left(&self, listening: &Listening) -> usize {
        self.pool.iter().filter(|p| self.open(p, listening)).count()
    }

    /// Whether the song came from the station.
    pub fn owns(&self, video_id: &str) -> bool {
        self.pool.iter().any(|p| p.track.video_id == video_id)
    }

    /// The genre a song is counted under: its own when the listener plays
    /// it, else none (the others).
    fn bucket<'a>(&self, genre: Option<&'a str>) -> Option<&'a str> {
        genre.filter(|g| self.targets.iter().any(|(t, _)| t == g))
    }

    /// The genre whose turn it is: of those with songs left, the one
    /// furthest behind its share (leaned by the session), `None` for the
    /// others. Only when the listener's genres are known.
    fn turn<'a>(&self, open: &[(usize, f64, Option<&'a str>)]) -> Option<Option<&'a str>> {
        if self.targets.is_empty() {
            return None;
        }
        let mut shares: Vec<(Option<&'a str>, f64)> = Vec::new();
        for (_, _, genre) in open {
            let bucket = self.bucket(*genre);
            if shares.iter().any(|(b, _)| *b == bucket) {
                continue;
            }
            let share = match bucket {
                Some(g) => {
                    let target = self
                        .targets
                        .iter()
                        .find(|(t, _)| t == g)
                        .map_or(0.0, |(_, share)| *share);
                    target * self.genres.get(g).copied().unwrap_or(1.0)
                }
                None => OTHERS,
            };
            shares.push((bucket, share));
        }
        let sum: f64 = shares.iter().map(|(_, share)| share).sum();
        if sum <= 0.0 {
            return None;
        }
        let picks = f64::from(self.count + 1);
        shares
            .into_iter()
            .map(|(bucket, share)| {
                let had = f64::from(
                    self.picked
                        .get(bucket.unwrap_or_default())
                        .copied()
                        .unwrap_or(0),
                );
                (bucket, share / sum * picks - had, share)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(bucket, _, _)| bucket)
    }

    /// How well a song suits now, within its genre.
    fn score(&self, pick: &Pick) -> f64 {
        let keys = track_keys(&pick.track);
        let artist = self.leaning(&keys);
        let genre = pick
            .genre
            .as_ref()
            .and_then(|g| self.genres.get(g))
            .copied()
            .unwrap_or(1.0);
        let seed = pick
            .seeds
            .iter()
            .map(|s| self.seeds.get(s).copied().unwrap_or(1.0))
            .reduce(f64::max)
            .unwrap_or(1.0);
        let waits = self
            .recent
            .iter()
            .rev()
            .take(ARTIST_GAP)
            .any(|played| played.iter().any(|k| keys.contains(k)));
        let gap = if waits { 0.2 } else { 1.0 };
        pick.score * artist * genre * seed * gap
    }

    /// The next song's place in the pool: the best of the genre whose
    /// turn it is, or, every [`SURPRISE_EVERY`] picks, a surprise of any
    /// genre from further down.
    fn choose(&mut self, listening: &Listening) -> Option<usize> {
        let mut open: Vec<(usize, f64, Option<&str>)> = self
            .pool
            .iter()
            .enumerate()
            .filter(|(_, p)| self.open(p, listening))
            .map(|(i, p)| (i, self.score(p), p.genre.as_deref()))
            .collect();
        open.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let surprise = self.count % SURPRISE_EVERY == SURPRISE_EVERY - 1;
        if surprise && open.len() > SURPRISE_FROM {
            let range = &open[SURPRISE_FROM..open.len().min(SURPRISE_TO)];
            let total: f64 = range.iter().map(|(_, s, _)| s.max(0.0).sqrt()).sum();
            if total > 0.0 {
                let mut at = self.random.unit() * total;
                for (i, score, _) in range {
                    at -= score.max(0.0).sqrt();
                    if at <= 0.0 {
                        return Some(*i);
                    }
                }
                return range.last().map(|(i, _, _)| *i);
            }
        }
        let turn = self.turn(&open);
        open.iter()
            .find(|(_, _, genre)| turn.is_none_or(|bucket| self.bucket(*genre) == bucket))
            .map(|(i, _, _)| *i)
    }

    /// The next `count` songs to play (fewer when the pool runs out).
    pub fn pick(&mut self, listening: &Listening, count: usize) -> Vec<Track> {
        let mut picks = Vec::new();
        for _ in 0..count {
            let Some(i) = self.choose(listening) else {
                break;
            };
            let pick = &self.pool[i];
            self.used.insert(pick.track.video_id.clone());
            self.count += 1;
            self.since_asked += 1;
            let bucket = self.bucket(pick.genre.as_deref()).unwrap_or_default();
            *self.picked.entry(bucket.to_string()).or_insert(0) += 1;
            self.recent.push(track_keys(&pick.track));
            if self.recent.len() > 8 {
                self.recent.remove(0);
            }
            picks.push(pick.track.clone());
        }
        picks
    }

    /// Songs the station queued that were taken out of the queue before
    /// they played (to be chosen again after a reaction): they may play
    /// later, and count as never picked.
    pub fn give_back(&mut self, video_ids: &[String]) {
        for id in video_ids {
            if !self.used.remove(id) {
                continue;
            }
            let Some(pick) = self.pool.iter().find(|p| &p.track.video_id == id) else {
                continue;
            };
            self.count = self.count.saturating_sub(1);
            let bucket = self.bucket(pick.genre.as_deref()).unwrap_or_default();
            if let Some(had) = self.picked.get_mut(bucket) {
                *had = had.saturating_sub(1);
            }
            let keys = track_keys(&pick.track);
            if let Some(at) = self.recent.iter().rposition(|k| *k == keys) {
                self.recent.remove(at);
            }
        }
    }

    /// The listener took `track` (a song the station played) this way:
    /// the station leans toward, or away from, its artist, its genre and
    /// the songs whose radios suggested it. For a song liked, the song
    /// whose radio to ask for now (once per song).
    pub fn react(&mut self, track: &Track, reaction: Reaction) -> Option<String> {
        let [artist, genre, seed] = match reaction {
            Reaction::Liked => LIKED,
            Reaction::Disliked => DISLIKED,
            Reaction::Skipped => SKIPPED,
            Reaction::Finished => FINISHED,
        };
        let lean = |map: &mut HashMap<String, f64>, key: &str, by: f64| {
            let factor = map.entry(key.to_string()).or_insert(1.0);
            *factor = (*factor * by).clamp(LEAST, MOST);
        };
        for key in track_keys(track) {
            lean(&mut self.artists, &key, artist);
        }
        let pick = self
            .pool
            .iter()
            .find(|p| p.track.video_id == track.video_id);
        if let Some(pick) = pick {
            if let Some(g) = &pick.genre {
                lean(&mut self.genres, g, genre);
            }
            for s in &pick.seeds {
                lean(&mut self.seeds, s, seed);
            }
        }
        let id = &track.video_id;
        match reaction {
            Reaction::Liked => {
                // Songs its radio brings lean its way too.
                lean(&mut self.seeds, id, LIKED[2]);
                self.liked.retain(|l| l != id);
                self.liked.push(id.clone());
                if self.asked.insert(id.clone()) {
                    self.since_asked = 0;
                    return Some(id.clone());
                }
            }
            Reaction::Finished => {
                self.finished.retain(|f| f != id);
                self.finished.push(id.clone());
            }
            Reaction::Disliked | Reaction::Skipped => {}
        }
        None
    }

    /// The radio of `seed` (a song played in the station) arrived: its
    /// songs new to the listener join the pool, counted as a favourite's
    /// radio, for the genre `seed` was played for. How many joined.
    pub fn add_radio(&mut self, seed: &Track, radio: &[Track], listening: &Listening) -> usize {
        let genre = self
            .pool
            .iter()
            .find(|p| p.track.video_id == seed.video_id)
            .and_then(|p| p.genre.clone())
            .or_else(|| listening.genre_for(seed));
        let names = listening.song_keys();
        let mut added = 0;
        for (place, track) in radio.iter().enumerate() {
            let id = &track.video_id;
            let new = !id.is_empty()
                && *id != seed.video_id
                && !knows_song(
                    track,
                    |id| self.known.contains(id) || listening.knows(id),
                    &names,
                );
            let wanted = track.kind == TrackKind::Song
                && track.playable
                && track.like.is_none_or(|r| r == Rating::Indifferent)
                && !listening.unwanted(track);
            if !new || !wanted {
                continue;
            }
            let gain = LIKED_WEIGHT / (1.0 + place as f64 / 10.0);
            match self.pool.iter_mut().find(|p| p.track.video_id == *id) {
                Some(pick) => {
                    pick.score += gain;
                    if !pick.seeds.contains(&seed.video_id) {
                        pick.seeds.push(seed.video_id.clone());
                    }
                }
                None => {
                    self.pool.push(Pick {
                        track: track.clone(),
                        score: gain,
                        genre: genre.clone(),
                        seeds: vec![seed.video_id.clone()],
                    });
                    added += 1;
                }
            }
        }
        added
    }

    /// When few songs are left: the song whose radio would refill the
    /// station (the last liked in it, else the last heard to its end, one
    /// not asked for yet), at most once every [`ASK_EVERY`] picks.
    pub fn wants_radio(&mut self, listening: &Listening) -> Option<String> {
        if self.left(listening) >= LOW || self.since_asked < ASK_EVERY {
            return None;
        }
        let next = self
            .liked
            .iter()
            .rev()
            .chain(self.finished.iter().rev())
            .find(|id| !self.asked.contains(*id))
            .cloned()?;
        self.asked.insert(next.clone());
        self.since_asked = 0;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::Ending;
    use crate::read::TrackKind;

    const NOW: u64 = 1_792_497_600;

    fn song(id: &str, artist: &str) -> Track {
        Track {
            video_id: id.to_string(),
            title: id.to_uppercase(),
            artists: artist.to_string(),
            artist_id: Some(format!("UC{artist}")),
            kind: TrackKind::Song,
            playable: true,
            ..Track::default()
        }
    }

    fn pick(id: &str, artist: &str, score: f64, genre: &str, seed: &str) -> Pick {
        Pick {
            track: song(id, artist),
            score,
            genre: (!genre.is_empty()).then(|| genre.to_string()),
            seeds: vec![seed.to_string()],
        }
    }

    fn radar(pool: Vec<Pick>) -> Radar {
        Radar {
            pool,
            ..Radar::default()
        }
    }

    fn ids(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|t| t.video_id.as_str()).collect()
    }

    /// A listener who played `jpop` J-Pop songs and `rock` Rock songs this
    /// week.
    fn listener(jpop: usize, rock: usize) -> Listening {
        let mut listening = Listening::default();
        for (n, genre) in std::iter::repeat_n("J-Pop", jpop)
            .chain(std::iter::repeat_n("Rock", rock))
            .enumerate()
        {
            let track = song(&format!("mine{n}"), &format!("Fav{n}"));
            listening.played(&track, 200.0, 200.0, Ending::Finished, NOW - 3600);
            listening.set_genre(&track.video_id, Some(genre.to_string()));
        }
        listening
    }

    #[test]
    fn picks_new_songs_best_first_never_twice() {
        let mut listening = Listening::default();
        listening.played(&song("heard", "H"), 200.0, 200.0, Ending::Finished, NOW);
        let mut gone = pick("gone", "G", 9.0, "", "s");
        gone.track.playable = false;
        let mut station = Station::new(
            &Radar {
                known: vec!["history".into()],
                ..radar(vec![
                    pick("a", "A", 3.0, "", "s"),
                    pick("heard", "H", 8.0, "", "s"),
                    pick("history", "Y", 7.0, "", "s"),
                    gone,
                    pick("b", "B", 2.0, "", "s"),
                    pick("c", "C", 1.0, "", "s"),
                ])
            },
            &listening,
            NOW,
            1,
        );
        assert_eq!(ids(&station.pick(&listening, 2)), ["a", "b"]);
        assert_eq!(ids(&station.pick(&listening, 5)), ["c"]);
        assert!(station.pick(&listening, 1).is_empty());
        assert!(station.owns("a") && !station.owns("elsewhere"));
    }

    #[test]
    fn an_artist_waits_a_few_songs() {
        let listening = Listening::default();
        let mut station = Station::new(
            &radar(vec![
                pick("x1", "X", 10.0, "", "s"),
                pick("x2", "X", 9.0, "", "s"),
                pick("y1", "Y", 3.0, "", "s"),
                pick("z1", "Z", 2.0, "", "s"),
            ]),
            &listening,
            NOW,
            1,
        );
        assert_eq!(ids(&station.pick(&listening, 3)), ["x1", "y1", "z1"]);
    }

    #[test]
    fn the_genres_are_mixed_as_the_listener_plays_them() {
        // Three J-Pop plays to one Rock: eased, about 63% and 37%.
        let listening = listener(9, 3);
        let mut pool = Vec::new();
        for n in 0..40 {
            // Rock songs score higher: the mix, not the score, decides.
            pool.push(pick(&format!("j{n}"), &format!("J{n}"), 1.0, "J-Pop", "s"));
            pool.push(pick(&format!("r{n}"), &format!("R{n}"), 2.0, "Rock", "s"));
        }
        let mut station = Station::new(&radar(pool), &listening, NOW, 7);
        let picks = station.pick(&listening, 20);
        let jpop = picks.iter().filter(|t| t.video_id.starts_with('j')).count();
        assert!(
            (10..=15).contains(&jpop),
            "{jpop} of 20 J-Pop: {:?}",
            ids(&picks)
        );
    }

    #[test]
    fn a_like_leans_toward_a_dislike_away() {
        let listening = Listening::default();
        let pool = vec![
            pick("a1", "A", 5.0, "J-Pop", "sa"),
            pick("b1", "B", 4.0, "Rock", "sb"),
            pick("a2", "A", 3.0, "J-Pop", "sa"),
            pick("b2", "B", 2.9, "Rock", "sb"),
            pick("c1", "C", 2.8, "Rock", "sc"),
            pick("d1", "D", 2.7, "J-Pop", "sa"),
        ];
        let mut liked = Station::new(&radar(pool.clone()), &listening, NOW, 1);
        let first = liked.pick(&listening, 1);
        assert_eq!(ids(&first), ["a1"]);
        // Liked: its radio is asked for, once; songs like it come first.
        assert_eq!(
            liked.react(&first[0], Reaction::Liked).as_deref(),
            Some("a1")
        );
        assert_eq!(liked.react(&first[0], Reaction::Liked), None);
        assert_eq!(
            ids(&liked.pick(&listening, 1)),
            ["d1"],
            "same genre and seed"
        );
        // Disliked: its artist, genre and seed are left behind.
        let mut disliked = Station::new(&radar(pool), &listening, NOW, 1);
        let first = disliked.pick(&listening, 1);
        assert_eq!(disliked.react(&first[0], Reaction::Disliked), None);
        let next = disliked.pick(&listening, 3);
        assert_eq!(ids(&next), ["b1", "c1", "d1"]);
        // Its artist's other song never comes; even liked first, then
        // disliked, an artist is left out.
        assert_eq!(ids(&disliked.pick(&listening, 10)), ["b2"]);
        let mut both = Station::new(
            &radar(vec![
                pick("x1", "X", 5.0, "", "s"),
                pick("y1", "Y", 1.0, "", "s"),
                pick("x2", "X", 4.0, "", "s"),
            ]),
            &listening,
            NOW,
            1,
        );
        let first = both.pick(&listening, 1);
        both.react(&first[0], Reaction::Liked);
        both.react(&first[0], Reaction::Disliked);
        assert_eq!(ids(&both.pick(&listening, 5)), ["y1"]);
        // A few early skips do the same.
        let mut skipped = Station::new(
            &radar(vec![
                pick("z1", "Z", 5.0, "", "s"),
                pick("z2", "Z", 4.0, "", "s"),
                pick("w1", "W", 1.0, "", "s"),
            ]),
            &listening,
            NOW,
            1,
        );
        let z1 = skipped.pick(&listening, 1);
        for _ in 0..3 {
            skipped.react(&z1[0], Reaction::Skipped);
        }
        assert_eq!(ids(&skipped.pick(&listening, 5)), ["w1"]);
    }

    #[test]
    fn a_liked_songs_radio_joins_the_pool_and_comes_next() {
        let listening = Listening::default();
        let mut station = Station::new(
            &radar(vec![
                pick("a1", "A", 1.0, "J-Pop", "sa"),
                pick("b1", "B", 0.5, "Rock", "sb"),
            ]),
            &listening,
            NOW,
            1,
        );
        let first = station.pick(&listening, 1);
        station.react(&first[0], Reaction::Liked);
        let radio = vec![song("a1", "A"), song("new1", "N"), song("b1", "B")];
        assert_eq!(station.add_radio(&first[0], &radio, &listening), 1);
        assert_eq!(ids(&station.pick(&listening, 2)), ["new1", "b1"]);
    }

    #[test]
    fn songs_given_back_may_play_later() {
        let listening = Listening::default();
        let mut station = Station::new(
            &radar(vec![
                pick("a", "A", 3.0, "", "s"),
                pick("b", "B", 2.0, "", "s"),
            ]),
            &listening,
            NOW,
            1,
        );
        let queued = station.pick(&listening, 2);
        station.give_back(&["b".to_string()]);
        assert_eq!(ids(&station.pick(&listening, 2)), ["b"]);
        assert_eq!(ids(&queued), ["a", "b"]);
    }

    #[test]
    fn every_fourth_pick_is_a_surprise() {
        let listening = Listening::default();
        let pool: Vec<Pick> = (0..30)
            .map(|n| {
                pick(
                    &format!("s{n:02}"),
                    &format!("A{n}"),
                    100.0 - n as f64,
                    "",
                    "s",
                )
            })
            .collect();
        let mut station = Station::new(&radar(pool), &listening, NOW, 3);
        let picks = station.pick(&listening, 4);
        assert_eq!(ids(&picks[..3]), ["s00", "s01", "s02"]);
        // Not the next best: from further down.
        let place: usize = picks[3].video_id[1..].parse().unwrap();
        assert!(place >= 3 + SURPRISE_FROM, "{}", picks[3].video_id);
    }

    #[test]
    fn a_station_running_low_asks_for_a_liked_songs_radio() {
        let listening = Listening::default();
        let pool: Vec<Pick> = (0..10)
            .map(|n| pick(&format!("s{n}"), &format!("A{n}"), 10.0 - n as f64, "", "s"))
            .collect();
        let mut station = Station::new(&radar(pool), &listening, NOW, 1);
        let first = station.pick(&listening, 1);
        // Nothing liked or finished yet: nothing to ask.
        assert_eq!(station.wants_radio(&listening), None);
        station.react(&first[0], Reaction::Finished);
        assert_eq!(station.wants_radio(&listening).as_deref(), Some("s0"));
        // Not again, and not before a few more songs.
        let next = station.pick(&listening, 1);
        station.react(&next[0], Reaction::Finished);
        assert_eq!(station.wants_radio(&listening), None);
        station.pick(&listening, 2);
        assert_eq!(station.wants_radio(&listening).as_deref(), Some("s1"));
    }
}
