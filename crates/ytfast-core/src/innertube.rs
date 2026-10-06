//! Talking to YouTube Music's internal API: the one music.youtube.com itself
//! uses (`/youtubei/v1/...`). There is no official YouTube Music API.
//!
//! Requests carry the browser's YouTube cookies and the request signature,
//! the way the web page sends them (see `auth.rs` and `ytcfg.rs`). The
//! header set follows yt-dlp's `generate_api_headers`.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::cookies::CookieJar;
use crate::read::{self, Account, AccountFlags, PlayerInfo, Track};
use crate::ytcfg::WebConfig;
use crate::{auth, redact};

pub const ORIGIN: &str = "https://music.youtube.com";
const HOST: &str = "music.youtube.com";
const CLIENT_NAME: &str = "WEB_REMIX";
/// `WEB_REMIX` as a number, for the `X-YouTube-Client-Name` header.
const CLIENT_NAME_ID: &str = "67";
/// Only used when the page gives no version (yt-dlp's value, July 2026).
const FALLBACK_CLIENT_VERSION: &str = "1.20260707.12.00";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("YouTube treated the request as signed out")]
    SignedOut,
    #[error("YouTube answered with HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("could not reach YouTube: {0}")]
    Network(String),
    #[error("YouTube sent something unexpected: {0}")]
    Unexpected(String),
}

fn network(error: reqwest::Error) -> ApiError {
    ApiError::Network(redact::urls(&error.to_string()))
}

/// One page of a list (Liked songs, History).
#[derive(Clone, Debug)]
pub struct Page {
    pub tracks: Vec<Track>,
    /// More rows than this page holds.
    pub more: bool,
    pub flags: AccountFlags,
}

/// A signed-in YouTube Music session: the cookies and the page config.
pub struct Session {
    http: reqwest::Client,
    cookies: CookieJar,
    config: WebConfig,
}

impl Session {
    /// Loads the YouTube Music page with the cookies, to learn which account
    /// is active and what the web client currently sends. `http` should be
    /// [`crate::net::api_client`].
    pub async fn start(http: reqwest::Client, cookies: &CookieJar) -> Result<Self, ApiError> {
        let cookies = cookies.youtube_only().with_consent();
        let mut request = http
            .get(format!("{ORIGIN}/"))
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Accept", "text/html,application/xhtml+xml");
        if let Some(header) = cookies.header_for(HOST) {
            request = request.header("Cookie", header);
        }
        let response = request.send().await.map_err(network)?;
        let status = response.status();
        let html = response.text().await.map_err(network)?;
        if !status.is_success() {
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: "the YouTube Music page did not load".into(),
            });
        }
        let config = WebConfig::from_html(&html);
        Ok(Self {
            http,
            cookies,
            config,
        })
    }

    pub fn config(&self) -> &WebConfig {
        &self.config
    }

    pub fn cookies(&self) -> &CookieJar {
        &self.cookies
    }

    fn context(&self) -> Value {
        let mut context = self
            .config
            .innertube_context
            .clone()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "client": {}, "user": {} }));
        if !context.get("client").is_some_and(Value::is_object) {
            context["client"] = json!({});
        }
        let version = self
            .config
            .client_version
            .clone()
            .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_string());
        let client = &mut context["client"];
        client["clientName"] = json!(CLIENT_NAME);
        client["clientVersion"] = json!(version);
        if client.get("hl").is_none() {
            client["hl"] = json!("en");
        }
        context
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let mut put = |name: &'static str, value: &str| {
            if let Ok(value) = HeaderValue::from_str(value) {
                headers.insert(HeaderName::from_static(name), value);
            }
        };
        let version = self
            .config
            .client_version
            .as_deref()
            .unwrap_or(FALLBACK_CLIENT_VERSION);
        put("content-type", "application/json");
        put("origin", ORIGIN);
        put("x-origin", ORIGIN);
        put("referer", "https://music.youtube.com/");
        put("x-youtube-client-name", CLIENT_NAME_ID);
        put("x-youtube-client-version", version);
        put(
            "x-goog-authuser",
            &self.config.session_index.unwrap_or(0).to_string(),
        );
        if let Some(page_id) = &self.config.delegated_session_id {
            put("x-goog-pageid", page_id);
        }
        if let Some(visitor) = &self.config.visitor_data {
            put("x-goog-visitor-id", visitor);
        }
        if self.config.logged_in == Some(true) {
            put("x-youtube-bootstrap-logged-in", "true");
        }
        let sids = self.cookies.sid_cookies(HOST);
        if let Some(signature) = auth::authorization(
            &sids,
            ORIGIN,
            self.config.user_session_id.as_deref(),
            unix_now(),
        ) {
            put("authorization", &signature);
        }
        if let Some(cookie) = self.cookies.header_for(HOST) {
            put("cookie", &cookie);
        }
        headers
    }

    /// Calls one endpoint (`browse`, `player`, `account/account_menu`...).
    pub async fn call(&self, endpoint: &str, mut body: Value) -> Result<Value, ApiError> {
        if !body.is_object() {
            body = json!({});
        }
        body["context"] = self.context();
        let mut url = format!("{ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false");
        if let Some(key) = &self.config.api_key {
            url.push_str("&key=");
            url.push_str(key);
        }
        let response = self
            .http
            .post(url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(network)?;
        let status = response.status();
        let text = response.text().await.map_err(network)?;
        if status.as_u16() == 401 {
            return Err(ApiError::SignedOut);
        }
        if !status.is_success() {
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "no details".into());
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact::urls(&message),
            });
        }
        serde_json::from_str(&text)
            .map_err(|e| ApiError::Unexpected(format!("the reply was not JSON ({e})")))
    }

    /// The signed-in account's name, and the account flags.
    pub async fn account(&self) -> Result<(Option<Account>, AccountFlags), ApiError> {
        let reply = self.call("account/account_menu", json!({})).await?;
        Ok((read::account(&reply), read::account_flags(&reply)))
    }

    /// The raw reply for a browse page (`FEmusic_home`, `VLLM`...).
    pub async fn browse(&self, browse_id: &str) -> Result<Value, ApiError> {
        self.call("browse", json!({ "browseId": browse_id })).await
    }

    async fn page(&self, browse_id: &str) -> Result<Page, ApiError> {
        let reply = self.browse(browse_id).await?;
        Ok(Page {
            tracks: read::tracks(&reply),
            more: read::track_continuation(&reply).is_some(),
            flags: read::account_flags(&reply),
        })
    }

    /// The first page of Liked songs (the `LM` playlist).
    pub async fn liked_songs(&self) -> Result<Page, ApiError> {
        self.page("VLLM").await
    }

    /// The first page of listening History, newest first.
    pub async fn history(&self) -> Result<Page, ApiError> {
        self.page("FEmusic_history").await
    }

    /// What YouTube says about one song: whether it plays for this account,
    /// its loudness, and where to report listening.
    pub async fn player(&self, video_id: &str) -> Result<PlayerInfo, ApiError> {
        // The signature timestamp only matters for stream addresses, which
        // YtFast takes from yt-dlp; ytmusicapi sends the same placeholder.
        let days = unix_now() / 86_400;
        let reply = self
            .call(
                "player",
                json!({
                    "videoId": video_id,
                    "playbackContext": {
                        "contentPlaybackContext": { "signatureTimestamp": days.saturating_sub(1) }
                    },
                    "contentCheckOk": true,
                    "racyCheckOk": true,
                }),
            )
            .await?;
        Ok(read::player_info(&reply))
    }

    /// Sends one listening report (see `playreport.rs`). Returns the HTTP
    /// status; YouTube answers 204 when it accepted the report.
    pub async fn send_report(&self, url: &str) -> Result<u16, ApiError> {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
            return Err(ApiError::Unexpected(
                "a report address outside youtube.com".into(),
            ));
        }
        let mut request = self
            .http
            .get(url)
            .header("Referer", "https://music.youtube.com/");
        if let Some(cookie) = self.cookies.header_for(&host) {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await.map_err(network)?;
        Ok(response.status().as_u16())
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(config: WebConfig) -> Session {
        let cookies = CookieJar::parse_netscape(
            ".youtube.com\tTRUE\t/\tTRUE\t4102444800\tSAPISID\tsap\n\
             #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t4102444800\tLOGIN_INFO\tlogin\n",
        );
        Session {
            http: reqwest::Client::new(),
            cookies,
            config,
        }
    }

    #[test]
    fn headers_follow_the_page_config() {
        let s = session(WebConfig {
            client_version: Some("1.20261001.01.00".into()),
            session_index: Some(2),
            delegated_session_id: Some("brand".into()),
            visitor_data: Some("visitor".into()),
            logged_in: Some(true),
            user_session_id: Some("user".into()),
            ..WebConfig::default()
        });
        let h = s.headers();
        assert_eq!(h["x-goog-authuser"], "2");
        assert_eq!(h["x-goog-pageid"], "brand");
        assert_eq!(h["x-goog-visitor-id"], "visitor");
        assert_eq!(h["x-youtube-client-version"], "1.20261001.01.00");
        assert_eq!(h["x-youtube-bootstrap-logged-in"], "true");
        let auth = h["authorization"].to_str().unwrap();
        assert!(auth.starts_with("SAPISIDHASH ") && auth.ends_with("_u"));
        assert!(h["cookie"].to_str().unwrap().contains("LOGIN_INFO=login"));
    }

    #[test]
    fn defaults_without_a_page_config() {
        let s = session(WebConfig::default());
        let h = s.headers();
        assert_eq!(h["x-goog-authuser"], "0");
        assert!(h.get("x-goog-pageid").is_none());
        let context = s.context();
        assert_eq!(context["client"]["clientName"], "WEB_REMIX");
        assert_eq!(context["client"]["clientVersion"], FALLBACK_CLIENT_VERSION);
        assert_eq!(context["client"]["hl"], "en");
    }

    #[test]
    fn context_keeps_the_page_context() {
        let s = session(WebConfig {
            client_version: Some("1.2".into()),
            innertube_context: Some(json!({
                "client": { "clientName": "WEB_REMIX", "hl": "de", "gl": "AU", "visitorData": "v" },
                "user": { "lockedSafetyMode": false }
            })),
            ..WebConfig::default()
        });
        let context = s.context();
        assert_eq!(context["client"]["hl"], "de");
        assert_eq!(context["client"]["gl"], "AU");
        assert_eq!(context["client"]["clientVersion"], "1.2");
        assert_eq!(context["user"]["lockedSafetyMode"], false);
    }
}
