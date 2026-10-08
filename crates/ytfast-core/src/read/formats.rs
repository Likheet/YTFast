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
    /// How many channels: 2 for stereo, 6 for surround. `None` when
    /// YouTube did not say.
    pub channels: Option<u64>,
    /// The video's own language. A video with dubbed versions (in other
    /// languages) lists each format once per language; one with a single
    /// language counts as its own.
    pub original_language: bool,
    /// The language YouTube plays unless asked otherwise.
    pub default_language: bool,
}

impl std::fmt::Debug for StreamFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamFormat")
            .field("itag", &self.itag)
            .field("mime", &self.mime)
            .field("bitrate", &self.bitrate)
            .field("ciphered", &self.signature_cipher.is_some())
            .field("original_language", &self.original_language)
            .finish()
    }
}

impl StreamFormat {
    /// Stereo AAC (AAC-LC, `mp4a.40.2`) in MP4, which YtFast's decoder
    /// plays. YouTube also puts other sound in MP4 for some videos:
    /// surround (E-AC-3, AC-3, 5.1 AAC) and HE-AAC.
    pub fn is_aac(&self) -> bool {
        if !self.mime.starts_with("audio/mp4") || self.channels.is_some_and(|c| c > 2) {
            return false;
        }
        match self.mime.split_once("codecs=") {
            Some((_, codecs)) => {
                codecs.trim_matches(|c: char| c == '"' || c.is_whitespace()) == "mp4a.40.2"
            }
            // No codec named: only the formats known to be stereo AAC.
            None => matches!(self.itag, 140 | 141),
        }
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
            // Which language this copy is in, for videos with several.
            let track = f.get("audioTrack");
            let original_language = track.is_none_or(|t| {
                t.get("displayName")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.to_lowercase().contains("original"))
            });
            let default_language = track
                .is_none_or(|t| t.get("audioIsDefault").and_then(Value::as_bool) == Some(true));
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
                channels: number("audioChannels"),
                original_language,
                default_language,
                mime,
            })
        })
        .collect()
}

/// The format to play: stereo AAC (what the decoder plays), not quieted
/// (DRC), not locked (DRM); in the video's own language first (as yt-dlp
/// chooses), else YouTube's default one; then Premium's 256 kbps, then
/// the highest bitrate.
pub fn best_stream(formats: &[StreamFormat]) -> Option<&StreamFormat> {
    formats
        .iter()
        .filter(|f| f.is_aac() && !f.drc && !f.drm)
        .filter(|f| f.url.is_some() || f.signature_cipher.is_some())
        .max_by_key(|f| {
            (
                f.original_language,
                f.default_language,
                f.is_premium(),
                f.bitrate,
            )
        })
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

    #[test]
    fn surround_and_other_codecs_in_mp4_are_not_aac() {
        // A music video with surround sound, as YouTube lists it: more
        // bits than 140, but not stereo AAC, which the decoder needs.
        let reply = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000,
             "audioChannels": 2, "url": "https://a.example/140"},
            {"itag": 328, "mimeType": "audio/mp4; codecs=\"ec-3\"", "bitrate": 384000,
             "audioChannels": 6, "url": "https://a.example/328"},
            {"itag": 380, "mimeType": "audio/mp4; codecs=\"ac-3\"", "bitrate": 384000,
             "url": "https://a.example/380"},
            {"itag": 256, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 192000,
             "audioChannels": 6, "url": "https://a.example/256"},
            {"itag": 258, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 384000,
             "audioChannels": 6, "url": "https://a.example/258"},
            {"itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 49000,
             "audioChannels": 2, "url": "https://a.example/139"}
        ]}});
        let formats = stream_formats(&reply);
        let aac: Vec<u32> = formats
            .iter()
            .filter(|f| f.is_aac())
            .map(|f| f.itag)
            .collect();
        assert_eq!(aac, [140]);
        assert_eq!(best_stream(&formats).unwrap().itag, 140);
        // Without a codec named, only the formats known to be stereo AAC.
        let bare = |itag: u32| StreamFormat {
            itag,
            mime: "audio/mp4".into(),
            bitrate: 0,
            content_length: None,
            duration_ms: None,
            url: None,
            signature_cipher: None,
            drc: false,
            drm: false,
            channels: None,
            original_language: true,
            default_language: true,
        };
        assert!(bare(141).is_aac());
        assert!(!bare(328).is_aac());
    }

    #[test]
    fn the_original_language_plays() {
        // A video dubbed into other languages: one 140 per language, told
        // apart by `audioTrack`. The dub has the higher bitrate.
        let reply = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 131000,
             "url": "https://a.example/es",
             "audioTrack": {"displayName": "Spanish (Spain)", "id": "es.3", "audioIsDefault": false}},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 129000,
             "url": "https://a.example/en",
             "audioTrack": {"displayName": "English (United States) original", "id": "en-US.4", "audioIsDefault": true}},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000,
             "url": "https://a.example/fr",
             "audioTrack": {"displayName": "French (France)", "id": "fr.3", "audioIsDefault": false}}
        ]}});
        let formats = stream_formats(&reply);
        assert_eq!(
            best_stream(&formats).unwrap().url.as_deref(),
            Some("https://a.example/en")
        );
        // None marked original: YouTube's default language.
        let reply = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 131000,
             "url": "https://a.example/de", "audioTrack": {"displayName": "German", "audioIsDefault": false}},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 129000,
             "url": "https://a.example/ja", "audioTrack": {"displayName": "Japanese", "audioIsDefault": true}}
        ]}});
        let formats = stream_formats(&reply);
        assert_eq!(
            best_stream(&formats).unwrap().url.as_deref(),
            Some("https://a.example/ja")
        );
        // One language only: its own.
        let single = stream_formats(&json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "url": "https://a.example/a"}
        ]}}));
        assert!(single[0].original_language && single[0].default_language);
    }
}
