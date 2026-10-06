//! What YouTube answers to changes in the account (playlist edits).

use serde_json::Value;

/// How a playlist edit or deletion went: `STATUS_SUCCEEDED` when it
/// worked.
pub fn edit_status(reply: &Value) -> Option<String> {
    reply
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The ID of a playlist just made (`playlist/create`), without `VL`.
pub fn created_playlist_id(reply: &Value) -> Option<String> {
    reply
        .get("playlistId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edit_replies() {
        assert_eq!(
            edit_status(&json!({"status": "STATUS_SUCCEEDED", "playlistEditResults": []}))
                .as_deref(),
            Some("STATUS_SUCCEEDED")
        );
        assert_eq!(edit_status(&json!({"actions": []})), None);
        assert_eq!(
            created_playlist_id(&json!({"playlistId": "PLnew"})).as_deref(),
            Some("PLnew")
        );
        assert_eq!(created_playlist_id(&json!({"playlistId": ""})), None);
        assert_eq!(created_playlist_id(&Value::Null), None);
    }
}
