//! Taking what is personal out of what the check saves: the home folder
//! (which holds the computer's user name) out of the report, and the
//! account out of YouTube's replies (`--save-replies`).
//!
//! From a reply go: the account's names, handle, email, photo and
//! channel, wherever they appear; every email address; visitorData,
//! tracking and click-tracking params and the other session tokens; the
//! query of every web address but a song picture's (stream and report
//! addresses carry the session there) and the stream servers' names (they
//! say roughly where the computer is). The sign-in's cookie values and the session's own
//! IDs are replaced too, and a reply that still holds one is not saved.

use serde_json::Value;
use ytfast_core::read;

/// What the account's name becomes.
const SAMPLE_NAME: &str = "Sample Listener";
/// What each part of the account's name becomes.
const SAMPLE_NAME_PART: &str = "Sample";
const SAMPLE_HANDLE: &str = "@samplelistener";
const SAMPLE_HANDLE_BARE: &str = "samplelistener";
const SAMPLE_EMAIL: &str = "listener@example.com";
/// A channel ID of the same length (`UC` and 22 more).
const SAMPLE_CHANNEL: &str = "UCsampleListener00000000";
const SAMPLE_PHOTO: &str = "sample-photo";
const SAMPLE_PHOTO_URL: &str = "https://yt3.ggpht.com/sample-photo";
/// What a token or tracking value becomes.
const REDACTED: &str = "redacted";
/// Every stream server becomes this one.
const STREAM_HOST: &str = "rr1---sn-redacted.googlevideo.com";

/// Keys whose values are session tokens or tracking, replaced whole. The
/// readers need none of them, only (for `continuation` and `token`) that
/// there is one.
const TOKEN_KEYS: &[&str] = &[
    "visitorData",
    "trackingParams",
    "clickTrackingParams",
    "trackingParam",
    "serializedContextData",
    "encryptedTokenJarContents",
    "datasyncId",
    "sessionId",
    "heartbeatToken",
    "heartbeatServerData",
    "attestation",
    "challenge",
    "botguardData",
    "feedbackToken",
    "continuation",
    "token",
    "ctoken",
    "cpn",
];

/// Keys that hold the account itself (its menu's header), and what their
/// texts become.
const ACCOUNT_KEYS: &[(&str, &str)] = &[
    ("accountName", SAMPLE_NAME),
    ("channelHandle", SAMPLE_HANDLE),
    ("email", SAMPLE_EMAIL),
    ("accountByline", SAMPLE_EMAIL),
];

/// Keys that hold people's pictures and names: the account's photo, and
/// the faces on a playlist's header (its owner, its collaborators).
const PEOPLE_KEYS: &[&str] = &["accountPhoto", "facepile", "avatarStackViewModel"];

/// Under [`PEOPLE_KEYS`], the keys whose texts are names.
const NAME_KEYS: &[&str] = &["text", "simpleText", "content", "label", "title"];

/// The tracking params kept (each service's `params`): the readers' two
/// account flags, and the page's own ID.
const KEPT_TRACKING: &[&str] = &["logged_in", "has_unlimited_entitlement", "browse_id"];

/// What identifies the account in its replies, read from its account menu,
/// and the session's secrets.
#[derive(Debug, Default)]
pub struct Personal {
    /// The account's names and their parts, its handle and email; replaced
    /// as whole words, longest first.
    words: Vec<Replacement>,
    /// Replaced wherever they appear: the account's channel and photo IDs.
    ids: Vec<Replacement>,
    /// The sign-in's cookie values and the session's own IDs: replaced, and
    /// never written.
    secrets: Vec<String>,
}

#[derive(Debug)]
struct Replacement {
    find: String,
    with: &'static str,
    any_case: bool,
}

impl Personal {
    /// Reads the account from its `account/account_menu` reply. `secrets`
    /// are the cookie values and session IDs (short ones are left out:
    /// they would turn up anywhere). `None` when the reply names no account:
    /// its name could then not be taken out of the other replies.
    pub fn from_account_menu(menu: &Value, secrets: Vec<String>) -> Option<Self> {
        let account = read::account(menu)?;
        let mut names = vec![account.name];
        let mut handles: Vec<String> = account.handle.into_iter().collect();
        let mut emails = Vec::new();
        let mut photos = Vec::new();
        texts_under(menu, &["accountName"], &mut names);
        texts_under(menu, &["channelHandle"], &mut handles);
        texts_under(menu, &["email", "accountByline"], &mut emails);
        urls_under(menu, "accountPhoto", &mut photos);
        let mut channels = Vec::new();
        channel_ids(menu, &mut channels);

        let mut personal = Self {
            secrets: secrets.into_iter().filter(|s| s.len() >= 10).collect(),
            ..Self::default()
        };
        let mut word = |find: &str, with: &'static str, any_case: bool| {
            let find = find.trim();
            if !find.is_empty() && !personal.words.iter().any(|w| w.find == find) {
                personal.words.push(Replacement {
                    find: find.to_string(),
                    with,
                    any_case,
                });
            }
        };
        for name in &names {
            word(name, SAMPLE_NAME, false);
            for part in name.split_whitespace() {
                if part.chars().count() >= 3 {
                    word(part, SAMPLE_NAME_PART, false);
                }
            }
        }
        for handle in &handles {
            word(handle, SAMPLE_HANDLE, false);
            let bare = handle.trim().trim_start_matches('@');
            if bare.chars().count() >= 4 {
                word(bare, SAMPLE_HANDLE_BARE, false);
            }
        }
        for email in &emails {
            if email.contains('@') {
                word(email, SAMPLE_EMAIL, true);
            }
        }
        personal
            .words
            .sort_by_key(|w| std::cmp::Reverse(w.find.len()));
        for channel in channels {
            personal.ids.push(Replacement {
                find: channel,
                with: SAMPLE_CHANNEL,
                any_case: false,
            });
        }
        for photo in photos.iter().filter_map(|url| photo_id(url)) {
            personal.ids.push(Replacement {
                find: photo.to_string(),
                with: SAMPLE_PHOTO,
                any_case: false,
            });
        }
        Some(personal)
    }

    /// One text, with everything personal in it replaced.
    fn clean(&self, text: &str) -> String {
        let mut text = text.to_string();
        for secret in &self.secrets {
            text = replace_text(&text, secret, REDACTED, false, false);
        }
        for id in &self.ids {
            text = replace_text(&text, &id.find, id.with, false, id.any_case);
        }
        for word in &self.words {
            text = replace_text(&text, &word.find, word.with, true, word.any_case);
        }
        strip_addresses(&hide_emails(&text))
    }

    /// Whether `text` still holds one of the secrets.
    pub fn holds_secret(&self, text: &str) -> bool {
        self.secrets.iter().any(|s| text.contains(s.as_str()))
    }
}

/// Takes everything personal out of a reply, in place.
pub fn reply(reply: &mut Value, personal: &Personal) {
    node(reply, "", false, personal);
}

fn node(value: &mut Value, key: &str, among_people: bool, personal: &Personal) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let k = k.as_str();
                if TOKEN_KEYS.contains(&k) {
                    replace_strings(v, REDACTED);
                } else if let Some((_, with)) = ACCOUNT_KEYS.iter().find(|(name, _)| *name == k) {
                    replace_strings(v, with);
                } else if k == "serviceTrackingParams" {
                    keep_account_flags(v);
                    node(v, k, among_people, personal);
                } else if k == "signatureCipher" || k == "cipher" {
                    if let Value::String(text) = v {
                        *text = personal.clean(&without_signature(text));
                    }
                } else {
                    node(v, k, among_people || PEOPLE_KEYS.contains(&k), personal);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                node(item, key, among_people, personal);
            }
        }
        Value::String(text) => {
            *text = if among_people && is_address(text) {
                SAMPLE_PHOTO_URL.to_string()
            } else if among_people && NAME_KEYS.contains(&key) {
                SAMPLE_NAME.to_string()
            } else {
                personal.clean(text)
            };
        }
        _ => {}
    }
}

/// Replaces every text under `value` with `with`.
fn replace_strings(value: &mut Value, with: &str) {
    match value {
        Value::String(text) => *text = with.to_string(),
        Value::Array(items) => items.iter_mut().for_each(|v| replace_strings(v, with)),
        Value::Object(map) => map.values_mut().for_each(|v| replace_strings(v, with)),
        _ => {}
    }
}

/// Keeps only [`KEPT_TRACKING`] of each service's tracking params.
fn keep_account_flags(services: &mut Value) {
    for service in services.as_array_mut().into_iter().flatten() {
        if let Some(params) = service.get_mut("params").and_then(Value::as_array_mut) {
            params.retain(|p| {
                p.get("key")
                    .and_then(Value::as_str)
                    .is_some_and(|key| KEPT_TRACKING.contains(&key))
            });
        }
    }
}

/// A `signatureCipher` (`s=...&sp=sig&url=...`) without its scrambled
/// signature, which is made from the stream address.
fn without_signature(cipher: &str) -> String {
    cipher
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some(("s", _)) => format!("s={REDACTED}"),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Every text under one of `keys`.
fn texts_under(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    fn all(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) if !text.trim().is_empty() => out.push(text.trim().to_string()),
            Value::Array(items) => items.iter().for_each(|v| all(v, out)),
            Value::Object(map) => map.values().for_each(|v| all(v, out)),
            _ => {}
        }
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if keys.contains(&k.as_str()) {
                    all(v, out);
                } else {
                    texts_under(v, keys, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| texts_under(v, keys, out)),
        _ => {}
    }
}

/// Every web address under `key`.
fn urls_under(value: &Value, key: &str, out: &mut Vec<String>) {
    let mut texts = Vec::new();
    texts_under(value, &[key], &mut texts);
    out.extend(texts.into_iter().filter(|t| is_address(t)));
}

/// Every channel ID (`UC` and 22 letters, digits, `-` or `_`) in a reply.
/// In the account's menu, these are the account's own channels.
fn channel_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let is_channel = text.len() == 24
                && text.starts_with("UC")
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if is_channel && !out.contains(text) {
                out.push(text.clone());
            }
        }
        Value::Array(items) => items.iter().for_each(|v| channel_ids(v, out)),
        Value::Object(map) => map.values().for_each(|v| channel_ids(v, out)),
        _ => {}
    }
}

/// The picture's own name in a photo's address, without its size
/// (`.../ytc/AIdro_x=s88-c-k` → `AIdro_x`). Short ones are left out.
fn photo_id(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?;
    let id = last.split('=').next()?;
    (id.len() >= 10).then_some(id)
}

fn is_address(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://") || text.starts_with("//")
}

/// `text` with every `find` replaced by `with`. `whole` keeps to whole
/// words: no letter, digit or `_` may touch a match where `find` itself
/// begins or ends with one. `any_case` ignores the case of ASCII letters.
pub fn replace_text(text: &str, find: &str, with: &str, whole: bool, any_case: bool) -> String {
    if find.is_empty() {
        return text.to_string();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let starts_with_word = find.chars().next().is_some_and(is_word);
    let ends_with_word = find.chars().next_back().is_some_and(is_word);
    let (hay, pattern) = (text.as_bytes(), find.as_bytes());
    let mut out = String::with_capacity(text.len());
    let (mut at, mut copied) = (0, 0);
    while at + pattern.len() <= hay.len() {
        let end = at + pattern.len();
        let window = &hay[at..end];
        let same = if any_case {
            window.eq_ignore_ascii_case(pattern)
        } else {
            window == pattern
        };
        let found = same
            && text.is_char_boundary(at)
            && text.is_char_boundary(end)
            && (!whole
                || ((!starts_with_word || !text[..at].chars().next_back().is_some_and(is_word))
                    && (!ends_with_word || !text[end..].chars().next().is_some_and(is_word))));
        if found {
            out.push_str(&text[copied..at]);
            out.push_str(with);
            at = end;
            copied = end;
        } else {
            at += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` with every email address replaced.
fn hide_emails(text: &str) -> String {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let domain = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let before = &rest[..at];
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| local(*c))
            .last()
            .map_or(at, |(i, _)| i);
        let after = &rest[at + 1..];
        let length = after.find(|c: char| !domain(c)).unwrap_or(after.len());
        let host = after[..length].trim_end_matches('.');
        let parts: Vec<&str> = host.split('.').collect();
        if start < at && parts.len() >= 2 && parts.iter().all(|p| !p.is_empty()) {
            out.push_str(&before[..start]);
            out.push_str(SAMPLE_EMAIL);
            rest = &after[host.len()..];
        } else {
            out.push_str(&rest[..=at]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `text` with every web address cut down to its server and path: the
/// query and fragment go (stream and report addresses carry the session
/// there), and every stream server gets one name. Addresses inside other
/// addresses' fields (`url=https%3A%2F%2F...`) are cut the same way. A
/// song picture's address (`i.ytimg.com`) is left whole.
pub fn strip_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut first = true;
    loop {
        let found = if first && rest.starts_with("//") {
            Some((0, false))
        } else {
            find_address(rest)
        };
        first = false;
        let Some((start, encoded)) = found else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace()
                    || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '\\')
                    || (encoded && c == '&')
            })
            .unwrap_or(tail.len());
        out.push_str(&cut_address(&tail[..end], encoded));
        rest = &tail[end..];
    }
}

/// Where the next address starts, and whether it is percent-encoded.
fn find_address(text: &str) -> Option<(usize, bool)> {
    let lower = text.to_ascii_lowercase();
    [
        ("https://", false),
        ("http://", false),
        ("https%3a%2f%2f", true),
        ("http%3a%2f%2f", true),
    ]
    .iter()
    .filter_map(|(scheme, encoded)| lower.find(scheme).map(|at| (at, *encoded)))
    .min_by_key(|(at, _)| *at)
}

/// One address with only its scheme, server and path.
fn cut_address(address: &str, encoded: bool) -> String {
    let lower = address.to_ascii_lowercase();
    let (slash, query, fragment, scheme_end) = if encoded {
        ("%2f", "%3f", "%23", "%3a%2f%2f")
    } else {
        ("/", "?", "#", "://")
    };
    let host_start = if address.starts_with("//") {
        2
    } else {
        lower.find(scheme_end).map_or(0, |at| at + scheme_end.len())
    };
    let path_end = [query, fragment]
        .iter()
        .filter_map(|mark| lower[host_start..].find(mark))
        .min()
        .map_or(address.len(), |at| host_start + at);
    let host_end = lower[host_start..path_end]
        .find(slash)
        .map_or(path_end, |at| host_start + at);
    let host = &address[host_start..host_end];
    if host.to_ascii_lowercase().ends_with("ytimg.com") {
        // A song's picture: its query only sizes it.
        return address.to_string();
    }
    let host = if host.to_ascii_lowercase().ends_with(".googlevideo.com") {
        STREAM_HOST
    } else {
        host
    };
    format!(
        "{}{host}{}",
        &address[..host_start],
        &address[host_end..path_end]
    )
}

/// `text` with the home folder (it holds the computer's user name) written
/// as `%USERPROFILE%` on Windows and `~` elsewhere.
pub fn without_home(text: &str) -> String {
    let Some(dirs) = directories::UserDirs::new() else {
        return text.to_string();
    };
    let home = dirs.home_dir().to_string_lossy();
    let with = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
    hide_folder(text, &home, with)
}

/// `text` with `folder` replaced, written with either slash and in any
/// case (Windows does not mind either), but not as the start of a longer
/// name (`C:\Users\sam` is not in `C:\Users\samantha`).
fn hide_folder(text: &str, folder: &str, with: &str) -> String {
    let folder = folder.trim_end_matches(['/', '\\']);
    if folder.len() < 2 {
        return text.to_string();
    }
    let mut text = text.to_string();
    for form in [
        folder.to_string(),
        folder.replace('\\', "/"),
        folder.replace('/', "\\"),
    ] {
        text = replace_text(&text, &form, with, true, true);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VISITOR: &str = "CgtWaXNpdG9ySWQxMjM0NQ%3D%3D";
    const COOKIE: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz012345";
    const CHANNEL: &str = "UCabcdefghijklmnopqrstuv";
    const PHOTO: &str = "AIdro_RobinsOwnPhoto1234";

    /// An account menu in the layout ytmusicapi reads, with a made-up
    /// person in it.
    fn menu() -> Value {
        json!({
            "responseContext": {
                "visitorData": VISITOR,
                "serviceTrackingParams": [{"service": "GFEEDBACK", "params": [
                    {"key": "logged_in", "value": "1"},
                    {"key": "ipcc", "value": "0"},
                    {"key": "e", "value": "1,2,3"}
                ]}]
            },
            "actions": [{"openPopupAction": {"popup": {"multiPageMenuRenderer": {
                "header": {"activeAccountHeaderRenderer": {
                    "accountName": {"runs": [{"text": "Robin Testperson"}]},
                    "channelHandle": {"runs": [{"text": "@robintestperson"}]},
                    "email": {"simpleText": "robin.testperson@gmail.com"},
                    "accountPhoto": {"thumbnails": [{"url": format!("https://yt3.ggpht.com/ytc/{PHOTO}=s88-c-k-c0x00ffffff-no-rj"), "width": 88}]}
                }},
                "sections": [{"multiPageMenuSectionRenderer": {"items": [
                    {"compactLinkRenderer": {
                        "title": {"runs": [{"text": "Your channel"}]},
                        "navigationEndpoint": {"browseEndpoint": {"browseId": CHANNEL}}
                    }}
                ]}}]
            }}}}]
        })
    }

    fn personal() -> Personal {
        Personal::from_account_menu(&menu(), vec![VISITOR.into(), COOKIE.into(), "CAI".into()])
            .unwrap()
    }

    /// A reply with the person in every place found so far, and what the
    /// readers need beside it.
    fn page() -> Value {
        json!({
            "responseContext": {
                "visitorData": VISITOR,
                "serviceTrackingParams": [{"service": "GFEEDBACK", "params": [
                    {"key": "has_unlimited_entitlement", "value": "True"},
                    {"key": "logged_in", "value": "1"},
                    {"key": "ipcc", "value": "0"}
                ]}],
                "mainAppWebResponseContext": {"loggedOut": false, "trackingParam": "kx_fmPxhoPZR"}
            },
            "trackingParams": "CAAQhGciEwj",
            "header": {"musicResponsiveHeaderRenderer": {
                "title": {"runs": [{"text": "Get Lucky"}]},
                "straplineTextOne": {"runs": [{
                    "text": "Robin Testperson",
                    "navigationEndpoint": {
                        "clickTrackingParams": "CBQQ8JMBGAAiEwj",
                        "browseEndpoint": {"browseId": CHANNEL, "pageType": "MUSIC_PAGE_TYPE_USER_CHANNEL"}
                    }
                }]},
                "straplineThumbnail": {"thumbnails": [{"url": format!("https://yt3.ggpht.com/ytc/{PHOTO}=s176-c-k")}]},
                "facepile": {"avatarStackViewModel": {
                    "avatars": [{"avatarViewModel": {
                        "image": {"sources": [{"url": "https://yt3.ggpht.com/ytc/AIdro_SomeoneElse12345=s48"}]},
                        "avatarImageSize": "AVATAR_SIZE_XS"
                    }}],
                    "rendererContext": {"accessibilityContext": {"label": "Alex Collaborator"}}
                }},
                "subtitle": {"runs": [{"text": "Robin's mix"}, {"text": " • "}, {"text": "Robin"}]},
                "description": {"runs": [{"text": "Mail me at robin.t@example.org or visit youtube.com/@robintestperson. Cookie AbCdEfGhIjKlMnOpQrStUvWxYz012345 CAI"}]}
            }},
            "thumbnail": {"thumbnails": [{"url": "https://lh3.googleusercontent.com/abcDEF=w60-h60-l90-rj", "width": 60}]},
            "continuations": [{"nextContinuationData": {"continuation": "4qmFsgKbARIMVkxQTGFaUE1zdVFO", "clickTrackingParams": "CBQQ"}}],
            "menu": {"feedbackEndpoint": {"feedbackToken": "AB9zfpLongToken"}},
            "streamingData": {"adaptiveFormats": [
                {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000,
                 "signatureCipher": "s=SCRAMBLEDsig%3D&sp=sig&url=https%3A%2F%2Frr3---sn-ab5l6nr6.googlevideo.com%2Fvideoplayback%3Fexpire%3D1%26ip%3D203.0.113.9%26sig%3Dx",
                 "contentLength": "6000000"},
                {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000,
                 "url": "https://rr3---sn-ab5l6nr6.googlevideo.com/videoplayback?expire=1&ip=203.0.113.9&sig=y"}
            ]},
            "playbackTracking": {
                "videostatsPlaybackUrl": {"baseUrl": "https://s.youtube.com/api/stats/playback?cl=1&docid=abc&ei=secretEi&cpn=secretCpn&vm=secretVm"}
            },
            "videoDetails": {"videoId": "AjXQiKP5kMs", "lengthSeconds": "246"},
            "playerConfig": {"audioConfig": {"loudnessDb": -1.3}}
        })
    }

    fn scrubbed(mut value: Value) -> (Value, String) {
        reply(&mut value, &personal());
        let text = serde_json::to_string(&value).unwrap();
        (value, text)
    }

    #[test]
    fn nothing_personal_is_left() {
        let (_, text) = scrubbed(page());
        for gone in [
            "Robin",
            "Testperson",
            "robintestperson",
            "robin.t@",
            "gmail",
            "Alex Collaborator",
            "SomeoneElse",
            PHOTO,
            CHANNEL,
            VISITOR,
            COOKIE,
            "kx_fmPxhoPZR",
            "CAAQhGciEwj",
            "CBQQ",
            "4qmFsgKbARIMVkxQTGFaUE1zdVFO",
            "AB9zfpLongToken",
            "SCRAMBLEDsig",
            "sn-ab5l6nr6",
            "203.0.113.9",
            "expire",
            "secretEi",
            "secretCpn",
            "secretVm",
            "ipcc",
        ] {
            assert!(!text.contains(gone), "{gone} is still in {text}");
        }
        let (_, menu_text) = scrubbed(menu());
        for gone in ["Robin", "robintestperson", "gmail", PHOTO, CHANNEL, VISITOR] {
            assert!(!menu_text.contains(gone), "{gone} is still in the menu");
        }
        assert!(!personal().holds_secret(&text));
        assert!(personal().holds_secret(&format!("x{COOKIE}x")));
        // A short cookie value would turn up anywhere: it is not looked for.
        assert!(!personal().holds_secret("CAI"));
    }

    #[test]
    fn what_the_readers_need_stays() {
        let (value, text) = scrubbed(page());
        for kept in [
            "Get Lucky",
            "MUSIC_PAGE_TYPE_USER_CHANNEL",
            "AVATAR_SIZE_XS",
            "https://lh3.googleusercontent.com/abcDEF=w60-h60-l90-rj",
            "https://s.youtube.com/api/stats/playback",
            "rr1---sn-redacted.googlevideo.com/videoplayback",
            "Sample Listener",
            "Sample's mix",
            "listener@example.com",
            "youtube.com/@samplelistener",
            SAMPLE_CHANNEL,
        ] {
            assert!(text.contains(kept), "{kept} is missing from {text}");
        }
        let flags = read::account_flags(&value);
        assert_eq!(flags.logged_in, Some(true));
        assert_eq!(flags.premium, Some(true));
        assert!(read::track_continuation(&json!({"contents": [{"musicResponsiveListItemRenderer": {}}], "continuations": value["continuations"]})).is_some());
        let formats = read::stream_formats(&value);
        let best = read::best_stream(&formats).unwrap();
        assert_eq!(best.itag, 141);
        assert_eq!(
            best.signature_cipher.as_deref(),
            Some(
                "s=redacted&sp=sig&url=https%3A%2F%2Frr1---sn-redacted.googlevideo.com%2Fvideoplayback"
            )
        );
        assert_eq!(
            formats[1].url.as_deref(),
            Some("https://rr1---sn-redacted.googlevideo.com/videoplayback")
        );
        let info = read::player_info(&value);
        assert_eq!(info.loudness_db, Some(-1.3));
        assert_eq!(
            info.playback_url.as_deref(),
            Some("https://s.youtube.com/api/stats/playback")
        );

        let (menu, _) = scrubbed(menu());
        let account = read::account(&menu).unwrap();
        assert_eq!(account.name, SAMPLE_NAME);
        assert_eq!(account.handle.as_deref(), Some(SAMPLE_HANDLE));
    }

    #[test]
    fn real_replies_read_the_same_once_scrubbed() {
        for name in [
            "playlist_signed_in_premium.json",
            "playlist_collaborative.json",
            "album.json",
            "artist.json",
        ] {
            let path = format!(
                "{}/../ytfast-core/tests/fixtures/{name}",
                env!("CARGO_MANIFEST_DIR")
            );
            let original: Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let (clean, _) = scrubbed(original.clone());
            assert_eq!(read::tracks(&clean), read::tracks(&original), "{name}");
            assert_eq!(read::page(&clean), read::page(&original), "{name}");
            assert_eq!(
                read::track_continuation(&clean).is_some(),
                read::track_continuation(&original).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn no_account_name_saves_nothing() {
        assert!(Personal::from_account_menu(&json!({"actions": []}), Vec::new()).is_none());
    }

    #[test]
    fn whole_words_only() {
        assert_eq!(
            replace_text("Sam, Samantha and sam", "Sam", "X", true, false),
            "X, Samantha and sam"
        );
        assert_eq!(replace_text("aXa", "X", "Y", false, false), "aYa");
        assert_eq!(
            replace_text("ÄSamÄ Sam", "Sam", "X", true, false),
            "ÄSamÄ X"
        );
        assert_eq!(replace_text("HELLO", "hello", "x", true, true), "x");
    }

    #[test]
    fn emails_go() {
        assert_eq!(
            hide_emails("write to a.b+c@mail.example.com."),
            "write to listener@example.com."
        );
        assert_eq!(hide_emails("@handle and a@b"), "@handle and a@b");
    }

    #[test]
    fn addresses_lose_their_queries() {
        assert_eq!(
            strip_addresses("see https://x.example/p/q?a=1#f and http://y.example?b"),
            "see https://x.example/p/q and http://y.example"
        );
        assert_eq!(
            strip_addresses("//www.google.com/js/th/x.js?y=1"),
            "//www.google.com/js/th/x.js"
        );
        assert_eq!(
            strip_addresses("sp=sig&url=https%3A%2F%2Fa.example%2Fb%3Fc%3D1&x=2"),
            "sp=sig&url=https%3A%2F%2Fa.example%2Fb&x=2"
        );
    }

    #[test]
    fn home_folder_goes() {
        let home = r"C:\Users\Sam";
        assert_eq!(
            hide_folder(
                r"C:\Users\Sam\AppData\Local\YtFast\helpers\yt-dlp.exe is missing",
                home,
                "%USERPROFILE%"
            ),
            r"%USERPROFILE%\AppData\Local\YtFast\helpers\yt-dlp.exe is missing"
        );
        assert_eq!(
            hide_folder("could not move it into c:/users/sam/AppData", home, "~"),
            "could not move it into ~/AppData"
        );
        assert_eq!(
            hide_folder(r"C:\Users\Samantha\x", home, "~"),
            r"C:\Users\Samantha\x"
        );
        assert_eq!(
            hide_folder("/Users/sam/Library/Caches/YtFast", "/Users/sam/", "~"),
            "~/Library/Caches/YtFast"
        );
    }
}
