//! Musixmatch: lyrics timed word by word, and people's translations.
//!
//! YouTube Music's own lyrics come from Musixmatch, but timed by the line
//! only. Musixmatch's apps also time each word ("richsync"). YTFast asks
//! for them as Musixmatch's Android app does, through ThetaDev's
//! musixmatch-inofficial (MIT). Its author means it for private use, and
//! Musixmatch may refuse it: every answer is optional, and the other
//! sources stand in.
//!
//! Two requests a song: which song it is (`matcher.track.get`, which also
//! says what people have translated its lyrics into, though not always),
//! then its words' times when it has them. A translation is one more, only
//! when wanted, and one for rōmaji (Japanese in Latin letters, written by
//! people: "rj").
//! When Musixmatch says YTFast asks too often, it is left alone for 15
//! minutes.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use musixmatch_inofficial::models::{RichsyncLine, TrackId};
use musixmatch_inofficial::storage::SessionStorage;
use musixmatch_inofficial::{Error, Musixmatch as Client};

use crate::lyrics::{LyricLine, Lyrics, Word};
use crate::redact;

/// How far Musixmatch's length of a song may be from YouTube's, in seconds.
const CLOSE_ENOUGH_SECONDS: f64 = 4.0;
/// How long Musixmatch is left alone once it refuses YTFast.
const REST: Duration = Duration::from_secs(15 * 60);
/// A pause this long between two lines, in seconds, shows as a line of its
/// own (♪), as YouTube Music's lyrics have.
const BREAK_SECONDS: f32 = 4.0;
/// How much of a song's lyrics people must have translated (of 1) for the
/// translation to be offered.
const TRANSLATED_ENOUGH: f32 = 0.8;
/// How long one request may take.
const PATIENCE: Duration = Duration::from_secs(10);

/// A song on Musixmatch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Song {
    pub track_id: u64,
    /// The languages people have translated its lyrics into (ISO 639-1;
    /// "rj" for rōmaji). Musixmatch leaves this out for some songs that
    /// have translations.
    pub translated_to: Vec<String>,
    /// The language its lyrics are in, when Musixmatch says.
    pub language: Option<String>,
}

impl Song {
    /// Whether people may have translated its lyrics into `language`: they
    /// have, or Musixmatch did not say.
    pub fn may_have(&self, language: &str) -> bool {
        self.translated_to.is_empty() || self.translated_to.iter().any(|l| l == language)
    }
}

/// What Musixmatch knows of a song: the song (for its translations), and
/// its lyrics timed word by word when it has them.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub song: Song,
    pub lyrics: Option<Lyrics>,
}

pub struct Musixmatch {
    client: Client,
    /// Asked nothing before then, after Musixmatch refused YTFast.
    resting_until: Mutex<Option<Instant>>,
}

impl Musixmatch {
    /// A client keeping its session (an anonymous token Musixmatch gives
    /// the app; asking for one takes seconds, and Musixmatch gives few) in
    /// `session_file`, readable by this user only.
    pub fn new(session_file: PathBuf) -> Result<Self, String> {
        // The client's reqwest has no encryption engine of its own: it
        // takes the program's, which is YTFast's (ring).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .storage(Box::new(PrivateFile(session_file)))
            .build()
            .map_err(|e| redact::urls(&e.to_string()))?;
        Ok(Self {
            client,
            resting_until: Mutex::new(None),
        })
    }

    /// The song Musixmatch has by this title and artist (and album), and
    /// its lyrics timed word by word. `Ok(None)` when it has none, or one
    /// that is not this song: another length (more than 4 seconds off
    /// `duration`, YouTube's), or another version of it.
    pub async fn find(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: f64,
    ) -> Result<Option<Found>, String> {
        if self.resting() {
            return Ok(None);
        }
        let asked =
            self.client
                .matcher_track(title, artist, album.unwrap_or_default(), true, false, false);
        let track = match patiently(asked).await {
            Ok(track) => track,
            Err(Error::NotFound | Error::NotAvailable) => return Ok(None),
            Err(e) => return Err(self.failed(e)),
        };
        if track.instrumental {
            return Ok(None);
        }
        if !close_enough(track.track_length, duration) || !same_version(title, &track.track_name) {
            log::info!(
                "Musixmatch's song is another version ({} s, not {duration:.0})",
                track.track_length
            );
            return Ok(None);
        }
        let statuses = &track.track_lyrics_translation_status;
        let song = Song {
            track_id: track.track_id,
            translated_to: statuses
                .iter()
                .filter(|s| s.perc >= TRANSLATED_ENOUGH)
                .map(|s| s.to.clone())
                .collect(),
            language: statuses.iter().find_map(|s| s.from.clone()),
        };
        if !track.has_richsync {
            return Ok(Some(Found { song, lyrics: None }));
        }
        let asked = self.client.track_richsync(
            TrackId::TrackId(track.track_id),
            Some(duration as f32),
            Some(CLOSE_ENOUGH_SECONDS as f32),
        );
        let richsync = match patiently(asked).await {
            Ok(richsync) => richsync,
            Err(Error::NotFound | Error::NotAvailable) => {
                return Ok(Some(Found { song, lyrics: None }));
            }
            Err(e) => return Err(self.failed(e)),
        };
        let lines = richsync
            .get_lines()
            .map(|lines| lines_of(&lines))
            .unwrap_or_default();
        let lyrics = (!lines.is_empty()).then(|| Lyrics {
            lines,
            synced: true,
            source: "Musixmatch".into(),
            timed_words: true,
            language: richsync
                .richsync_language
                .clone()
                .or_else(|| song.language.clone()),
            musixmatch: Some(song.clone()),
        });
        Ok(Some(Found { song, lyrics }))
    }

    /// People's translation of a song's lyrics into `language` (ISO 639-1):
    /// each line as written, and its translation.
    pub async fn translation(
        &self,
        track_id: u64,
        language: &str,
    ) -> Result<Vec<(String, String)>, String> {
        if self.resting() {
            return Err("Musixmatch is being left alone for a while".into());
        }
        let asked = self
            .client
            .track_lyrics_translation(TrackId::TrackId(track_id), language);
        match patiently(asked).await {
            Ok(list) => Ok(list
                .lines
                .into_iter()
                .map(|t| (t.matched_line, t.description))
                .collect()),
            Err(Error::NotFound | Error::NotAvailable) => Ok(Vec::new()),
            Err(e) => Err(self.failed(e)),
        }
    }

    fn resting(&self) -> bool {
        let until = *self.resting_until.lock().unwrap_or_else(|e| e.into_inner());
        until.is_some_and(|until| Instant::now() < until)
    }

    /// Why a request failed, in words. Refused (asked too often, or not
    /// allowed), Musixmatch is left alone for a while.
    fn failed(&self, e: Error) -> String {
        if matches!(
            e,
            Error::Ratelimit
                | Error::MusixmatchError {
                    status_code: 401 | 403,
                    ..
                }
        ) {
            *self.resting_until.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(Instant::now() + REST);
            log::warn!("Musixmatch refused YTFast ({e}); it is left alone for 15 minutes");
        }
        redact::urls(&e.to_string())
    }
}

/// A request given [`PATIENCE`] to answer.
async fn patiently<T>(request: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
    tokio::time::timeout(PATIENCE, request)
        .await
        .unwrap_or_else(|_| {
            Err(Error::InvalidData(
                "Musixmatch did not answer in time".into(),
            ))
        })
}

/// Whether Musixmatch's length of a song (whole seconds; 0 when it does
/// not know) is YouTube's.
fn close_enough(length: u32, duration: f64) -> bool {
    length == 0 || (f64::from(length) - duration).abs() <= CLOSE_ENOUGH_SECONDS
}

/// Whether Musixmatch's song is the version asked for: a word in its name
/// that marks another version ("Idol (English Version)" for "Idol", a
/// remix, a live recording) must be in the name asked for too.
fn same_version(asked: &str, found: &str) -> bool {
    const OTHER: [&str; 20] = [
        "english",
        "japanese",
        "korean",
        "chinese",
        "spanish",
        "french",
        "german",
        "italian",
        "portuguese",
        "remix",
        "live",
        "acoustic",
        "instrumental",
        "karaoke",
        "cover",
        "sped",
        "slowed",
        "nightcore",
        "reverb",
        "orchestral",
    ];
    let words = |name: &str| -> HashSet<String> {
        name.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (asked, found) = (words(asked), words(found));
    OTHER
        .iter()
        .all(|word| !found.contains(*word) || asked.contains(*word))
}

/// Musixmatch's lines timed word by word as YTFast's lines: each piece of
/// a line ("l", with when it starts from the line's start) a word, its
/// spaces left out, ending as the next starts. A long pause between two
/// lines becomes a line of its own.
fn lines_of(richsync: &[RichsyncLine]) -> Vec<LyricLine> {
    let ms = |seconds: f32| (seconds.max(0.0) * 1000.0).round() as u64;
    let mut lines = Vec::new();
    for (i, line) in richsync.iter().enumerate() {
        let mut text = String::new();
        let mut words = Vec::new();
        for (k, piece) in line.l.iter().enumerate() {
            let from = text.len();
            text.push_str(&piece.c);
            let words_only = piece.c.trim();
            if words_only.is_empty() {
                continue;
            }
            let start = line.ts + piece.o;
            let end = line.l.get(k + 1).map_or(line.te, |next| line.ts + next.o);
            let from = from + (piece.c.len() - piece.c.trim_start().len());
            words.push(Word {
                start_ms: ms(start),
                end_ms: ms(end.max(start)),
                from,
                to: from + words_only.len(),
            });
        }
        if words.is_empty() {
            text = line.x.trim().to_string();
        } else {
            // Spaces at the line's ends go, and the words' places with them.
            let lead = text.len() - text.trim_start().len();
            text = text.trim().to_string();
            for word in &mut words {
                word.from -= lead;
                word.to -= lead;
            }
        }
        lines.push(LyricLine {
            start_ms: Some(ms(line.ts)),
            end_ms: Some(ms(line.te)),
            text,
            words,
        });
        if let Some(next) = richsync.get(i + 1)
            && next.ts - line.te >= BREAK_SECONDS
        {
            lines.push(LyricLine {
                start_ms: Some(ms(line.te)),
                end_ms: Some(ms(next.ts)),
                ..LyricLine::default()
            });
        }
    }
    lines
}

/// Musixmatch's session, in a file only this user can read.
struct PrivateFile(PathBuf);

impl SessionStorage for PrivateFile {
    fn write(&self, data: &str) {
        if let Err(e) = write_private(&self.0, data) {
            log::info!("Musixmatch's session was not kept: {e}");
        }
    }

    fn read(&self) -> Option<String> {
        std::fs::read_to_string(&self.0)
            .ok()
            .filter(|text| !text.trim().is_empty())
    }
}

/// Writes a file only this user can read.
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up lines in the shape of Musixmatch's (`richsync_body`).
    fn richsync() -> Vec<RichsyncLine> {
        serde_json::from_str(
            r#"[
                {"ts": 12.5, "te": 15.0, "x": "Paper planes tonight",
                 "l": [{"c": "Paper", "o": 0.0}, {"c": " ", "o": 0.4},
                       {"c": "planes", "o": 0.5}, {"c": " ", "o": 1.1},
                       {"c": "tonight", "o": 1.2}]},
                {"ts": 15.2, "te": 17.0, "x": "夜空に",
                 "l": [{"c": "夜", "o": 0.0}, {"c": "空", "o": 0.3},
                       {"c": "に", "o": 0.6}]},
                {"ts": 30.0, "te": 31.5, "x": "Again",
                 "l": [{"c": " Again ", "o": 0.0}]}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn musixmatchs_words_and_times() {
        let lines = lines_of(&richsync());
        let words = |line: &LyricLine| -> Vec<(String, u64, u64)> {
            line.words
                .iter()
                .map(|w| (line.text[w.from..w.to].to_string(), w.start_ms, w.end_ms))
                .collect()
        };
        assert_eq!(lines[0].text, "Paper planes tonight");
        assert_eq!(
            (lines[0].start_ms, lines[0].end_ms),
            (Some(12_500), Some(15_000))
        );
        assert_eq!(
            words(&lines[0]),
            [
                ("Paper".into(), 12_500, 12_900),
                ("planes".into(), 13_000, 13_600),
                ("tonight".into(), 13_700, 15_000)
            ]
        );
        assert_eq!(
            words(&lines[1]),
            [
                ("夜".into(), 15_200, 15_500),
                ("空".into(), 15_500, 15_800),
                ("に".into(), 15_800, 17_000)
            ]
        );
        // Thirteen seconds without words: a line of its own.
        assert_eq!(lines[2].text, "");
        assert_eq!(
            (lines[2].start_ms, lines[2].end_ms),
            (Some(17_000), Some(30_000))
        );
        // Spaces round a line go, its word's place with them.
        assert_eq!(lines[3].text, "Again");
        assert_eq!(words(&lines[3]), [("Again".into(), 30_000, 31_500)]);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn only_the_song_asked_for() {
        assert!(same_version("Idol", "Idol"));
        assert!(same_version("Idol", "アイドル"));
        assert!(!same_version("Idol", "Idol (English Version)"));
        assert!(!same_version("Take On Me", "Take On Me - Live"));
        assert!(same_version("Take On Me (Live)", "Take On Me - Live"));
        assert!(same_version("Take On Me", "Take On Me (2015 Remaster)"));
        assert!(!same_version(
            "Blinding Lights",
            "Blinding Lights (Slowed + Reverb)"
        ));
        assert!(close_enough(213, 214.6));
        assert!(!close_enough(229, 282.0));
        assert!(close_enough(0, 200.0));
    }

    /// Asks Musixmatch itself when `YTFAST_TEST_MUSIXMATCH` names a folder
    /// for its session; otherwise does nothing. Lemon (Kenshi Yonezu) is
    /// timed word by word and translated by people; a length far from the
    /// song's finds nothing; Kaikai Kitan (Eve) has rōmaji by people. (Idol,
    /// YOASOBI, finds the English version of アイドル, the same length:
    /// `lyrics::same_words` leaves it out beside YouTube's own lyrics.)
    #[tokio::test]
    async fn musixmatch_itself() {
        let Ok(folder) = std::env::var("YTFAST_TEST_MUSIXMATCH") else {
            return;
        };
        let client =
            Musixmatch::new(PathBuf::from(folder).join("musixmatch-session.json")).unwrap();
        let started = Instant::now();
        let lemon = client
            .find("Lemon", "Kenshi Yonezu", None, 256.0)
            .await
            .unwrap()
            .expect("Lemon");
        eprintln!("Lemon found in {:?}", started.elapsed());
        let lyrics = lemon.lyrics.as_ref().expect("timed word by word");
        assert!(lyrics.timed_words && lyrics.lines.len() > 20);
        for line in &lyrics.lines {
            assert!(
                line.words
                    .windows(2)
                    .all(|w| w[0].start_ms <= w[1].start_ms)
            );
            assert!(
                line.words
                    .iter()
                    .all(|w| line.text.get(w.from..w.to).is_some())
            );
        }
        // Musixmatch leaves Lemon's translations unsaid, but has them.
        assert!(lemon.song.may_have("en"), "{:?}", lemon.song);
        let english = client.translation(lemon.song.track_id, "en").await.unwrap();
        eprintln!("{} lines translated by people", english.len());
        assert!(english.len() > 10);
        let far = client
            .find("Despacito", "Luis Fonsi", None, 120.0)
            .await
            .unwrap();
        assert!(far.is_none());
        // Rōmaji written by people, for a Japanese song that says it has
        // them.
        let kitan = client
            .find("Kaikai Kitan", "Eve", None, 221.0)
            .await
            .unwrap()
            .expect("Kaikai Kitan");
        assert_eq!(kitan.song.language.as_deref(), Some("ja"));
        assert!(
            kitan.song.translated_to.iter().any(|l| l == "rj"),
            "{:?}",
            kitan.song
        );
        let romaji = client.translation(kitan.song.track_id, "rj").await.unwrap();
        eprintln!("{} lines in rōmaji by people", romaji.len());
        assert!(romaji.len() > 10);
    }
}
