//! The configuration the YouTube Music web page carries (`ytcfg`).
//!
//! Loading `https://music.youtube.com/` with the sign-in cookies gives the
//! values the real web client sends with every request: its current version,
//! which signed-in account and channel are active, and the visitor ID.
//! Reading them from the page, instead of hard-coding them, keeps YtFast
//! looking like the current web client and picks the right account.

use serde_json::{Map, Value};

/// What YtFast uses from the page's `ytcfg.set({...})` calls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebConfig {
    pub api_key: Option<String>,
    pub client_version: Option<String>,
    pub visitor_data: Option<String>,
    /// The page's own view of whether the cookies are signed in.
    pub logged_in: Option<bool>,
    /// Which of the browser's signed-in Google accounts this is
    /// (`X-Goog-AuthUser`).
    pub session_index: Option<u32>,
    /// Set when a secondary (brand) channel is active (`X-Goog-PageId`).
    pub delegated_session_id: Option<String>,
    /// Mixed into the request signature when present.
    pub user_session_id: Option<String>,
    /// The page's full client context, sent as the request `context`.
    pub innertube_context: Option<Value>,
}

impl WebConfig {
    /// Reads every `ytcfg.set({...})` object in the page; later values win,
    /// as they do in the browser.
    pub fn from_html(html: &str) -> Self {
        let mut merged = Map::new();
        let mut rest = html;
        while let Some(at) = rest.find("ytcfg.set(") {
            rest = &rest[at + "ytcfg.set(".len()..];
            let trimmed = rest.trim_start();
            if !trimmed.starts_with('{') {
                continue;
            }
            if let Some(object) = json_object_prefix(trimmed)
                && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(object)
            {
                merged.extend(map);
            }
        }
        Self::from_map(&merged)
    }

    fn from_map(map: &Map<String, Value>) -> Self {
        let text = |key: &str| {
            map.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let context = map
            .get("INNERTUBE_CONTEXT")
            .filter(|v| v.is_object())
            .cloned();
        let context_client = |key: &str| {
            context
                .as_ref()
                .and_then(|c| c.pointer(&format!("/client/{key}")))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let session_index = match map.get("SESSION_INDEX") {
            Some(Value::Number(n)) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            Some(Value::String(s)) => s.parse().ok(),
            _ => None,
        };
        let (sync_delegated, sync_user) = parse_data_sync_id(text("DATASYNC_ID").as_deref());
        Self {
            api_key: text("INNERTUBE_API_KEY"),
            client_version: text("INNERTUBE_CLIENT_VERSION")
                .or_else(|| context_client("clientVersion")),
            visitor_data: text("VISITOR_DATA").or_else(|| context_client("visitorData")),
            logged_in: map.get("LOGGED_IN").and_then(Value::as_bool),
            session_index,
            delegated_session_id: text("DELEGATED_SESSION_ID").or(sync_delegated),
            user_session_id: text("USER_SESSION_ID").or(sync_user),
            innertube_context: context,
        }
    }
}

/// `DATASYNC_ID` is `delegated||user` on a secondary channel and `user||`
/// on the main one (as yt-dlp reads it).
fn parse_data_sync_id(id: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return (None, None);
    };
    let (first, second) = id.split_once("||").unwrap_or((id, ""));
    let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
    if second.is_empty() {
        (None, some(first))
    } else {
        (some(first), some(second))
    }
}

/// The JSON object at the start of `text` (which starts with `{`), found by
/// matching braces outside strings.
fn json_object_prefix(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, byte) in text.bytes().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><script nonce="x">window.ytcfg.set({"CLIENT_CANARY_STATE":"none","DEVICE":"cbr=Chrome"});</script>
<script>ytcfg.set({"INNERTUBE_API_KEY":"key-123","INNERTUBE_CLIENT_VERSION":"1.20261001.01.00",
"LOGGED_IN":true,"SESSION_INDEX":"1","DATASYNC_ID":"111||222","VISITOR_DATA":"visitor",
"TRICKY":"a } brace and a \" quote {",
"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB_REMIX","clientVersion":"1.20261001.01.00","hl":"en"},"user":{"lockedSafetyMode":false}}});
ytcfg.set("NOT_AN_OBJECT", 1);</script></html>"#;

    #[test]
    fn reads_the_page_config() {
        let cfg = WebConfig::from_html(PAGE);
        assert_eq!(cfg.api_key.as_deref(), Some("key-123"));
        assert_eq!(cfg.client_version.as_deref(), Some("1.20261001.01.00"));
        assert_eq!(cfg.visitor_data.as_deref(), Some("visitor"));
        assert_eq!(cfg.logged_in, Some(true));
        assert_eq!(cfg.session_index, Some(1));
        assert_eq!(cfg.delegated_session_id.as_deref(), Some("111"));
        assert_eq!(cfg.user_session_id.as_deref(), Some("222"));
        let context = cfg.innertube_context.unwrap();
        assert_eq!(context["client"]["clientName"], "WEB_REMIX");
    }

    #[test]
    fn main_channel_has_no_delegated_session() {
        assert_eq!(
            parse_data_sync_id(Some("999||")),
            (None, Some("999".into()))
        );
        assert_eq!(parse_data_sync_id(Some("999")), (None, Some("999".into())));
        assert_eq!(parse_data_sync_id(None), (None, None));
    }

    #[test]
    fn a_page_without_config_gives_nothing() {
        assert_eq!(
            WebConfig::from_html("<html>consent page</html>"),
            WebConfig::default()
        );
        // An unterminated object is ignored, not a panic.
        assert_eq!(
            WebConfig::from_html("ytcfg.set({\"A\":"),
            WebConfig::default()
        );
    }
}
