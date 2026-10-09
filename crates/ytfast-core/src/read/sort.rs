//! A page's sort button: the Library's "Recent activity", "A to Z" and so
//! on (`musicSortFilterButtonRenderer`).

use serde_json::Value;

use super::{find_key, text};

/// A page's sort button: the order shown, and every order it offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SortMenu {
    /// "Recent activity", as the button says.
    pub chosen: String,
    pub orders: Vec<SortOrder>,
}

/// One order a sort button offers: its words, and the page in that order
/// (a browse request: its page and `params`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SortOrder {
    pub title: String,
    pub browse_id: String,
    pub params: String,
}

/// The page's sort button, when it has one with orders that can be read.
pub(super) fn sort_menu(reply: &Value) -> Option<SortMenu> {
    let button = find_key(reply, "musicSortFilterButtonRenderer")?;
    let chosen = button.get("title").and_then(text)?;
    let orders: Vec<SortOrder> = button
        .pointer("/menu/musicMultiSelectMenuRenderer/options")?
        .as_array()?
        .iter()
        .filter_map(|option| {
            let item = option.get("musicMultiSelectMenuItemRenderer")?;
            let title = item.get("title").and_then(text)?;
            let token = find_key(item, "reloadContinuationData")?
                .get("continuation")?
                .as_str()?;
            let (browse_id, params) = reloaded_browse(token)?;
            Some(SortOrder {
                title,
                browse_id,
                params,
            })
        })
        .collect();
    (!orders.is_empty()).then_some(SortMenu { chosen, orders })
}

/// The page a reload token asks for. Such a token is a browse request in
/// YouTube's binary form (protobuf), base64 encoded: one field holding the
/// page's ID (field 2) and its `params` (field 3). `None` for any other.
fn reloaded_browse(token: &str) -> Option<(String, String)> {
    let bytes = base64(&token.replace("%3D", "="))?;
    fields(&bytes)?.into_iter().find_map(|(_, inner)| {
        let inner = fields(inner)?;
        let field = |number| {
            inner
                .iter()
                .find(|(n, _)| *n == number)
                .and_then(|(_, value)| std::str::from_utf8(value).ok())
                .map(str::to_string)
        };
        Some((field(2)?, field(3)?))
    })
}

/// The length-delimited fields of a protobuf message, with their numbers;
/// `None` when the bytes are not one.
fn fields(mut bytes: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        match key & 7 {
            0 => {
                varint(&mut bytes)?;
            }
            1 => bytes = bytes.get(8..)?,
            2 => {
                let length = usize::try_from(varint(&mut bytes)?).ok()?;
                let value = bytes.get(..length)?;
                out.push((key >> 3, value));
                bytes = &bytes[length..];
            }
            5 => bytes = bytes.get(4..)?,
            _ => return None,
        }
    }
    Some(out)
}

/// A protobuf number (7 bits a byte, least significant first).
fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Base64, in either alphabet (`+/` or `-_`), padding optional.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The Library's front page's button, as YouTube Music sends it (the
    /// three orders' tokens are real; nothing in them is the account's).
    fn landing_button() -> Value {
        let option = |title: &str, token: &str| {
            json!({"musicMultiSelectMenuItemRenderer": {
                "title": {"runs": [{"text": title}]},
                "selectedCommand": {"commandExecutorCommand": {"commands": [
                    {"musicCheckboxFormItemMutatedCommand": {"newCheckedState": true}},
                    {"browseSectionListReloadEndpoint": {"continuation": {
                        "reloadContinuationData": {"continuation": token, "showSpinnerOverlay": true}
                    }}}
                ]}},
                "selectedIcon": {"iconType": "CHECK"}
            }})
        };
        json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {
            "sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [
                {"musicSideAlignedItemRenderer": {"endItems": [{"musicSortFilterButtonRenderer": {
                    "title": {"runs": [{"text": "Recent activity"}]},
                    "icon": {"iconType": "CHEVRON_DOWN"},
                    "menu": {"musicMultiSelectMenuRenderer": {
                        "title": {"musicMenuTitleRenderer": {"primaryText": {"runs": [{"text": "Sort by"}]}}},
                        "options": [
                            option("Recent activity", "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCaEFCb0FZQg%3D%3D"),
                            option("Recently saved", "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlBQkFCb0FZQg%3D%3D"),
                            option("Recently played", "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCUkFCb0FZQg%3D%3D")
                        ]
                    }}
                }}]}}
            ]}}]}
        }}}]}}})
    }

    #[test]
    fn the_librarys_orders() {
        let menu = sort_menu(&landing_button()).expect("a sort button");
        assert_eq!(menu.chosen, "Recent activity");
        let orders: Vec<(&str, &str, &str)> = menu
            .orders
            .iter()
            .map(|o| (o.title.as_str(), o.browse_id.as_str(), o.params.as_str()))
            .collect();
        assert_eq!(
            orders,
            [
                (
                    "Recent activity",
                    "FEmusic_library_landing",
                    "ggMGKgQIBhABoAYB"
                ),
                (
                    "Recently saved",
                    "FEmusic_library_landing",
                    "ggMGKgQIABABoAYB"
                ),
                (
                    "Recently played",
                    "FEmusic_library_landing",
                    "ggMGKgQIBRABoAYB"
                ),
            ]
        );
        assert_eq!(sort_menu(&json!({})), None);
    }

    #[test]
    fn a_reload_token_is_a_page_and_its_params() {
        // Library > Songs, A to Z.
        assert_eq!(
            reloaded_browse("4qmFsgIoEhRGRW11c2ljX2xpa2VkX3ZpZGVvcxoQZ2dNR0tnUUlBUkFBb0FZQg%3D%3D"),
            Some((
                "FEmusic_liked_videos".to_string(),
                "ggMGKgQIARAAoAYB".to_string()
            ))
        );
        // Not a token of this kind.
        assert_eq!(reloaded_browse("not base64!"), None);
        assert_eq!(reloaded_browse("AAAA"), None);
    }
}
