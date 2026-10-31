//! What a song's, album's, playlist's or artist's menu offers beyond what
//! YTFast does by itself: the entries that need YouTube's own token for
//! them (`feedbackEndpoint`: pin to Listen again, a song in the library,
//! Remove from history, Not interested), and those that point where the
//! row does not (its song's credits, an album's artist). Read with the row
//! or card, from its `menu.menuRenderer.items`, as YouTube Music shows
//! them (measured on the owner's account, 31 October 2026). And a song's
//! credits ("View song credits").

use serde_json::Value;

use super::{find_key, text};

/// An entry that says how things stand and changes them: whether it is
/// on now (pinned, in the library), and the tokens that turn it on and
/// off (`toggleMenuServiceItemRenderer`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toggle {
    pub on: bool,
    turn_on: String,
    turn_off: String,
}

impl Toggle {
    pub fn new(on: bool, turn_on: impl Into<String>, turn_off: impl Into<String>) -> Self {
        Self {
            on,
            turn_on: turn_on.into(),
            turn_off: turn_off.into(),
        }
    }

    /// The token that makes it `on`.
    pub fn token(&self, on: bool) -> &str {
        if on { &self.turn_on } else { &self.turn_off }
    }
}

/// An entry that takes something away (from History, from Home's
/// suggestions): its token, and what YouTube says then, when it says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feedback {
    pub token: String,
    pub said: Option<String>,
}

/// What a row's or card's menu offers that YTFast cannot make up itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menu {
    /// "Pin to Listen again" or "Unpin from Listen again".
    pub pin: Option<Toggle>,
    /// A song's "Save to library" or "Remove from library".
    pub library: Option<Toggle>,
    /// History's "Remove from history".
    pub forget: Option<Feedback>,
    /// Home's "Not interested".
    pub not_interested: Option<Feedback>,
    /// Home's "Don't recommend artist".
    pub not_artist: Option<Feedback>,
    /// "View song credits": the credits' page (`MPTC...`).
    pub credits: Option<String>,
    /// "Go to artist" (`UC...`) and "Go to album" (`MPREb_...`), where a
    /// card names no page of theirs.
    pub artist: Option<String>,
    pub album: Option<String>,
    /// What the account thinks of a card's song, as its menu's like entry
    /// says (a row says it by its own buttons).
    pub like: Option<super::Rating>,
    /// An artist's "Subscribe" or "Unsubscribe": whether subscribed now.
    pub subscribed: Option<bool>,
}

/// Reads [`Menu`] from a row or card (`node` holds its `menu`); `None`
/// when its menu offers none of it.
pub fn menu_of(node: &Value) -> Option<Box<Menu>> {
    let items = node.pointer("/menu/menuRenderer/items")?.as_array()?;
    let mut menu = Menu::default();
    let icon = |entry: &Value, key: &str| {
        entry
            .pointer(&format!("/{key}/iconType"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    for item in items {
        if let Some(entry) = item.get("toggleMenuServiceItemRenderer") {
            let token = |key: &str| {
                entry
                    .pointer(&format!("/{key}/feedbackEndpoint/feedbackToken"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let words = entry.get("defaultText").and_then(text).unwrap_or_default();
            let subscribe = entry.pointer("/defaultServiceEndpoint/subscribeEndpoint");
            let unsubscribe = entry.pointer("/defaultServiceEndpoint/unsubscribeEndpoint");
            if subscribe.is_some() || unsubscribe.is_some() {
                menu.subscribed = Some(unsubscribe.is_some());
                continue;
            }
            let (Some(default), Some(toggled)) = (
                token("defaultServiceEndpoint"),
                token("toggledServiceEndpoint"),
            ) else {
                continue;
            };
            // Shown as "Unpin..." or "Remove...": on now, its own token
            // turns it off.
            let toggle = |on: bool| {
                let (turn_on, turn_off) = if on {
                    (toggled.clone(), default.clone())
                } else {
                    (default.clone(), toggled.clone())
                };
                Toggle {
                    on,
                    turn_on,
                    turn_off,
                }
            };
            match icon(entry, "defaultIcon").as_str() {
                "KEEP" => menu.pin = Some(toggle(false)),
                "KEEP_OFF" => menu.pin = Some(toggle(true)),
                _ if words.contains("library") => {
                    menu.library = Some(toggle(words.starts_with("Remove")));
                }
                _ => {}
            }
        } else if let Some(entry) = item.get("menuServiceItemRenderer") {
            let Some(feedback) = entry.pointer("/serviceEndpoint/feedbackEndpoint") else {
                continue;
            };
            let Some(token) = feedback.get("feedbackToken").and_then(Value::as_str) else {
                continue;
            };
            let found = Some(Feedback {
                token: token.to_string(),
                said: feedback
                    .get("actions")
                    .and_then(|a| find_key(a, "responseText"))
                    .and_then(text)
                    .filter(|t| !t.trim().is_empty()),
            });
            match icon(entry, "icon").as_str() {
                "REMOVE_FROM_HISTORY" => menu.forget = found,
                "NOT_INTERESTED" => menu.not_interested = found,
                "REMOVE_CIRCLE" => menu.not_artist = found,
                _ => {}
            }
        } else if let Some(entry) = item.get("menuNavigationItemRenderer") {
            let page = entry
                .pointer("/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
                .map(str::to_string);
            match icon(entry, "icon").as_str() {
                "PEOPLE_GROUP" => menu.credits = page,
                "ARTIST" => menu.artist = page,
                "ALBUM" => menu.album = page,
                _ => {}
            }
        }
    }
    menu.like = node.get("menu").and_then(super::song::menu_rating);
    (menu != Menu::default()).then(|| Box::new(menu))
}

/// A song's credits, as "View song credits" shows them: the song, its
/// artists, and who did what ("Performed by", "Written by"...), each
/// role with its people.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credits {
    pub title: String,
    pub artists: String,
    pub roles: Vec<(String, Vec<String>)>,
}

/// Reads a song's credits from their page (a dialog YouTube opens:
/// `dismissableDialogRenderer`); `None` when the reply has none.
pub fn credits(reply: &Value) -> Option<Credits> {
    let dialog = find_key(reply, "dismissableDialogRenderer")?;
    let song = dialog.pointer("/metadata/musicMultiRowListItemRenderer");
    let roles: Vec<(String, Vec<String>)> = dialog
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|section| {
            let section = section.get("dismissableDialogContentSectionRenderer")?;
            let role = section.get("title").and_then(text)?;
            let people: Vec<String> = section
                .get("subtitle")
                .and_then(text)
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            (!people.is_empty()).then_some((role, people))
        })
        .collect();
    if roles.is_empty() {
        return None;
    }
    Some(Credits {
        title: song
            .and_then(|s| s.get("title"))
            .and_then(text)
            .unwrap_or_default(),
        artists: song
            .and_then(|s| s.get("subtitle"))
            .and_then(text)
            .unwrap_or_default(),
        roles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A song row's menu as YouTube Music's (its order, icons and
    /// endpoints as read on the owner's account; the tokens made up).
    fn song_menu(pinned: bool) -> Value {
        let (pin_text, pin_icon) = if pinned {
            ("Unpin from Listen again", "KEEP_OFF")
        } else {
            ("Pin to Listen again", "KEEP")
        };
        json!({"menu": {"menuRenderer": {"items": [
            {"menuNavigationItemRenderer": {"text": {"runs": [{"text": "Start mix"}]},
                "icon": {"iconType": "MIX"},
                "navigationEndpoint": {"watchEndpoint": {"videoId": "v"}}}},
            {"toggleMenuServiceItemRenderer": {
                "defaultText": {"runs": [{"text": "Save to library"}]},
                "defaultIcon": {"iconType": "BOOKMARK_BORDER"},
                "defaultServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": "save"}},
                "toggledText": {"runs": [{"text": "Remove from library"}]},
                "toggledServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": "unsave"}}}},
            {"toggleMenuServiceItemRenderer": {
                "defaultText": {"runs": [{"text": "Remove from liked songs"}]},
                "defaultIcon": {"iconType": "UNFAVORITE"},
                "defaultServiceEndpoint": {"likeEndpoint": {"status": "INDIFFERENT"}},
                "toggledServiceEndpoint": {"likeEndpoint": {"status": "LIKE"}}}},
            {"menuNavigationItemRenderer": {"text": {"runs": [{"text": "Go to artist"}]},
                "icon": {"iconType": "ARTIST"},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "UCartist"}}}},
            {"menuNavigationItemRenderer": {"text": {"runs": [{"text": "View song credits"}]},
                "icon": {"iconType": "PEOPLE_GROUP"},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "MPTCcredits"}}}},
            {"menuServiceItemRenderer": {"text": {"runs": [{"text": "Remove from history"}]},
                "icon": {"iconType": "REMOVE_FROM_HISTORY"},
                "serviceEndpoint": {"feedbackEndpoint": {"feedbackToken": "forget",
                    "actions": [{"replaceEnclosingAction": {"item": {"notificationMultiActionRenderer": {
                        "responseText": {"runs": [{"text": "Removed from history"}]}}}}}]}}}},
            {"toggleMenuServiceItemRenderer": {
                "defaultText": {"runs": [{"text": pin_text}]},
                "defaultIcon": {"iconType": pin_icon},
                "defaultServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": "pin-default"}},
                "toggledServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": "pin-toggled"}}}},
            {"menuServiceItemRenderer": {"text": {"runs": [{"text": "Not interested"}]},
                "icon": {"iconType": "NOT_INTERESTED"},
                "serviceEndpoint": {"feedbackEndpoint": {"feedbackToken": "nope"}}}},
            {"menuServiceItemRenderer": {"text": {"runs": [{"text": "Don't recommend artist"}]},
                "icon": {"iconType": "REMOVE_CIRCLE"},
                "serviceEndpoint": {"feedbackEndpoint": {"feedbackToken": "not-them"}}}}
        ]}}})
    }

    #[test]
    fn a_menus_tokens_are_read() {
        let menu = menu_of(&song_menu(false)).unwrap();
        let pin = menu.pin.as_ref().unwrap();
        assert!(!pin.on);
        assert_eq!(pin.token(true), "pin-default");
        assert_eq!(pin.token(false), "pin-toggled");
        let library = menu.library.as_ref().unwrap();
        assert!(!library.on);
        assert_eq!(
            (library.token(true), library.token(false)),
            ("save", "unsave")
        );
        assert_eq!(menu.credits.as_deref(), Some("MPTCcredits"));
        assert_eq!(menu.artist.as_deref(), Some("UCartist"));
        let forget = menu.forget.as_ref().unwrap();
        assert_eq!(forget.token, "forget");
        assert_eq!(forget.said.as_deref(), Some("Removed from history"));
        assert_eq!(menu.not_interested.as_ref().unwrap().token, "nope");
        assert_eq!(menu.not_interested.as_ref().unwrap().said, None);
        assert_eq!(menu.not_artist.as_ref().unwrap().token, "not-them");
        assert_eq!(menu.subscribed, None);
        // Pinned already: its own entry unpins.
        let pinned = menu_of(&song_menu(true)).unwrap();
        let pin = pinned.pin.as_ref().unwrap();
        assert!(pin.on);
        assert_eq!(
            (pin.token(false), pin.token(true)),
            ("pin-default", "pin-toggled")
        );
        // Nothing of it: none.
        assert_eq!(
            menu_of(&json!({"menu": {"menuRenderer": {"items": []}}})),
            None
        );
        assert_eq!(menu_of(&json!({})), None);
    }

    #[test]
    fn an_artists_subscription_is_read() {
        let artist = |endpoint: &str| {
            json!({"menu": {"menuRenderer": {"items": [
                {"toggleMenuServiceItemRenderer": {
                    "defaultText": {"runs": [{"text": "Subscribe"}]},
                    "defaultIcon": {"iconType": "SUBSCRIBE"},
                    "defaultServiceEndpoint": {endpoint: {"channelIds": ["UCa"]}},
                    "toggledServiceEndpoint": {}}}
            ]}}})
        };
        assert_eq!(
            menu_of(&artist("subscribeEndpoint")).unwrap().subscribed,
            Some(false)
        );
        assert_eq!(
            menu_of(&artist("unsubscribeEndpoint")).unwrap().subscribed,
            Some(true)
        );
    }

    #[test]
    fn a_songs_credits_are_read() {
        let section = |role: &str, people: &[&str]| {
            let runs: Vec<Value> = people
                .iter()
                .enumerate()
                .flat_map(|(i, p)| {
                    let mut runs = vec![json!({"text": p})];
                    if i + 1 < people.len() {
                        runs.push(json!({"text": "\n"}));
                    }
                    runs
                })
                .collect();
            json!({"dismissableDialogContentSectionRenderer": {
                "title": {"runs": [{"text": role}]},
                "subtitle": {"runs": runs}
            }})
        };
        let reply = json!({"onResponseReceivedActions": [{"openPopupAction": {"popup": {
            "dismissableDialogRenderer": {
                "title": "Song credits",
                "sections": [
                    section("Performed by", &["Artist One"]),
                    section("Written by", &["Writer A", "Writer B", "Writer C"]),
                    section("Produced by", &["Producer"]),
                    section("Music metadata provided by", &["A Label"])
                ],
                "metadata": {"musicMultiRowListItemRenderer": {
                    "title": {"runs": [{"text": "A Song"}]},
                    "subtitle": {"runs": [{"text": "Artist One"}]}
                }}
            }
        }}}]});
        let read = credits(&reply).unwrap();
        assert_eq!(read.title, "A Song");
        assert_eq!(read.artists, "Artist One");
        assert_eq!(read.roles.len(), 4);
        assert_eq!(
            read.roles[1],
            (
                "Written by".to_string(),
                vec!["Writer A".into(), "Writer B".into(), "Writer C".into()]
            )
        );
        assert_eq!(credits(&json!({})), None);
    }
}
