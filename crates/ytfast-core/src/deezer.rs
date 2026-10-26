//! Deezer's listeners as a second opinion for Radar: the artists Deezer
//! calls related to a favourite artist, and the songs of that artist's
//! radio there. Two services' listeners agreeing on a new song or artist
//! is good evidence that it suits the listener. Deezer's public API needs
//! no key. Every answer is optional: when Deezer does not answer, Radar
//! goes on with YouTube's alone; when it says YTFast asks too often, it is
//! left alone for 15 minutes.
//!
//! Nothing is played from Deezer: its songs only vote for YouTube's.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

const BASE: &str = "https://api.deezer.com";
/// How long Deezer is left alone once it refuses YTFast.
const REST: Duration = Duration::from_secs(15 * 60);
/// How long one request may take.
const PATIENCE: Duration = Duration::from_secs(10);
/// The pause between two requests, so that they come at a listener's pace.
const PAUSE: Duration = Duration::from_millis(300);
/// Related artists, and radio songs, read per favourite.
const RELATED: usize = 20;
const RADIO: usize = 40;

/// A song in a Deezer radio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub title: String,
    pub artist: String,
}

/// What Deezer's listeners say about a few favourite artists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opinion {
    /// The artists Deezer relates to them, by [`name_key`], each with how
    /// many favourites' lists name it.
    pub artists: HashMap<String, u32>,
    /// The songs of their Deezer radios, by [`song_key`].
    pub songs: HashSet<String>,
    /// Deezer's IDs of the favourites, by [`name_key`], to keep.
    pub ids: BTreeMap<String, u64>,
}

impl Opinion {
    /// How many of Deezer's lists agree with YouTube on a song: its
    /// artist related to a favourite, and the song in a favourite's radio.
    pub fn agrees(&self, title: &str, artist: &str) -> (bool, bool) {
        let artist = first_artist(artist);
        (
            self.artists.contains_key(&name_key(artist)),
            self.songs.contains(&song_key(title, artist)),
        )
    }

    /// How many favourites' lists name this artist.
    pub fn names(&self, artist: &str) -> u32 {
        self.artists
            .get(&name_key(first_artist(artist)))
            .copied()
            .unwrap_or(0)
    }
}

pub struct Deezer {
    client: reqwest::Client,
    /// Asked nothing before then, after Deezer refused YTFast.
    resting_until: Mutex<Option<Instant>>,
}

impl Deezer {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            resting_until: Mutex::new(None),
        }
    }

    /// Asks about `favourites` (artists' names): for each, its Deezer ID
    /// (from `known`, else a search), its related artists and its radio.
    /// At most three requests a favourite, one at a time. What does not
    /// load is left out.
    pub async fn opinion(&self, favourites: &[String], known: &BTreeMap<String, u64>) -> Opinion {
        let mut opinion = Opinion::default();
        for name in favourites {
            let key = name_key(name);
            if key.is_empty() {
                continue;
            }
            let id = match known.get(&key) {
                Some(id) => Some(*id),
                None => {
                    tokio::time::sleep(PAUSE).await;
                    self.artist(name).await.unwrap_or_else(|e| {
                        log::info!("Deezer did not find {name}: {e}");
                        None
                    })
                }
            };
            let Some(id) = id else { continue };
            opinion.ids.insert(key, id);
            tokio::time::sleep(PAUSE).await;
            match self.related(id).await {
                Ok(related) => {
                    for artist in related {
                        *opinion.artists.entry(name_key(&artist)).or_insert(0) += 1;
                    }
                }
                Err(e) => log::info!("Deezer's related artists did not load: {e}"),
            }
            tokio::time::sleep(PAUSE).await;
            match self.radio(id).await {
                Ok(songs) => {
                    opinion
                        .songs
                        .extend(songs.iter().map(|s| song_key(&s.title, &s.artist)));
                }
                Err(e) => log::info!("Deezer's radio did not load: {e}"),
            }
        }
        opinion
    }

    /// Deezer's artist of this name (letters and digits compared, in small
    /// letters): `None` when its search finds none of that name.
    pub async fn artist(&self, name: &str) -> Result<Option<u64>, String> {
        let reply = self
            .get("/search/artist", &[("q", name), ("limit", "5")])
            .await?;
        Ok(artist_id(&reply, name))
    }

    /// The names of the artists Deezer relates to an artist.
    pub async fn related(&self, id: u64) -> Result<Vec<String>, String> {
        let limit = RELATED.to_string();
        let reply = self
            .get(&format!("/artist/{id}/related"), &[("limit", &limit)])
            .await?;
        Ok(related_names(&reply))
    }

    /// The songs of an artist's Deezer radio (mostly other artists').
    pub async fn radio(&self, id: u64) -> Result<Vec<Song>, String> {
        let limit = RADIO.to_string();
        let reply = self
            .get(&format!("/artist/{id}/radio"), &[("limit", &limit)])
            .await?;
        Ok(radio_songs(&reply))
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        let until = *self.resting_until.lock().unwrap_or_else(|e| e.into_inner());
        if until.is_some_and(|until| Instant::now() < until) {
            return Err("Deezer is being left alone for a while".into());
        }
        let asked = self
            .client
            .get(format!("{BASE}{path}"))
            .query(query)
            .timeout(PATIENCE)
            .send();
        let reply: Value = asked
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| crate::redact::urls(&e.to_string()))?
            .json()
            .await
            .map_err(|e| crate::redact::urls(&e.to_string()))?;
        if let Some(error) = reply.get("error") {
            // Code 4: "Quota limit exceeded".
            if error.get("code").and_then(Value::as_u64) == Some(4) {
                *self.resting_until.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(Instant::now() + REST);
                log::warn!("Deezer said YTFast asks too often; it is left alone for 15 minutes");
            }
            let message = error.get("message").and_then(Value::as_str);
            return Err(message.unwrap_or("Deezer refused").to_string());
        }
        Ok(reply)
    }
}

/// A name in letters and digits only, in small letters ("Mrs. GREEN
/// APPLE" and "Mrs. Green Apple" are one), for comparing names between
/// services. Any script's letters count.
pub fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The first of a row's artists ("YOASOBI" of "YOASOBI, Ayase").
fn first_artist(artists: &str) -> &str {
    artists.split([',', '&']).next().unwrap_or_default().trim()
}

/// A song's title and first artist as one key: the title without what
/// one service adds and another does not ("(feat. ...)", "[Remastered]",
/// "- Single Version").
pub fn song_key(title: &str, artist: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0usize;
    for c in title.chars() {
        match c {
            '(' | '[' | '（' | '【' => depth += 1,
            ')' | ']' | '）' | '】' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.split(" - ").next().unwrap_or_default();
    format!("{}|{}", name_key(plain), name_key(first_artist(artist)))
}

/// The ID of the artist of this name in a search's reply.
fn artist_id(reply: &Value, name: &str) -> Option<u64> {
    let wanted = name_key(name);
    reply
        .get("data")?
        .as_array()?
        .iter()
        .find(|artist| {
            artist
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| name_key(n) == wanted)
        })?
        .get("id")?
        .as_u64()
}

fn related_names(reply: &Value) -> Vec<String> {
    let Some(artists) = reply.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    artists
        .iter()
        .filter_map(|a| a.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn radio_songs(reply: &Value) -> Vec<Song> {
    let Some(songs) = reply.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    songs
        .iter()
        .filter_map(|song| {
            let title = song
                .get("title_short")
                .or_else(|| song.get("title"))
                .and_then(Value::as_str)?;
            let artist = song.get("artist")?.get("name")?.as_str()?;
            Some(Song {
                title: title.to_string(),
                artist: artist.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn reads_deezers_search_related_artists_and_radio() {
        // Real replies about YOASOBI (27 October 2026).
        assert_eq!(
            artist_id(&fixture("deezer_search.json"), "yoasobi"),
            Some(108_153_772)
        );
        assert_eq!(artist_id(&fixture("deezer_search.json"), "Ado"), None);
        let related = related_names(&fixture("deezer_related.json"));
        assert_eq!(related.len(), 12);
        assert_eq!(related[..3], ["Eve", "yama", "Kenshi Yonezu"]);
        let radio = radio_songs(&fixture("deezer_radio.json"));
        assert_eq!(radio.len(), 25);
        assert_eq!(
            radio[5],
            Song {
                title: "Hikoutei".into(),
                artist: "King Gnu".into()
            }
        );
        // Nothing found, or an error, reads as nothing.
        assert!(related_names(&serde_json::json!({"data": [], "total": 0})).is_empty());
        assert!(radio_songs(&serde_json::json!({"error": {"code": 4}})).is_empty());
    }

    /// Asks Deezer itself about YOASOBI when `YTFAST_TEST_DEEZER` is set;
    /// otherwise does nothing. Three requests, nothing of the listener's.
    #[tokio::test]
    async fn deezer_itself() {
        if std::env::var_os("YTFAST_TEST_DEEZER").is_none() {
            return;
        }
        let deezer = Deezer::new(crate::net::api_client());
        let opinion = deezer
            .opinion(&["YOASOBI".to_string()], &BTreeMap::new())
            .await;
        eprintln!(
            "{} related artists, {} radio songs",
            opinion.artists.len(),
            opinion.songs.len()
        );
        assert!(opinion.ids.contains_key("yoasobi"));
        assert!(opinion.artists.len() >= 5 && opinion.songs.len() >= 10);
    }

    #[test]
    fn names_and_songs_compare_across_services() {
        assert_eq!(name_key("Mrs. GREEN APPLE"), name_key("Mrs. Green Apple"));
        assert_eq!(name_key("ヨルシカ"), "ヨルシカ");
        assert_ne!(name_key("Eve"), name_key("Evee"));
        assert_eq!(
            song_key("Hikoutei", "King Gnu"),
            song_key(
                "Hikoutei (feat. Someone) - Single Version",
                "King Gnu, Other"
            )
        );
        assert_eq!(
            song_key("Bling‐Bang‐Bang‐Born", "Creepy Nuts"),
            song_key("Bling-Bang-Bang-Born", "Creepy Nuts")
        );
        let opinion = Opinion {
            artists: HashMap::from([(name_key("Eve"), 2)]),
            songs: HashSet::from([song_key("Hikoutei", "King Gnu")]),
            ids: BTreeMap::new(),
        };
        assert_eq!(opinion.agrees("Hikoutei", "King Gnu"), (false, true));
        assert_eq!(
            opinion.agrees("Kaikai Kitan", "Eve & Someone"),
            (true, false)
        );
        assert_eq!(opinion.names("Eve"), 2);
        assert_eq!(opinion.names("Ado"), 0);
    }
}
