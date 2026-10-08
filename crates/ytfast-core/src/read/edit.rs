//! What YouTube answers to changes in the account (playlist edits, a
//! past search removed).

use serde_json::Value;

/// How a playlist edit or deletion went: `STATUS_SUCCEEDED` when it
/// worked.
pub fn edit_status(reply: &Value) -> Option<String> {
    reply
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Whether a `feedback` request (a past search removed) was carried out:
/// every `feedbackResponses[].isProcessed`. `None` when the reply does not
/// say.
pub fn feedback_processed(reply: &Value) -> Option<bool> {
    let responses = reply.get("feedbackResponses")?.as_array()?;
    Some(
        !responses.is_empty()
            && responses
                .iter()
                .all(|r| r.get("isProcessed").and_then(Value::as_bool) == Some(true)),
    )
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
        let processed = |done: bool| json!({"feedbackResponses": [{"isProcessed": done}]});
        assert_eq!(feedback_processed(&processed(true)), Some(true));
        assert_eq!(feedback_processed(&processed(false)), Some(false));
        assert_eq!(feedback_processed(&json!({})), None);
    }
}
