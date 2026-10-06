//! The request signature a signed-in YouTube web page sends with each API
//! call (`Authorization: SAPISIDHASH ...`).
//!
//! It is a SHA-1 of the time, a session cookie and the page's origin. This
//! follows yt-dlp (`_get_sid_authorization_header`), which tracks what
//! YouTube's own pages send: one hash per signature cookie, and the user
//! session ID mixed in when the page has one.

use sha1::{Digest, Sha1};

use crate::cookies::SidCookies;

/// The `Authorization` header value, or `None` without signature cookies.
pub fn authorization(
    sids: &SidCookies,
    origin: &str,
    user_session_id: Option<&str>,
    unix_time: u64,
) -> Option<String> {
    let schemes = [
        ("SAPISIDHASH", sids.sapisid.as_deref()),
        ("SAPISID1PHASH", sids.sapisid_1p.as_deref()),
        ("SAPISID3PHASH", sids.sapisid_3p.as_deref()),
    ];
    let parts: Vec<String> = schemes
        .into_iter()
        .filter_map(|(scheme, sid)| {
            sid.map(|sid| signature(scheme, sid, origin, user_session_id, unix_time))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn signature(
    scheme: &str,
    sid: &str,
    origin: &str,
    user_session_id: Option<&str>,
    unix_time: u64,
) -> String {
    let time = unix_time.to_string();
    let mut input: Vec<&str> = Vec::with_capacity(4);
    if let Some(user) = user_session_id {
        input.push(user);
    }
    input.extend([time.as_str(), sid, origin]);
    let digest = Sha1::digest(input.join(" ").as_bytes());
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    match user_session_id {
        Some(_) => format!("{scheme} {time}_{hash}_u"),
        None => format!("{scheme} {time}_{hash}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://music.youtube.com";

    fn sids() -> SidCookies {
        SidCookies {
            sapisid: Some("sap-value".into()),
            sapisid_1p: Some("one-p".into()),
            sapisid_3p: Some("three-p".into()),
        }
    }

    // Expected hashes computed independently with Python's hashlib:
    // sha1(" ".join([time, sid, origin])), with the user session ID first
    // when there is one.
    #[test]
    fn matches_reference_hashes() {
        let header = authorization(&sids(), ORIGIN, None, 1_700_000_000).unwrap();
        assert_eq!(
            header,
            "SAPISIDHASH 1700000000_c809c1407479038f3389af523e6c57ad15a79a49 \
             SAPISID1PHASH 1700000000_3ccb862fb31696abe532233be1b232c729af5863 \
             SAPISID3PHASH 1700000000_6370dc4adb7b1631567dc500c947699a02d89987"
        );
    }

    #[test]
    fn mixes_in_the_user_session() {
        let header = authorization(&sids(), ORIGIN, Some("1234567890"), 1_700_000_000).unwrap();
        assert_eq!(
            header,
            "SAPISIDHASH 1700000000_75a176ec7bdc50185307e268cbebb871293fae0a_u \
             SAPISID1PHASH 1700000000_6cf1752fa157f9787ab176dd00b18f70e2d8696b_u \
             SAPISID3PHASH 1700000000_736463bb25bf853d578219962996570d0010017f_u"
        );
    }

    #[test]
    fn nothing_without_cookies() {
        assert_eq!(authorization(&SidCookies::default(), ORIGIN, None, 1), None);
    }
}
