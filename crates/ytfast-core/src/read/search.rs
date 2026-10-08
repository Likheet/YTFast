//! Search suggestions: what YouTube Music offers while a search is typed
//! (the `music/get_search_suggestions` reply).

use serde_json::Value;

use super::text;
use crate::read::Item;

/// What YouTube Music suggests while a search is typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suggestions {
    /// Words to search for: the account's past searches and YouTube
    /// Music's suggestions, in its order, each once.
    pub words: Vec<SuggestedWords>,
    /// The songs, artists and albums suggested under them, with pictures.
    pub items: Vec<Item>,
}

/// Words to search for, as YouTube Music suggests them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuggestedWords {
    pub text: String,
    /// For one of the account's past searches: what removes it from the
    /// search history (`feedbackEndpoint.feedbackToken`).
    pub forget: Option<String>,
}

/// What a `music/get_search_suggestions` reply suggests.
pub fn search_suggestions(reply: &Value) -> Suggestions {
    let mut rows = Vec::new();
    list_rows(reply, &mut rows);
    Suggestions {
        words: suggested_words(reply),
        items: rows.into_iter().filter_map(super::page::item).collect(),
    }
}

/// The past searches and suggestions, in YouTube's order, each once.
fn suggested_words(reply: &Value) -> Vec<SuggestedWords> {
    let mut found = Vec::new();
    suggestions(reply, &mut found);
    let mut out: Vec<SuggestedWords> = Vec::new();
    for suggestion in found {
        let words = suggestion
            .get("suggestion")
            .and_then(text)
            .filter(|w| !w.trim().is_empty())
            .or_else(|| {
                suggestion
                    .pointer("/navigationEndpoint/searchEndpoint/query")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        if let Some(words) = words.map(|w| w.trim().to_string())
            && !words.is_empty()
            && !out
                .iter()
                .any(|o| o.text.to_lowercase() == words.to_lowercase())
        {
            let forget = suggestion
                .pointer("/serviceEndpoint/feedbackEndpoint/feedbackToken")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            out.push(SuggestedWords {
                text: words,
                forget,
            });
        }
    }
    out
}

/// Every row of songs, artists or albums under `node` (each the object
/// holding its `musicResponsiveListItemRenderer`), in order.
fn list_rows<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            if map.contains_key("musicResponsiveListItemRenderer") {
                out.push(node);
                return;
            }
            map.values().for_each(|v| list_rows(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| list_rows(v, out)),
        _ => {}
    }
}

/// Every suggestion under `node`, past searches and new ones, in order.
fn suggestions<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "searchSuggestionRenderer" || key == "historySuggestionRenderer" {
                    out.push(value);
                } else {
                    suggestions(value, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| suggestions(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn suggestions_in_order_once_each() {
        // The layout ytmusicapi reads: suggestions with the typed part in
        // bold, then songs and artists.
        let reply = json!({"contents": [
            {"searchSuggestionsSectionRenderer": {"contents": [
                {"historySuggestionRenderer": {
                    "suggestion": {"runs": [{"text": "fade", "bold": true}, {"text": "d"}]},
                    "navigationEndpoint": {"searchEndpoint": {"query": "faded"}},
                    "serviceEndpoint": {"feedbackEndpoint": {"feedbackToken": "t"}}
                }},
                {"searchSuggestionRenderer": {
                    "suggestion": {"runs": [{"text": "fade", "bold": true}, {"text": "d alan walker"}]},
                    "navigationEndpoint": {"searchEndpoint": {"query": "faded alan walker"}}
                }},
                {"searchSuggestionRenderer": {
                    "suggestion": {"runs": [{"text": "Faded"}]},
                    "navigationEndpoint": {"searchEndpoint": {"query": "faded"}}
                }},
                {"searchSuggestionRenderer": {
                    "navigationEndpoint": {"searchEndpoint": {"query": "faded lyrics"}}
                }}
            ]}},
            {"searchSuggestionsSectionRenderer": {"contents": [
                {"musicResponsiveListItemRenderer": {"flexColumns": []}}
            ]}}
        ]});
        let found = search_suggestions(&reply);
        let texts: Vec<&str> = found.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["faded", "faded alan walker", "faded lyrics"]);
        // Only the past search can be removed from the history.
        assert_eq!(found.words[0].forget.as_deref(), Some("t"));
        assert!(found.words[1..].iter().all(|w| w.forget.is_none()));
        // A row with nothing in it is no suggestion.
        assert!(found.items.is_empty());
        assert_eq!(search_suggestions(&json!({})), Suggestions::default());
    }

    #[test]
    fn real_suggestions_with_pictures() {
        use crate::read::{PageKind, Target};
        let path = format!(
            "{}/tests/fixtures/search_suggestions.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let reply: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let found = search_suggestions(&reply);
        assert_eq!(found.words.len(), 6);
        assert_eq!(found.words[0].text, "coldplay");
        assert!(found.words.iter().all(|w| w.forget.is_none()));
        assert_eq!(found.items.len(), 2);
        // An artist, round, opening its page.
        let Item::Card(artist) = &found.items[0] else {
            panic!("an artist card")
        };
        assert_eq!(artist.title, "Coldplay");
        assert_eq!(artist.subtitle, "332M monthly audience");
        assert!(artist.round);
        assert!(matches!(
            &artist.open,
            Some(Target::Browse { id, kind: PageKind::Artist, .. })
                if id == "UCIaFw5VBEK8qaW6nRpx_qnw"
        ));
        // A song, with its plays and its album.
        let Item::Track(song) = &found.items[1] else {
            panic!("a song")
        };
        assert_eq!(song.title, "Yellow");
        assert_eq!(song.video_id, "9qnqYL0eNNI");
        assert_eq!(song.artists, "Coldplay");
        assert_eq!(song.count(), Some("2.4B plays"));
        assert_eq!(song.album.as_deref(), Some("Parachutes"));
    }
}
