//! The two HTTP clients YtFast uses.

use std::time::Duration;

/// A current desktop browser. YouTube serves its normal web client to it.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

/// For API calls: each must finish within a minute.
pub fn api_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("the HTTP client settings are valid")
}

/// For downloads (audio, helper programs): no limit on the whole transfer,
/// but a stalled connection gives up after 30 seconds without data.
pub fn download_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("the HTTP client settings are valid")
}
