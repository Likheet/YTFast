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
use crate::read::{self, Account, AccountFlags, Continuation, Page, PlayerInfo, Track};
use crate::ytcfg::WebConfig;
use crate::{auth, redact};

pub const ORIGIN: &str = "https://music.youtube.com";
const HOST: &str = "music.youtube.com";
const CLIENT_NAME: &str = "WEB_REMIX";
/// `WEB_REMIX` as a number, for the `X-YouTube-Client-Name` header.
const CLIENT_NAME_ID: &str = "67";
/// Only used when the page gives no version (yt-dlp's value, July 2026).
const FALLBACK_CLIENT_VERSION: &str = "1.20260707.12.00";
/// The songs saved in the library: a long list, like a playlist.
pub(crate) const LIBRARY_SONGS: &str = "FEmusic_liked_videos";

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

/// The first page of a song list (Liked songs, History), with the account
/// flags the reply carried.
#[derive(Clone, Debug)]
pub struct TrackPage {
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

    /// The context of another YouTube client: the page's, with the
    /// client's name and version replaced.
    fn context_as(&self, client_name: &str, client_version: &str) -> Value {
        let mut context = self.context();
        context["client"]["clientName"] = json!(client_name);
        context["client"]["clientVersion"] = json!(client_version);
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
    pub async fn call(&self, endpoint: &str, body: Value) -> Result<Value, ApiError> {
        self.call_with(endpoint, &[], body).await
    }

    /// [`Session::call`] as another YouTube client, with the same sign-in:
    /// the request says it comes from `client_name` at `client_version`
    /// (as ytmusicapi does to get timed lyrics, which YouTube sends to its
    /// Android app, `ANDROID_MUSIC`).
    pub async fn call_as(
        &self,
        endpoint: &str,
        body: Value,
        client_name: &str,
        client_version: &str,
    ) -> Result<Value, ApiError> {
        let context = self.context_as(client_name, client_version);
        self.send(endpoint, &[], body, context).await
    }

    /// [`Session::call`], with more in the address.
    async fn call_with(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
        body: Value,
    ) -> Result<Value, ApiError> {
        self.send(endpoint, query, body, self.context()).await
    }

    async fn send(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
        mut body: Value,
        context: Value,
    ) -> Result<Value, ApiError> {
        if !body.is_object() {
            body = json!({});
        }
        body["context"] = context;
        let mut url = format!("{ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false");
        if let Some(key) = &self.config.api_key {
            url.push_str("&key=");
            url.push_str(key);
        }
        let response = self
            .http
            .post(url)
            .query(query)
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

    async fn track_page(&self, browse_id: &str) -> Result<TrackPage, ApiError> {
        let reply = self.browse(browse_id).await?;
        Ok(TrackPage {
            tracks: read::tracks(&reply),
            more: read::track_continuation(&reply).is_some(),
            flags: read::account_flags(&reply),
        })
    }

    /// The first page of Liked songs (the `LM` playlist).
    pub async fn liked_songs(&self) -> Result<TrackPage, ApiError> {
        self.track_page("VLLM").await
    }

    /// The first page of listening History, newest first.
    pub async fn history(&self) -> Result<TrackPage, ApiError> {
        self.track_page("FEmusic_history").await
    }

    /// Any page, as the app draws it: Home (`FEmusic_home`), an album
    /// (`MPREb_...`), a playlist (`VL...`), an artist (`UC...`), a mood
    /// (with its `params`).
    pub async fn page(&self, browse_id: &str, params: Option<&str>) -> Result<Page, ApiError> {
        Ok(self.long_page(browse_id, params).await?.0)
    }

    /// A page, and where the rest of its songs come from when it is a long
    /// list (a playlist, Liked Music); see [`Session::more_tracks`].
    pub async fn long_page(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<(Page, Option<Continuation>), ApiError> {
        let mut body = json!({ "browseId": browse_id });
        if let Some(params) = params {
            body["params"] = json!(params);
        }
        let reply = self.call("browse", body).await?;
        let mut page = read::page(&reply);
        if browse_id.starts_with("MPRE") {
            page.fill_album_songs();
        }
        // Only lists of songs go on (playlists, the library's songs); other
        // pages' tokens load more shelves.
        let more = if browse_id.starts_with("VL") || browse_id == LIBRARY_SONGS {
            read::track_continuation(&reply)
        } else {
            None
        };
        Ok((page, more))
    }

    /// The next songs of a long list, and where the ones after them come
    /// from (`None` at the end).
    pub async fn more_tracks(
        &self,
        from: &Continuation,
    ) -> Result<(Vec<Track>, Option<Continuation>), ApiError> {
        let reply = match from {
            Continuation::Body(token) => {
                self.call("browse", json!({ "continuation": token }))
                    .await?
            }
            Continuation::Address(token) => {
                // As ytmusicapi sends it.
                let query = [("ctoken", token.as_str()), ("continuation", token)];
                self.call_with("browse", &query, json!({})).await?
            }
        };
        Ok((read::tracks(&reply), read::track_continuation(&reply)))
    }

    pub async fn home(&self) -> Result<Page, ApiError> {
        self.page("FEmusic_home", None).await
    }

    /// The playlists saved in the library.
    pub async fn library_playlists(&self) -> Result<Page, ApiError> {
        self.page("FEmusic_liked_playlists", None).await
    }

    /// Search results, grouped the way YouTube Music groups them (top
    /// result, songs, albums, artists, playlists...).
    pub async fn search(&self, query: &str) -> Result<Page, ApiError> {
        Ok(read::page(
            &self.call("search", json!({ "query": query })).await?,
        ))
    }

    /// What plays after a song: the playlist or album it came from, or a
    /// radio of similar songs when `playlist_id` is `None` (YouTube
    /// Music's own "Up next"). The body follows ytmusicapi's
    /// `get_watch_playlist`.
    pub async fn up_next(
        &self,
        video_id: &str,
        playlist_id: Option<&str>,
    ) -> Result<Vec<Track>, ApiError> {
        let body = next_body(video_id, playlist_id);
        Ok(read::up_next(&self.call("next", body).await?))
    }

    /// The songs of a playlist or album to play, by its playlist ID
    /// (`OLAK5uy_...`, `PL...`, `RDCLAK...`).
    pub async fn playlist_queue(&self, playlist_id: &str) -> Result<Vec<Track>, ApiError> {
        let body = json!({
            "enablePersistentPlaylistPanel": true,
            "isAudioOnly": true,
            "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            "playlistId": playlist_id,
        });
        Ok(read::up_next(&self.call("next", body).await?))
    }

    /// What YouTube says about one song: whether it plays for this account,
    /// its loudness, and where to report listening.
    pub async fn player(&self, video_id: &str) -> Result<PlayerInfo, ApiError> {
        Ok(read::player_info(&self.player_reply(video_id, None).await?))
    }

    /// The whole `player` reply. `sts` is the player code's signature
    /// timestamp ([`crate::direct::signature_timestamp`]); stream addresses
    /// only unlock with the player whose timestamp was sent. Without one,
    /// the placeholder ytmusicapi sends.
    pub async fn player_reply(&self, video_id: &str, sts: Option<u64>) -> Result<Value, ApiError> {
        let sts = sts.unwrap_or_else(|| (unix_now() / 86_400).saturating_sub(1));
        self.call(
            "player",
            json!({
                "videoId": video_id,
                "playbackContext": {
                    "contentPlaybackContext": {
                        "html5Preference": "HTML5_PREF_WANTS",
                        "signatureTimestamp": sts
                    }
                },
                "contentCheckOk": true,
                "racyCheckOk": true,
            }),
        )
        .await
    }

    /// Where YouTube Music's player code is, from the page.
    pub fn player_js_url(&self) -> Option<String> {
        let path = self.config.player_js_url.as_deref()?;
        Some(if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{ORIGIN}{path}")
        })
    }

    /// A public YouTube file as text (the player code). No sign-in is sent.
    pub async fn fetch_text(&self, url: &str) -> Result<String, ApiError> {
        let response = self
            .http
            .get(url)
            .header("Referer", format!("{ORIGIN}/"))
            .send()
            .await
            .map_err(network)?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(ApiError::Http {
                status,
                message: "the file could not be fetched".into(),
            });
        }
        response.text().await.map_err(network)
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

/// The body of a `next` request for a song: what plays after it (the
/// playlist or album it came from, or a radio of similar songs when
/// `playlist_id` is `None`), and what YouTube says about it. Follows
/// ytmusicapi's `get_watch_playlist`.
pub(crate) fn next_body(video_id: &str, playlist_id: Option<&str>) -> Value {
    let radio = format!("RDAMVM{video_id}");
    json!({
        "enablePersistentPlaylistPanel": true,
        "isAudioOnly": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "videoId": video_id,
        "playlistId": playlist_id.unwrap_or(&radio),
        "watchEndpointMusicSupportedConfigs": {
            "watchEndpointMusicConfig": {
                "hasPersistentPlaylistPanel": true,
                "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
            }
        }
    })
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

    #[test]
    fn another_client_keeps_the_rest_of_the_context() {
        let s = session(WebConfig {
            client_version: Some("1.2".into()),
            innertube_context: Some(json!({
                "client": { "clientName": "WEB_REMIX", "hl": "de", "visitorData": "v" },
                "user": { "lockedSafetyMode": false }
            })),
            ..WebConfig::default()
        });
        let context = s.context_as("ANDROID_MUSIC", "7.21.50");
        assert_eq!(context["client"]["clientName"], "ANDROID_MUSIC");
        assert_eq!(context["client"]["clientVersion"], "7.21.50");
        assert_eq!(context["client"]["hl"], "de");
        assert_eq!(context["client"]["visitorData"], "v");
        assert_eq!(context["user"]["lockedSafetyMode"], false);
        // The usual calls are unchanged.
        assert_eq!(s.context()["client"]["clientName"], "WEB_REMIX");
        // Without a page config too.
        let bare = session(WebConfig::default()).context_as("ANDROID_MUSIC", "7.21.50");
        assert_eq!(bare["client"]["clientName"], "ANDROID_MUSIC");
    }

    #[test]
    fn next_bodies() {
        let radio = next_body("abcdefghijk", None);
        assert_eq!(radio["videoId"], "abcdefghijk");
        assert_eq!(radio["playlistId"], "RDAMVMabcdefghijk");
        assert_eq!(radio["isAudioOnly"], true);
        assert_eq!(
            radio["watchEndpointMusicSupportedConfigs"]["watchEndpointMusicConfig"]["musicVideoType"],
            "MUSIC_VIDEO_TYPE_ATV"
        );
        let album = next_body("abcdefghijk", Some("OLAK5uy_x"));
        assert_eq!(album["playlistId"], "OLAK5uy_x");
    }
}
