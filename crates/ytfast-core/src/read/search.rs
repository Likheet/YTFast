//! Search suggestions: what YouTube Music offers while a search is typed
//! (the `music/get_search_suggestions` reply).

use serde_json::Value;

use super::text;

/// The account's past searches and YouTube Music's suggestions, in
/// YouTube's order, each once. Songs and artists suggested alongside
/// them are left out.
pub fn search_suggestions(reply: &Value) -> Vec<String> {
    let mut found = Vec::new();
    suggestions(reply, &mut found);
    let mut out: Vec<String> = Vec::new();
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
            && !out.iter().any(|o| o.to_lowercase() == words.to_lowercase())
        {
            out.push(words);
        }
    }
    out
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
        assert_eq!(
            search_suggestions(&reply),
            ["faded", "faded alan walker", "faded lyrics"]
        );
        assert!(search_suggestions(&json!({})).is_empty());
    }
}
