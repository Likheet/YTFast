//! The listener's own numbers, for the "Your stats" page: what they
//! played, for how long, played again, skipped, liked and disliked, this
//! week, this month, or all the time Radar has noted, and which genres,
//! songs and artists. From Radar's record ([`Listening`]), on the computer
//! only.
//!
//! A week and a month are the last 7 and 30 days, from the plays kept with
//! their dates. "All time" counts every song noted since Radar began
//! (1.5.0); older plays were not timed, so it has no time listened, and
//! its likes and dislikes are those of the songs played in YTFast.

use std::collections::{HashMap, HashSet};

use crate::radar::{Listening, Outcome, artist_keys};
use crate::read::{Rating, Track};

/// The songs, and artists, listed in each list at most.
const LISTED: usize = 10;

/// Which time the numbers are for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    #[default]
    Week,
    Month,
    All,
}

impl Period {
    pub const ALL: [Self; 3] = [Self::Week, Self::Month, Self::All];

    /// Its name, on its chip.
    pub fn words(self) -> &'static str {
        match self {
            Self::Week => "This week",
            Self::Month => "This month",
            Self::All => "All time",
        }
    }

    fn days(self) -> Option<u64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::All => None,
        }
    }
}

/// A song and how many times something happened to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Counted {
    pub track: Track,
    pub times: u32,
}

/// An artist and how many of their songs were played.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtistCount {
    pub name: String,
    /// Their page (`UC...`), when known.
    pub id: Option<String>,
    pub plays: u32,
}

/// The listener's numbers over a [`Period`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Every play noted (a play shorter than two seconds is not one).
    pub plays: u32,
    /// Seconds heard; `None` for all time (older plays were not timed).
    pub seconds: Option<u64>,
    /// Songs and artists played.
    pub songs: u32,
    pub artists: u32,
    /// Plays of a song after its first (early skips not counted).
    pub replays: u32,
    /// Plays heard to the end, and plays skipped in their first seconds.
    pub finished: u32,
    pub skips: u32,
    /// Songs liked, and disliked, in YTFast.
    pub likes: u32,
    pub dislikes: u32,
    /// Each genre's share of the plays of a known genre (early skips not
    /// counted), best first.
    pub genres: Vec<(String, f64)>,
    /// The songs played most, the artists played most, the songs skipped
    /// most (each at most [`LISTED`]), and the songs liked and disliked,
    /// newest first.
    pub most_played: Vec<Counted>,
    pub top_artists: Vec<ArtistCount>,
    pub skipped_most: Vec<Counted>,
    pub liked: Vec<Track>,
    pub disliked: Vec<Track>,
}

/// One song's counts over the period.
#[derive(Default)]
struct Tally {
    /// Plays not skipped early, and early skips.
    played: u32,
    skipped: u32,
    /// When it was last played (to order songs played as often).
    last: u64,
}

/// The listener's numbers over `period`, at `now` (seconds since 1970).
pub fn stats(listening: &Listening, period: Period, now: u64) -> Stats {
    let mut tallies: HashMap<String, Tally> = HashMap::new();
    let mut stats = Stats::default();
    match period.days() {
        Some(days) => {
            let since = now.saturating_sub(days * 86_400);
            let mut seconds = 0;
            for play in listening
                .log()
                .iter()
                .filter(|p| p.at >= since && p.at <= now)
            {
                stats.plays += 1;
                seconds += u64::from(play.heard);
                let tally = tallies.entry(play.id.clone()).or_default();
                tally.last = tally.last.max(play.at);
                match play.outcome() {
                    Outcome::Skipped => {
                        stats.skips += 1;
                        tally.skipped += 1;
                    }
                    Outcome::Finished => {
                        stats.finished += 1;
                        tally.played += 1;
                    }
                    Outcome::Played => tally.played += 1,
                }
            }
            stats.seconds = Some(seconds);
            // Each song's rating as last chosen in the period.
            let mut last: HashMap<&str, (u64, Rating, Track)> = HashMap::new();
            for rated in listening
                .ratings()
                .iter()
                .filter(|r| r.at >= since && r.at <= now)
            {
                let track = Track {
                    video_id: rated.id.clone(),
                    title: rated.title.clone(),
                    artists: rated.artists.clone(),
                    ..Track::default()
                };
                last.insert(&rated.id, (rated.at, rated.rating, track));
            }
            let mut rated: Vec<(u64, Rating, Track)> = last.into_values().collect();
            rated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.video_id.cmp(&b.2.video_id)));
            for (_, rating, track) in rated {
                match rating {
                    Rating::Like => stats.liked.push(track),
                    Rating::Dislike => stats.disliked.push(track),
                    Rating::Indifferent => {}
                }
            }
        }
        None => {
            for (id, song) in listening.songs() {
                stats.plays += song.plays;
                stats.finished += song.finished;
                stats.skips += song.skipped;
                tallies.insert(
                    id.clone(),
                    Tally {
                        played: song.plays.saturating_sub(song.skipped),
                        skipped: song.skipped,
                        last: song.last,
                    },
                );
            }
            let mut rated: Vec<(u64, Rating, Track)> = listening
                .songs()
                .iter()
                .filter_map(|(id, song)| {
                    let track = Track {
                        video_id: id.clone(),
                        title: song.title.clone(),
                        artists: song.artists.clone(),
                        artist_id: song.artist_id.clone(),
                        ..Track::default()
                    };
                    Some((song.last, song.rating?, track))
                })
                .collect();
            rated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.video_id.cmp(&b.2.video_id)));
            for (_, rating, track) in rated {
                match rating {
                    Rating::Like => stats.liked.push(track),
                    Rating::Dislike => stats.disliked.push(track),
                    Rating::Indifferent => {}
                }
            }
        }
    }
    stats.likes = stats.liked.len() as u32;
    stats.dislikes = stats.disliked.len() as u32;
    stats.liked.truncate(LISTED);
    stats.disliked.truncate(LISTED);
    stats.songs = tallies.len() as u32;
    stats.replays = tallies.values().map(|t| t.played.saturating_sub(1)).sum();

    let songs = listening.songs();
    let track_of = |id: &str| {
        let song = songs.get(id);
        Track {
            video_id: id.to_string(),
            title: song.map(|s| s.title.clone()).unwrap_or_default(),
            artists: song.map(|s| s.artists.clone()).unwrap_or_default(),
            artist_id: song.and_then(|s| s.artist_id.clone()),
            album_id: song.and_then(|s| s.album_id.clone()),
            ..Track::default()
        }
    };

    // Genres, and artists, by the plays not skipped early.
    let genres_of = listening.song_genres();
    let mut genres: HashMap<String, f64> = HashMap::new();
    let mut artists: HashMap<String, ArtistCount> = HashMap::new();
    // The names each artist's songs give them, to show the commonest.
    let mut names: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut every_artist: HashSet<String> = HashSet::new();
    for (id, tally) in &tallies {
        let Some(song) = songs.get(id) else { continue };
        let keys = artist_keys(song.artist_id.as_deref(), &song.artists);
        let Some(key) = keys.first() else { continue };
        every_artist.insert(key.clone());
        if tally.played == 0 {
            continue;
        }
        if let Some(genre) = genres_of.get(id) {
            *genres.entry(genre.clone()).or_insert(0.0) += f64::from(tally.played);
        }
        let artist = artists.entry(key.clone()).or_insert_with(|| ArtistCount {
            name: String::new(),
            id: song.artist_id.clone(),
            plays: 0,
        });
        artist.plays += tally.played;
        *names
            .entry(key.clone())
            .or_default()
            .entry(first_artist(&song.artists))
            .or_insert(0) += tally.played;
    }
    for (key, artist) in &mut artists {
        if let Some(name) = names.get(key).and_then(|names| {
            names
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.len().cmp(&a.0.len())))
        }) {
            artist.name = name.0.clone();
        }
    }
    stats.artists = every_artist.len() as u32;
    let sum: f64 = genres.values().sum();
    let mut genres: Vec<(String, f64)> = genres
        .into_iter()
        .map(|(genre, plays)| (genre, plays / sum))
        .collect();
    genres.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    stats.genres = genres;

    let mut top_artists: Vec<ArtistCount> = artists.into_values().collect();
    top_artists.sort_by(|a, b| b.plays.cmp(&a.plays).then_with(|| a.name.cmp(&b.name)));
    top_artists.truncate(LISTED);
    stats.top_artists = top_artists;

    // Songs by a count, most first, then the latest played.
    let ranked = |count: &dyn Fn(&Tally) -> u32| -> Vec<Counted> {
        let mut ranked: Vec<(&String, &Tally)> =
            tallies.iter().filter(|(_, t)| count(t) > 0).collect();
        ranked.sort_by(|a, b| {
            count(b.1)
                .cmp(&count(a.1))
                .then(b.1.last.cmp(&a.1.last))
                .then_with(|| a.0.cmp(b.0))
        });
        ranked
            .into_iter()
            .map(|(id, tally)| Counted {
                track: track_of(id),
                times: count(tally),
            })
            .filter(|c| !c.track.title.is_empty())
            .take(LISTED)
            .collect()
    };
    stats.most_played = ranked(&|t| t.played);
    stats.skipped_most = ranked(&|t| t.skipped);
    stats
}

/// The first of a row's artists ("YOASOBI" of "YOASOBI, Ayase"), by its
/// commas only: an artist may have "&" in their name ("Simon &
/// Garfunkel"), and of two, the commonest name across their songs shows.
fn first_artist(artists: &str) -> String {
    artists
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A time heard, in hours and minutes ("12 h 5 min", "45 min").
pub fn time_heard(seconds: u64) -> String {
    let minutes = (seconds + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::Ending;
    use crate::read::TrackKind;

    const NOW: u64 = 1_792_497_600;
    const DAY: u64 = 86_400;

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

    /// This week: a finished three times (once two weeks ago), b skipped
    /// twice early and once heard to its end, c stopped half way; a and b
    /// J-Pop, c Rock. a liked, then b disliked, c liked and unliked.
    fn listener() -> Listening {
        let mut listening = Listening::default();
        let (a, b, c) = (song("a", "A"), song("b", "B"), song("c", "C"));
        listening.played(&a, 200.0, 200.0, Ending::Finished, NOW - 14 * DAY);
        for day in [3, 2, 1] {
            listening.played(&a, 200.0, 200.0, Ending::Finished, NOW - day * DAY);
        }
        listening.played(&b, 5.0, 200.0, Ending::Skipped, NOW - 2 * DAY);
        listening.played(&b, 10.0, 200.0, Ending::Skipped, NOW - DAY);
        listening.played(&b, 200.0, 200.0, Ending::Finished, NOW - 3600);
        listening.played(&c, 100.0, 200.0, Ending::Stopped, NOW - 3600);
        listening.set_genre("a", Some("J-Pop".into()));
        listening.set_genre("b", Some("J-Pop".into()));
        listening.set_genre("c", Some("Rock".into()));
        listening.rated(&a, Rating::Like, NOW - 2 * DAY);
        listening.rated(&b, Rating::Dislike, NOW - DAY);
        listening.rated(&c, Rating::Like, NOW - DAY);
        listening.rated(&c, Rating::Indifferent, NOW - 3600);
        listening
    }

    fn ids(tracks: impl IntoIterator<Item = Track>) -> Vec<String> {
        tracks.into_iter().map(|t| t.video_id).collect()
    }

    #[test]
    fn this_weeks_numbers() {
        let stats = stats(&listener(), Period::Week, NOW);
        assert_eq!(stats.plays, 7);
        assert_eq!(stats.seconds, Some(3 * 200 + 5 + 10 + 200 + 100));
        assert_eq!((stats.songs, stats.artists), (3, 3));
        // a played twice more after its first this week.
        assert_eq!(stats.replays, 2);
        assert_eq!((stats.finished, stats.skips), (4, 2));
        assert_eq!((stats.likes, stats.dislikes), (1, 1));
        assert_eq!(ids(stats.liked), ["a"]);
        assert_eq!(ids(stats.disliked), ["b"]);
        // a three plays, b and c one each (b's early skips not counted).
        let most: Vec<(String, u32)> = stats
            .most_played
            .iter()
            .map(|c| (c.track.video_id.clone(), c.times))
            .collect();
        assert_eq!(most[0], ("a".to_string(), 3));
        assert_eq!(most.len(), 3);
        assert_eq!(stats.most_played[0].track.title, "A");
        let skipped: Vec<(String, u32)> = stats
            .skipped_most
            .iter()
            .map(|c| (c.track.video_id.clone(), c.times))
            .collect();
        assert_eq!(skipped, [("b".to_string(), 2)]);
        assert_eq!(stats.top_artists[0].name, "A");
        assert_eq!(stats.top_artists[0].plays, 3);
        assert_eq!(stats.top_artists[0].id.as_deref(), Some("UCA"));
        // Four J-Pop plays to one Rock.
        assert_eq!(
            stats.genres,
            [("J-Pop".to_string(), 0.8), ("Rock".to_string(), 0.2)]
        );
    }

    #[test]
    fn this_months_and_all_times_numbers() {
        let listening = listener();
        let month = stats(&listening, Period::Month, NOW);
        assert_eq!(month.plays, 8);
        assert_eq!(month.replays, 3);
        let all = stats(&listening, Period::All, NOW);
        assert_eq!(all.plays, 8);
        assert_eq!(all.seconds, None, "older plays were not timed");
        assert_eq!((all.finished, all.skips), (5, 2));
        // The ratings as each song has it now.
        assert_eq!((all.likes, all.dislikes), (1, 1));
        assert_eq!(all.most_played[0].times, 4);
        // A week from now, this week has nothing.
        let later = stats(&listening, Period::Week, NOW + 8 * DAY);
        assert_eq!((later.plays, later.songs, later.likes), (0, 0, 0));
        assert!(later.most_played.is_empty() && later.genres.is_empty());
    }

    #[test]
    fn a_play_noted_by_1_6_0_counts_by_its_ending() {
        // The record of 1.6.0 kept no outcome with each play.
        let old = r#"{"songs":{"x":{"title":"X","artists":"A","plays":2,"finished":1,"skipped":1,"last":1792497000}},
            "log":[{"at":1792490000,"id":"x","ending":"skipped","heard":4},
                   {"at":1792497000,"id":"x","ending":"finished","heard":190}]}"#;
        let listening: Listening = serde_json::from_str(old).unwrap();
        let stats = stats(&listening, Period::Week, NOW);
        assert_eq!((stats.plays, stats.skips, stats.finished), (2, 1, 1));
    }

    #[test]
    fn an_artist_shows_by_the_name_their_songs_give() {
        let mut listening = Listening::default();
        let mut together = song("t1", "Simon & Garfunkel");
        together.artist_id = Some("UCsg".into());
        listening.played(&together, 200.0, 200.0, Ending::Finished, NOW - 60);
        let mut also = song("t2", "Simon & Garfunkel, Someone");
        also.artist_id = Some("UCsg".into());
        listening.played(&also, 200.0, 200.0, Ending::Finished, NOW - 60);
        let stats = stats(&listening, Period::Week, NOW);
        assert_eq!(stats.top_artists[0].name, "Simon & Garfunkel");
        assert_eq!(stats.top_artists[0].plays, 2);
    }

    #[test]
    fn times_heard_read_plainly() {
        assert_eq!(time_heard(0), "0 min");
        assert_eq!(time_heard(45 * 60 + 10), "45 min");
        assert_eq!(time_heard(2 * 3600), "2 h");
        assert_eq!(time_heard(12 * 3600 + 5 * 60), "12 h 5 min");
    }
}
