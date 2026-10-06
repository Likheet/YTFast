//! What YouTube Music says about one song: whether the account likes it,
//! and where its lyrics and related songs are (the `next` reply); and the
//! lyrics themselves (the `browse` reply for the lyrics' page).

use serde_json::Value;

use super::{PAGE_TYPE, collect, find_key, text, toggle_on};
use crate::lyrics::{LyricLine, Lyrics, plain_lines};

/// The account's rating of a song: thumbs up, thumbs down, or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rating {
    Like,
    Dislike,
    Indifferent,
}

impl Rating {
    fn from_status(status: &str) -> Option<Self> {
        match status {
            "LIKE" => Some(Self::Like),
            "DISLIKE" => Some(Self::Dislike),
            "INDIFFERENT" => Some(Self::Indifferent),
            _ => None,
        }
    }
}

/// What the `next` reply says about the song that plays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SongDetails {
    /// The account's rating, when YouTube said.
    pub like: Option<Rating>,
    /// The page of the song's lyrics (`MPLYt...`), for
    /// `Session::lyrics`. `None` when YouTube has no lyrics for it.
    pub lyrics_id: Option<String>,
    /// The page of songs related to it (`MPTRt...`).
    pub related_id: Option<String>,
}

/// Reads [`SongDetails`] from a `next` reply. The rating comes from the
/// like buttons by the player, or else from the playing song's row in
/// the queue.
pub fn song_details(reply: &Value) -> SongDetails {
    SongDetails {
        like: like_status(reply),
        lyrics_id: tab(reply, "MUSIC_PAGE_TYPE_TRACK_LYRICS", "MPLYt"),
        related_id: tab(reply, "MUSIC_PAGE_TYPE_TRACK_RELATED", "MPTRt"),
    }
}

fn like_status(reply: &Value) -> Option<Rating> {
    let playing = reply
        .pointer("/currentVideoEndpoint/watchEndpoint/videoId")
        .and_then(Value::as_str);
    // The like buttons by the player say the rating outright.
    let mut buttons = Vec::new();
    if let Some(overlays) = reply.get("playerOverlays") {
        collect(overlays, "likeButtonRenderer", &mut buttons);
    }
    let button = buttons
        .iter()
        .find(|b| {
            playing.is_some() && b.pointer("/target/videoId").and_then(Value::as_str) == playing
        })
        .or(buttons.first());
    let rating = |button: Option<&&Value>| {
        button
            .and_then(|b| b.get("likeStatus"))
            .and_then(Value::as_str)
            .and_then(Rating::from_status)
    };
    if let Some(like) = rating(button) {
        return Some(like);
    }
    // Otherwise the song's row in the queue, and its menu.
    let row = playing_row(reply, playing)?;
    let mut buttons = Vec::new();
    collect(row, "likeButtonRenderer", &mut buttons);
    if let Some(like) = rating(buttons.first()) {
        return Some(like);
    }
    let mut toggles = Vec::new();
    collect(
        row.get("menu")?,
        "toggleMenuServiceItemRenderer",
        &mut toggles,
    );
    toggles
        .iter()
        // "Add to liked songs", not some other toggle.
        .filter(|t| {
            find_key(t, "likeEndpoint")
                .and_then(|like| like.pointer("/target/videoId"))
                .is_some()
        })
        .find_map(|t| toggle_on(t))
        // Not liked may also be disliked: the menu cannot tell.
        .map(|liked| {
            if liked {
                Rating::Like
            } else {
                Rating::Indifferent
            }
        })
}

/// The queue's row for the song that plays: the one marked as selected,
/// or the one with its ID.
fn playing_row<'a>(reply: &'a Value, playing: Option<&str>) -> Option<&'a Value> {
    let mut rows = Vec::new();
    collect(reply, "playlistPanelVideoRenderer", &mut rows);
    let selected = |row: &&&Value| row.get("selected").and_then(Value::as_bool) == Some(true);
    let is_playing =
        |row: &&&Value| playing.is_some() && row.get("videoId").and_then(Value::as_str) == playing;
    rows.iter()
        .find(|row| selected(row) && (playing.is_none() || is_playing(row)))
        .or_else(|| rows.iter().find(is_playing))
        .or_else(|| rows.iter().find(selected))
        .copied()
}

/// The page behind one of the tabs next to Up next (Lyrics, Related),
/// unless the tab is greyed out.
fn tab(reply: &Value, page_type: &str, prefix: &str) -> Option<String> {
    let tabs = find_key(reply, "watchNextTabbedResultsRenderer")?
        .get("tabs")?
        .as_array()?;
    tabs.iter()
        .filter_map(|t| t.get("tabRenderer"))
        .filter(|t| {
            t.get("unselectable")
                .is_none_or(|u| u.as_bool() == Some(false))
        })
        .filter_map(|t| t.pointer("/endpoint/browseEndpoint"))
        .find_map(|b| {
            let id = b.get("browseId")?.as_str()?;
            let matches = match b.pointer(PAGE_TYPE).and_then(Value::as_str) {
                Some(kind) => kind == page_type,
                None => id.starts_with(prefix),
            };
            matches.then(|| id.to_string())
        })
}

/// A song's lyrics from its lyrics page: timed ones (which YouTube sends
/// to its mobile app) when there are some, plain ones otherwise. `None`
/// when the page has none.
pub fn lyrics(reply: &Value) -> Option<Lyrics> {
    timed_lyrics(reply).or_else(|| plain_lyrics(reply))
}

fn timed_lyrics(reply: &Value) -> Option<Lyrics> {
    let data = find_key(reply, "timedLyricsModel")?.get("lyricsData")?;
    let mut lines: Vec<LyricLine> = data
        .get("timedLyricsData")?
        .as_array()?
        .iter()
        .filter_map(|row| {
            let text = row.get("lyricLine")?.as_str()?.trim().to_string();
            let time = |key: &str| row.get("cueRange")?.get(key).and_then(millis);
            Some(LyricLine {
                start_ms: time("startTimeMilliseconds"),
                end_ms: time("endTimeMilliseconds"),
                text,
            })
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    // Times for only some lines cannot follow the song: words alone then.
    let synced = lines.iter().all(|l| l.start_ms.is_some());
    if synced {
        // A line without an end ends when the next one starts.
        let next_starts: Vec<Option<u64>> = lines.iter().skip(1).map(|l| l.start_ms).collect();
        for (line, next) in lines.iter_mut().zip(next_starts) {
            line.end_ms = line.end_ms.or(next);
        }
    } else {
        for line in &mut lines {
            line.start_ms = None;
            line.end_ms = None;
        }
    }
    Some(Lyrics {
        lines,
        synced,
        source: source(data.get("sourceMessage")),
    })
}

/// Milliseconds, as a number or (as YouTube sends large numbers) a string.
fn millis(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn plain_lyrics(reply: &Value) -> Option<Lyrics> {
    let shelf = find_key(reply, "musicDescriptionShelfRenderer")?;
    let lines = plain_lines(&shelf.get("description").and_then(text)?);
    if lines.is_empty() {
        return None;
    }
    Some(Lyrics {
        lines,
        synced: false,
        source: source(shelf.get("footer")),
    })
}

/// Who provides the lyrics: "Source: Musixmatch" as "Musixmatch".
fn source(node: Option<&Value>) -> String {
    let words = node
        .and_then(|n| n.as_str().map(str::to_string).or_else(|| text(n)))
        .unwrap_or_default();
    // After "Source:", in whatever language.
    let name = words
        .split_once([':', '：'])
        .map_or(words.as_str(), |(_, name)| name)
        .trim();
    if name.is_empty() {
        "YouTube Music".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A `next` reply in the layout ytmusicapi reads, with the parts
    /// these readers use.
    fn next_reply(overlay_status: Option<&str>) -> Value {
        let mut reply = json!({
            "currentVideoEndpoint": {"watchEndpoint": {"videoId": "playing0001"}},
            "contents": {"singleColumnMusicWatchNextResultsRenderer": {"tabbedRenderer": {
                "watchNextTabbedResultsRenderer": {"tabs": [
                    {"tabRenderer": {"title": "Up next", "content": {"musicQueueRenderer": {"content": {
                        "playlistPanelRenderer": {"contents": [
                            {"playlistPanelVideoRenderer": {"videoId": "other000001", "selected": false}},
                            {"playlistPanelVideoRenderer": {
                                "videoId": "playing0001",
                                "selected": true,
                                "menu": {"menuRenderer": {"items": [
                                    {"toggleMenuServiceItemRenderer": {
                                        "defaultIcon": {"iconType": "KEEP"},
                                        "defaultServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": "t"}}
                                    }},
                                    {"toggleMenuServiceItemRenderer": {
                                        "defaultText": {"runs": [{"text": "Remove from liked songs"}]},
                                        "defaultServiceEndpoint": {"likeEndpoint": {
                                            "status": "INDIFFERENT",
                                            "target": {"videoId": "playing0001"}
                                        }},
                                        "toggledServiceEndpoint": {"likeEndpoint": {
                                            "status": "LIKE",
                                            "target": {"videoId": "playing0001"}
                                        }}
                                    }}
                                ]}}
                            }}
                        ]}
                    }}}}},
                    {"tabRenderer": {"title": "Lyrics", "endpoint": {"browseEndpoint": {
                        "browseId": "MPLYt_lyrics1",
                        "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_TRACK_LYRICS"
                        }}
                    }}}},
                    {"tabRenderer": {"title": "Related", "endpoint": {"browseEndpoint": {
                        "browseId": "MPTRt_related1",
                        "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_TRACK_RELATED"
                        }}
                    }}}}
                ]}
            }}}
        });
        if let Some(status) = overlay_status {
            reply["playerOverlays"] = json!({"playerOverlayRenderer": {"actions": [
                {"likeButtonRenderer": {
                    "target": {"videoId": "playing0001"},
                    "likeStatus": status,
                    "likesAllowed": true
                }}
            ]}});
        }
        reply
    }

    #[test]
    fn details_of_the_playing_song() {
        let details = song_details(&next_reply(Some("DISLIKE")));
        assert_eq!(
            details,
            SongDetails {
                like: Some(Rating::Dislike),
                lyrics_id: Some("MPLYt_lyrics1".into()),
                related_id: Some("MPTRt_related1".into()),
            }
        );
        // Without the player's button: the queue row's menu, whose
        // "Remove from liked songs" says the song is liked.
        assert_eq!(song_details(&next_reply(None)).like, Some(Rating::Like));
        assert_eq!(song_details(&Value::Null), SongDetails::default());
    }

    #[test]
    fn greyed_out_and_unlabelled_tabs() {
        // A song without lyrics: the tab is there but cannot be chosen.
        let mut reply = next_reply(Some("LIKE"));
        let tabs = reply
            .pointer_mut("/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs")
            .unwrap();
        tabs[1]["tabRenderer"]["unselectable"] = json!(true);
        // Without the page type, the ID tells.
        tabs[2]["tabRenderer"]["endpoint"]["browseEndpoint"] =
            json!({"browseId": "MPTRt_related1"});
        let details = song_details(&reply);
        assert_eq!(details.like, Some(Rating::Like));
        assert_eq!(details.lyrics_id, None);
        assert_eq!(details.related_id.as_deref(), Some("MPTRt_related1"));
    }

    #[test]
    fn menu_toggle_that_says_its_state() {
        // Newer replies keep "Add to liked songs" first and say whether it
        // is on.
        let reply = json!({"contents": {"playlistPanelRenderer": {"contents": [
            {"playlistPanelVideoRenderer": {
                "videoId": "playing0001",
                "selected": true,
                "menu": {"menuRenderer": {"items": [{"toggleMenuServiceItemRenderer": {
                    "isToggled": false,
                    "defaultServiceEndpoint": {"likeEndpoint": {"status": "LIKE", "target": {"videoId": "playing0001"}}},
                    "toggledServiceEndpoint": {"likeEndpoint": {"status": "INDIFFERENT", "target": {"videoId": "playing0001"}}}
                }}]}}
            }}
        ]}}});
        assert_eq!(song_details(&reply).like, Some(Rating::Indifferent));
        let mut liked = reply.clone();
        liked["contents"]["playlistPanelRenderer"]["contents"][0]["playlistPanelVideoRenderer"]["menu"]
            ["menuRenderer"]["items"][0]["toggleMenuServiceItemRenderer"]["isToggled"] =
            json!(true);
        assert_eq!(song_details(&liked).like, Some(Rating::Like));
    }

    fn timed_reply(rows: Value) -> Value {
        // The mobile app's layout, as ytmusicapi reads it.
        json!({"contents": {"elementRenderer": {"newElement": {"type": {"componentType": {"model": {
            "timedLyricsModel": {"lyricsData": {
                "timedLyricsData": rows,
                "sourceMessage": "Source: LyricFind"
            }}
        }}}}}}})
    }

    #[test]
    fn timed_lyrics() {
        let lyrics = lyrics(&timed_reply(json!([
            {"lyricLine": "I was a liar", "cueRange": {
                "startTimeMilliseconds": "9200", "endTimeMilliseconds": "10630", "metadata": {"id": "1"}}},
            {"lyricLine": "I gave in to the fire", "cueRange": {
                "startTimeMilliseconds": 10680, "metadata": {"id": "2"}}},
            {"lyricLine": "♪", "cueRange": {
                "startTimeMilliseconds": "12540", "endTimeMilliseconds": "15000"}}
        ])))
        .unwrap();
        assert!(lyrics.synced);
        assert_eq!(lyrics.source, "LyricFind");
        assert_eq!(
            lyrics.lines[0],
            LyricLine {
                start_ms: Some(9200),
                end_ms: Some(10630),
                text: "I was a liar".into()
            }
        );
        // No end given: it ends when the next line starts.
        assert_eq!(lyrics.lines[1].start_ms, Some(10680));
        assert_eq!(lyrics.lines[1].end_ms, Some(12540));
        assert_eq!(lyrics.lines.len(), 3);
    }

    #[test]
    fn partly_timed_lyrics_are_words_alone() {
        let lyrics = lyrics(&timed_reply(json!([
            {"lyricLine": "First line", "cueRange": {
                "startTimeMilliseconds": "1000", "endTimeMilliseconds": "2000"}},
            {"lyricLine": "Second line"}
        ])))
        .unwrap();
        assert!(!lyrics.synced);
        assert!(
            lyrics
                .lines
                .iter()
                .all(|l| l.start_ms.is_none() && l.end_ms.is_none())
        );
        assert_eq!(lyrics.lines[1].text, "Second line");
    }

    #[test]
    fn plain_lyrics_and_their_source() {
        // The web client's layout, as ytmusicapi reads it.
        let reply = json!({"contents": {"sectionListRenderer": {"contents": [
            {"musicDescriptionShelfRenderer": {
                "description": {"runs": [{"text": "Today is gonna be the day\nThat they're gonna throw it back to you\n\nVerse two\n"}]},
                "footer": {"runs": [{"text": "Source: Musixmatch"}]}
            }}
        ]}}});
        let lyrics = lyrics(&reply).unwrap();
        assert!(!lyrics.synced);
        assert_eq!(lyrics.source, "Musixmatch");
        let words: Vec<&str> = lyrics.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            words,
            [
                "Today is gonna be the day",
                "That they're gonna throw it back to you",
                "",
                "Verse two"
            ]
        );
        assert_eq!(source(None), "YouTube Music");
        assert_eq!(source(Some(&json!("Quelle: LyricFind"))), "LyricFind");
        assert_eq!(
            lyrics_of(json!({"contents": {"messageRenderer": {}}})),
            None
        );
    }

    fn lyrics_of(reply: Value) -> Option<Lyrics> {
        lyrics(&reply)
    }
}
