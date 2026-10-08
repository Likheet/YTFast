//! Lyrics: a song's lines, with when each is sung when that is known.
//!
//! YouTube Music's own lyrics come first (`Session::lyrics`, in
//! `library.rs`). LRCLIB (lrclib.net, a free and open lyrics database)
//! fills in where YouTube has none, or none with times. LRCLIB's timed
//! lyrics are LRC text (`[01:02.03]A line`), read by [`parse_lrc`].

use serde_json::Value;

use crate::redact;

const LRCLIB: &str = "https://lrclib.net/api";
/// LRCLIB asks apps to say who they are.
const USER_AGENT: &str = "YTFast (https://github.com/Likheet/YTFast)";
/// How far a search result's length may be from the song's, in seconds.
const CLOSE_ENOUGH_SECONDS: f64 = 5.0;
/// The one line shown for a song without words.
const INSTRUMENTAL: &str = "♪ Instrumental ♪";

/// One line of lyrics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LyricLine {
    /// When the line is sung, in milliseconds from the start of the song.
    /// `None` for lyrics without times.
    pub start_ms: Option<u64>,
    /// When it ends (usually when the next line starts). `None` for the
    /// last line and for lyrics without times.
    pub end_ms: Option<u64>,
    /// The words. Empty for a pause, or the gap between two verses.
    pub text: String,
}

/// A song's lyrics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// Every line has its time, so the lyrics can follow the song.
    pub synced: bool,
    /// Where they come from, to show with them: "Musixmatch", "LRCLIB".
    pub source: String,
}

/// Reads LRC text: lines that start with their times (`[mm:ss.xx]`,
/// `[mm:ss.xxx]` or `[mm:ss]`, several for a line sung more than once).
/// Tags such as `[ar:Artist]` are skipped, except `[offset:...]`, which
/// moves every line. Lines come out in time order, each ending when the
/// next starts; a time with no words (a pause) is kept as an empty line.
/// Text without any times comes out as lines without times.
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut offset_ms: i64 = 0;
    let mut timed: Vec<(u64, String)> = Vec::new();
    let mut untimed: Vec<String> = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        let mut tagged = false;
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            let tag = rest[1..end].trim();
            if let Some(ms) = lrc_time(tag) {
                times.push(ms);
            } else if let Some((name, value)) = tag.split_once(':')
                && is_tag_name(name)
            {
                if name.eq_ignore_ascii_case("offset") {
                    offset_ms = value.trim().parse().unwrap_or(0);
                }
            } else {
                // "[Chorus]": words, not a tag.
                break;
            }
            tagged = true;
            rest = rest[end + 1..].trim_start();
        }
        let words = rest.trim();
        if !times.is_empty() {
            timed.extend(times.into_iter().map(|t| (t, words.to_string())));
        } else if !tagged && !words.is_empty() {
            untimed.push(words.to_string());
        }
    }
    if timed.is_empty() {
        return untimed
            .into_iter()
            .map(|text| LyricLine {
                text,
                ..LyricLine::default()
            })
            .collect();
    }
    // Stable: lines at the same time keep the file's order.
    timed.sort_by_key(|(t, _)| *t);
    // A positive offset shows the lyrics earlier.
    let starts: Vec<u64> = timed
        .iter()
        .map(|(t, _)| (*t as i64).saturating_sub(offset_ms).max(0) as u64)
        .collect();
    timed
        .into_iter()
        .enumerate()
        .map(|(i, (_, text))| LyricLine {
            start_ms: Some(starts[i]),
            end_ms: starts.get(i + 1).copied(),
            text,
        })
        .collect()
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx` or `mm:ss.xxx` (some files write
/// `mm:ss:xx`) in milliseconds.
fn lrc_time(tag: &str) -> Option<u64> {
    let number = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let (minutes, rest) = tag.split_once(':')?;
    let (seconds, fraction) = rest.split_once(['.', ':']).unwrap_or((rest, ""));
    let minutes = number(minutes.trim())?;
    let seconds = number(seconds)?;
    let millis = if fraction.is_empty() {
        0
    } else {
        number(fraction)?;
        // Tenths, hundredths or thousandths of a second; finer is cut.
        let digits = &fraction[..fraction.len().min(3)];
        number(digits)? * 10u64.pow(3 - digits.len() as u32)
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds.checked_mul(1000)?)?
        .checked_add(millis)
}

/// `ar`, `ti`, `length`, `#`: the name of an LRC tag.
fn is_tag_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic() || c == '#')
}

/// Lyrics without times, a line each. Blank lines between verses are
/// kept; those before and after the lyrics are not.
pub(crate) fn plain_lines(text: &str) -> Vec<LyricLine> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    let (Some(first), Some(last)) = (first, last) else {
        return Vec::new();
    };
    lines[first..=last]
        .iter()
        .map(|l| LyricLine {
            text: l.to_string(),
            ..LyricLine::default()
        })
        .collect()
}

/// A song's lyrics from LRCLIB: the exact song first (title, artist,
/// album and length), then a search by title and artist for the result
/// with timed lyrics whose length is closest to the song's (within five
/// seconds). Timed lyrics are preferred to plain ones. `Ok(None)` when
/// LRCLIB has nothing for the song.
pub async fn lrclib(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_secs: Option<f64>,
) -> Result<Option<Lyrics>, String> {
    let mut query = vec![
        ("track_name", title.to_string()),
        ("artist_name", artist.to_string()),
    ];
    if let Some(album) = album.filter(|a| !a.trim().is_empty()) {
        query.push(("album_name", album.to_string()));
    }
    if let Some(seconds) = duration_secs.filter(|s| s.is_finite() && *s > 0.0) {
        query.push(("duration", (seconds.round() as u64).to_string()));
    }
    // The exact record, if it is timed (or a song without words). Plain
    // words are kept while the search looks for a timed record: LRCLIB
    // often holds the same song more than once (under other album names),
    // and only some are timed.
    let mut plain = None;
    match lrclib_get(http, "get", &query).await? {
        (200, Some(record)) => match from_lrclib(&record) {
            Some(lyrics) if lyrics.synced || is_instrumental(&lyrics) => return Ok(Some(lyrics)),
            found => plain = found,
        },
        // Not found, or not enough to look it up by: search instead.
        (200 | 400 | 404, _) => {}
        (status, _) => return Err(format!("LRCLIB answered with HTTP {status}")),
    }
    query.retain(|(name, _)| matches!(*name, "track_name" | "artist_name"));
    let found = match lrclib_get(http, "search", &query).await {
        Ok((200, results)) => results
            .as_ref()
            .and_then(|r| best_match(r, duration_secs))
            .and_then(from_lrclib),
        // The exact record's words are better than nothing.
        Ok(_) | Err(_) if plain.is_some() => None,
        Ok((status, _)) => return Err(format!("LRCLIB answered with HTTP {status}")),
        Err(e) => return Err(e),
    };
    Ok(match found {
        Some(lyrics) if lyrics.synced || plain.is_none() => Some(lyrics),
        _ => plain,
    })
}

/// Whether `lyrics` is the one line LRCLIB's lyrics are for a song
/// without words.
fn is_instrumental(lyrics: &Lyrics) -> bool {
    matches!(&lyrics.lines[..], [line] if line.text == INSTRUMENTAL)
}

/// One LRCLIB request: its HTTP status, and its JSON when it succeeded.
async fn lrclib_get(
    http: &reqwest::Client,
    path: &str,
    query: &[(&str, String)],
) -> Result<(u16, Option<Value>), String> {
    let failed =
        |e: reqwest::Error| format!("could not reach LRCLIB: {}", redact::urls(&e.to_string()));
    let response = http
        .get(format!("{LRCLIB}/{path}"))
        .query(query)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(failed)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Ok((status, None));
    }
    let text = response.text().await.map_err(failed)?;
    Ok((status, serde_json::from_str(&text).ok()))
}

/// The lyrics in one LRCLIB record: timed ones when it has them, plain
/// ones otherwise, and one "♪ Instrumental ♪" line for a song without
/// words. `None` when the record has none.
pub fn from_lrclib(record: &Value) -> Option<Lyrics> {
    let lyrics = |lines, synced| Lyrics {
        lines,
        synced,
        source: "LRCLIB".to_string(),
    };
    if record.get("instrumental").and_then(Value::as_bool) == Some(true) {
        let line = LyricLine {
            text: INSTRUMENTAL.to_string(),
            ..LyricLine::default()
        };
        return Some(lyrics(vec![line], false));
    }
    if let Some(timed) = record.get("syncedLyrics").and_then(Value::as_str) {
        let lines = parse_lrc(timed);
        if !lines.is_empty() && lines.iter().all(|l| l.start_ms.is_some()) {
            return Some(lyrics(lines, true));
        }
    }
    let lines = plain_lines(record.get("plainLyrics").and_then(Value::as_str)?);
    if lines.is_empty() {
        return None;
    }
    Some(lyrics(lines, false))
}

/// The best of LRCLIB's search results: timed lyrics before plain ones
/// before instrumentals, then the length closest to the song's, within
/// five seconds. Without the song's length, LRCLIB's order decides.
fn best_match(results: &Value, duration_secs: Option<f64>) -> Option<&Value> {
    let has = |record: &Value, key: &str| {
        record
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    results
        .as_array()?
        .iter()
        .filter_map(|record| {
            let rank = if has(record, "syncedLyrics") {
                0
            } else if has(record, "plainLyrics") {
                1
            } else if record.get("instrumental").and_then(Value::as_bool) == Some(true) {
                2
            } else {
                return None;
            };
            let off = match duration_secs {
                Some(seconds) => {
                    let off = (record.get("duration")?.as_f64()? - seconds).abs();
                    if off > CLOSE_ENOUGH_SECONDS {
                        return None;
                    }
                    off
                }
                None => 0.0,
            };
            Some((rank, off, record))
        })
        // The first of equals: LRCLIB's order.
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, _, record)| record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(start: u64, end: Option<u64>, text: &str) -> LyricLine {
        LyricLine {
            start_ms: Some(start),
            end_ms: end,
            text: text.into(),
        }
    }

    #[test]
    fn lrc_times_tags_and_order() {
        let lines = parse_lrc(
            "\u{feff}[ar:Someone]\n[ti:A song]\n[length: 03:20]\n\n\
             [00:12.34]First\r\n\
             [00:15.5] Second \n\
             [00:20.123][01:02.00]Twice\n\
             [00:30]\n\
             [00:31:50]Colon\n\
             [01:00.00][Chorus] words\n",
        );
        assert_eq!(
            lines,
            [
                line(12_340, Some(15_500), "First"),
                line(15_500, Some(20_123), "Second"),
                line(20_123, Some(30_000), "Twice"),
                // A pause: it ends the line before on time.
                line(30_000, Some(31_500), ""),
                line(31_500, Some(60_000), "Colon"),
                line(60_000, Some(62_000), "[Chorus] words"),
                line(62_000, None, "Twice"),
            ]
        );
    }

    #[test]
    fn lrc_offset_and_odd_input() {
        // A positive offset shows lines earlier, never before the start.
        let lines = parse_lrc("[offset:+500]\n[00:00.20]Early\n[00:01.00]Late");
        assert_eq!(
            lines,
            [line(0, Some(500), "Early"), line(500, None, "Late")]
        );
        // Words without any times: lines without times.
        let plain = parse_lrc("One\n\nTwo\n[ar:Tag]");
        assert_eq!(plain.len(), 2);
        assert!(plain.iter().all(|l| l.start_ms.is_none()));
        assert_eq!(plain[1].text, "Two");
        assert!(parse_lrc("").is_empty());
        // Not times: words.
        assert_eq!(parse_lrc("[1:2x]Hey")[0].text, "[1:2x]Hey");
        assert_eq!(lrc_time("99999999999999999:00"), None);
        assert_eq!(lrc_time("02:03.4"), Some(123_400));
        assert_eq!(lrc_time("02:03.4567"), Some(123_456));
    }

    #[test]
    fn plain_text_keeps_verse_gaps() {
        let lines = plain_lines("\n  One\nTwo  \n\nThree\n\n");
        let words: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(words, ["One", "Two", "", "Three"]);
        assert!(plain_lines(" \n ").is_empty());
    }

    #[test]
    fn lrclib_records() {
        let timed = from_lrclib(&json!({
            "trackName": "I Want to Live",
            "duration": 233.0,
            "instrumental": false,
            "plainLyrics": "I feel your breath upon my neck\nThe clock won't stop",
            "syncedLyrics": "[00:17.12] I feel your breath upon my neck\n[00:20.50] The clock won't stop"
        }))
        .unwrap();
        assert!(timed.synced);
        assert_eq!(timed.source, "LRCLIB");
        assert_eq!(
            timed.lines[0],
            line(17_120, Some(20_500), "I feel your breath upon my neck")
        );

        let plain = from_lrclib(&json!({
            "plainLyrics": "Only words\nNo times",
            "syncedLyrics": null
        }))
        .unwrap();
        assert!(!plain.synced);
        assert_eq!(plain.lines.len(), 2);
        assert_eq!(plain.lines[0].start_ms, None);

        let instrumental = from_lrclib(&json!({
            "instrumental": true,
            "plainLyrics": null,
            "syncedLyrics": null
        }))
        .unwrap();
        assert_eq!(instrumental.lines.len(), 1);
        assert_eq!(instrumental.lines[0].text, "♪ Instrumental ♪");

        assert_eq!(
            from_lrclib(&json!({"plainLyrics": "", "syncedLyrics": null})),
            None
        );
        assert_eq!(
            from_lrclib(&json!({"code": 404, "name": "TrackNotFound"})),
            None
        );
    }

    #[test]
    fn lrclib_search_picks_the_closest_timed_lyrics() {
        let results = json!([
            {"id": 1, "duration": 200.0, "syncedLyrics": "[00:01.00]Too long ago", "plainLyrics": "x"},
            {"id": 2, "duration": 232.0, "syncedLyrics": null, "plainLyrics": "Plain only"},
            {"id": 3, "duration": 236.0, "syncedLyrics": "[00:01.00]Close", "plainLyrics": "x"},
            {"id": 4, "duration": 234.0, "syncedLyrics": "[00:01.00]Closest", "plainLyrics": "x"},
            {"id": 5, "duration": 233.0, "instrumental": true}
        ]);
        let id = |r: Option<&Value>| r.and_then(|r| r["id"].as_u64());
        assert_eq!(id(best_match(&results, Some(233.4))), Some(4));
        // Timed lyrics too far off: plain ones that are close enough.
        let far = json!([
            {"id": 1, "duration": 300.0, "syncedLyrics": "[00:01.00]Far"},
            {"id": 2, "duration": 231.0, "plainLyrics": "Near"}
        ]);
        assert_eq!(id(best_match(&far, Some(233.0))), Some(2));
        // Nothing close enough.
        assert_eq!(id(best_match(&far, Some(100.0))), None);
        // Without the song's length: LRCLIB's order, timed first.
        assert_eq!(id(best_match(&results, None)), Some(1));
        assert_eq!(best_match(&json!({"error": "x"}), None), None);
        assert_eq!(best_match(&json!([]), Some(1.0)), None);
    }
}
