//! The audio formats in a `player` reply (`streamingData.adaptiveFormats`),
//! for playing a song without yt-dlp.

use serde_json::Value;

/// One audio format YouTube offers for a song.
#[derive(Clone, PartialEq, Eq)]
pub struct StreamFormat {
    /// YouTube's number for the format (`140` is AAC 128 kbps, `141` AAC
    /// 256 kbps for Premium).
    pub itag: u32,
    /// "audio/mp4; codecs=\"mp4a.40.2\"".
    pub mime: String,
    /// Bits per second.
    pub bitrate: u64,
    pub content_length: Option<u64>,
    pub duration_ms: Option<u64>,
    /// The stream address, when YouTube gives it plainly. Never shown.
    pub url: Option<String>,
    /// Otherwise the address with a scrambled signature (`s`, `sp`, `url`).
    pub signature_cipher: Option<String>,
    /// Dynamic range compressed: a quieter, flattened copy. Avoided.
    pub drc: bool,
    pub drm: bool,
}

impl std::fmt::Debug for StreamFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamFormat")
            .field("itag", &self.itag)
            .field("mime", &self.mime)
            .field("bitrate", &self.bitrate)
            .field("ciphered", &self.signature_cipher.is_some())
            .finish()
    }
}

impl StreamFormat {
    /// AAC in MP4, which YtFast's decoder plays.
    pub fn is_aac(&self) -> bool {
        self.mime.starts_with("audio/mp4")
    }

    /// Premium-only quality (256 kbps).
    pub fn is_premium(&self) -> bool {
        matches!(self.itag, 141 | 774)
    }

    /// For people: "AAC 256 kbps (format 141)".
    pub fn describe(&self) -> String {
        format!(
            "AAC {} kbps (format {})",
            (self.bitrate as f64 / 1000.0).round(),
            self.itag
        )
    }
}

/// Every audio format in a `player` reply.
pub fn stream_formats(reply: &Value) -> Vec<StreamFormat> {
    let formats = reply
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array);
    formats
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let mime = f.get("mimeType")?.as_str()?.to_string();
            if !mime.starts_with("audio/") {
                return None;
            }
            let number = |key: &str| {
                f.get(key).and_then(|v| match v {
                    Value::Number(n) => n.as_u64(),
                    Value::String(s) => s.parse().ok(),
                    _ => None,
                })
            };
            let text = |key: &str| f.get(key).and_then(Value::as_str).map(str::to_string);
            Some(StreamFormat {
                itag: u32::try_from(number("itag")?).ok()?,
                bitrate: number("bitrate").unwrap_or(0),
                content_length: number("contentLength"),
                duration_ms: number("approxDurationMs"),
                url: text("url"),
                signature_cipher: text("signatureCipher").or_else(|| text("cipher")),
                drc: f.get("isDrc").and_then(Value::as_bool).unwrap_or(false)
                    || f.get("xtags")
                        .and_then(Value::as_str)
                        .is_some_and(|x| x.contains("drc")),
                drm: f.get("drmFamilies").is_some(),
                mime,
            })
        })
        .collect()
}

/// The format to play: AAC (what the decoder plays), not quieted (DRC),
/// not locked (DRM); Premium's 256 kbps first, then the highest bitrate.
pub fn best_stream(formats: &[StreamFormat]) -> Option<&StreamFormat> {
    formats
        .iter()
        .filter(|f| f.is_aac() && !f.drc && !f.drm)
        .filter(|f| f.url.is_some() || f.signature_cipher.is_some())
        .max_by_key(|f| (f.is_premium(), f.bitrate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_premium_aac() {
        let reply = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000,
             "url": "https://r1.googlevideo.example/a", "contentLength": "3000000"},
            {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000,
             "signatureCipher": "s=abc&sp=sig&url=https%3A%2F%2Fr1.googlevideo.example%2Fb",
             "contentLength": "6000000", "approxDurationMs": "180000"},
            {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000,
             "url": "https://r1.googlevideo.example/c", "isDrc": true},
            {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 150000,
             "url": "https://r1.googlevideo.example/d"},
            {"itag": 137, "mimeType": "video/mp4", "bitrate": 4000000, "url": "https://x.example/v"}
        ]}});
        let formats = stream_formats(&reply);
        assert_eq!(formats.len(), 4);
        let best = best_stream(&formats).unwrap();
        assert_eq!(best.itag, 141);
        assert!(!best.drc);
        assert!(best.signature_cipher.is_some());
        assert_eq!(best.content_length, Some(6_000_000));
        assert_eq!(best.duration_ms, Some(180_000));
        assert_eq!(best.describe(), "AAC 260 kbps (format 141)");
        assert!(!format!("{best:?}").contains("googlevideo"));
    }

    #[test]
    fn standard_accounts_get_128() {
        let reply = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4", "bitrate": 130000, "url": "https://a.example/a"},
            {"itag": 251, "mimeType": "audio/webm", "bitrate": 150000, "url": "https://a.example/b"}
        ]}});
        assert_eq!(best_stream(&stream_formats(&reply)).unwrap().itag, 140);
        assert!(best_stream(&stream_formats(&json!({}))).is_none());
    }
}
