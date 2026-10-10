//! The YouTube sign-in, as browser cookies.
//!
//! A browser's cookie export holds every site's sign-in. Only YouTube's
//! cookies are kept from it, the rest are dropped as soon as the file is
//! read, and nothing here ever prints a cookie's value: `Debug` shows names
//! only.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// One cookie from a Netscape cookie file (the format browsers' cookie
/// exporters and yt-dlp write).
#[derive(Clone, PartialEq, Eq)]
pub struct Cookie {
    /// The domain without a leading dot.
    pub domain: String,
    /// Whether subdomains of `domain` get the cookie too.
    pub include_subdomains: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Seconds since 1970; `0` for a cookie that lasts the browser session.
    pub expires: i64,
    pub name: String,
    pub value: String,
}

impl Cookie {
    /// Whether a browser would send this cookie to `host`.
    pub fn matches_host(&self, host: &str) -> bool {
        let host = host.trim_start_matches('.').to_ascii_lowercase();
        host == self.domain
            || (self.include_subdomains
                && host
                    .strip_suffix(self.domain.as_str())
                    .is_some_and(|rest| rest.ends_with('.')))
    }

    fn is_expired(&self, now: i64) -> bool {
        self.expires > 0 && self.expires < now
    }

    fn is_youtube(&self) -> bool {
        self.domain == "youtube.com" || self.domain.ends_with(".youtube.com")
    }
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie")
            .field("domain", &self.domain)
            .field("name", &self.name)
            .field("value", &"<hidden>")
            .finish()
    }
}

/// A set of cookies. Built from a Netscape cookie file; see
/// [`CookieJar::youtube_only`] for the part YtFast keeps.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.cookies.iter().map(|c| c.name.as_str()).collect();
        f.debug_struct("CookieJar").field("names", &names).finish()
    }
}

/// The cookies a signed-in YouTube web page sends, for its request
/// signature (see [`crate::auth`]).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SidCookies {
    /// `SAPISID`, or `__Secure-3PAPISID` when that is missing (YouTube falls
    /// back the same way).
    pub sapisid: Option<String>,
    pub sapisid_1p: Option<String>,
    pub sapisid_3p: Option<String>,
}

impl fmt::Debug for SidCookies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SidCookies")
            .field("sapisid", &self.sapisid.is_some())
            .field("sapisid_1p", &self.sapisid_1p.is_some())
            .field("sapisid_3p", &self.sapisid_3p.is_some())
            .finish()
    }
}

impl SidCookies {
    pub fn any(&self) -> bool {
        self.sapisid.is_some() || self.sapisid_1p.is_some() || self.sapisid_3p.is_some()
    }
}

impl CookieJar {
    /// Reads a Netscape cookie file. Malformed lines are skipped, as
    /// browsers' exporters write slightly different files.
    pub fn parse_netscape(text: &str) -> Self {
        let mut cookies = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end_matches(['\r', '\n']);
            let (line, http_only) = match line.strip_prefix("#HttpOnly_") {
                Some(rest) => (rest, true),
                None => (line, false),
            };
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 7 {
                continue;
            }
            let raw_domain = fields[0].trim();
            let had_dot = raw_domain.starts_with('.');
            let domain = raw_domain.trim_start_matches('.').to_ascii_lowercase();
            if domain.is_empty() {
                continue;
            }
            let flag = |s: &str| s.trim().eq_ignore_ascii_case("TRUE");
            cookies.push(Cookie {
                domain,
                include_subdomains: flag(fields[1]) || had_dot,
                path: fields[2].to_string(),
                secure: flag(fields[3]),
                http_only,
                expires: fields[4].trim().parse().unwrap_or(0),
                name: fields[5].to_string(),
                // A value may itself contain tabs in rare exports; keep the rest.
                value: fields[6..].join("\t"),
            });
        }
        Self { cookies }
    }

    /// The YouTube cookies only, without expired ones. Everything else from
    /// the browser (other sites' sign-ins) is dropped here.
    pub fn youtube_only(&self) -> Self {
        let now = unix_now();
        Self {
            cookies: self
                .cookies
                .iter()
                .filter(|c| c.is_youtube() && !c.is_expired(now))
                .cloned()
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// The value a browser would send for `name` to `host`, preferring the
    /// most specific domain.
    pub fn get(&self, host: &str, name: &str) -> Option<&str> {
        let now = unix_now();
        self.cookies
            .iter()
            .filter(|c| c.name == name && c.matches_host(host) && !c.is_expired(now))
            .max_by_key(|c| c.domain.len())
            .map(|c| c.value.as_str())
    }

    /// The `Cookie:` header a browser would send to `host`, or `None` when
    /// no cookie applies.
    pub fn header_for(&self, host: &str) -> Option<String> {
        let now = unix_now();
        let mut pairs: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| c.matches_host(host) && !c.is_expired(now))
            .collect();
        // More specific domains first, like browsers, and one value per name.
        pairs.sort_by_key(|c| std::cmp::Reverse(c.domain.len()));
        let mut seen = std::collections::HashSet::new();
        let header = pairs
            .into_iter()
            .filter(|c| seen.insert(c.name.as_str()))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");
        (!header.is_empty()).then_some(header)
    }

    /// Whether a cookie with this name is set for `host`.
    pub fn has(&self, host: &str, name: &str) -> bool {
        self.get(host, name).is_some()
    }

    /// The signature cookies for `host`, with YouTube's own fallback.
    pub fn sid_cookies(&self, host: &str) -> SidCookies {
        let get = |name: &str| self.get(host, name).map(str::to_string);
        let sapisid_3p = get("__Secure-3PAPISID");
        SidCookies {
            sapisid: get("SAPISID").or_else(|| sapisid_3p.clone()),
            sapisid_1p: get("__Secure-1PAPISID"),
            sapisid_3p,
        }
    }

    /// Whether these cookies look like a signed-in YouTube session:
    /// `LOGIN_INFO` (cleared when YouTube signs a session out) and at least
    /// one signature cookie. This is what yt-dlp checks too.
    pub fn looks_signed_in(&self) -> bool {
        let host = "www.youtube.com";
        self.has(host, "LOGIN_INFO") && self.sid_cookies(host).any()
    }

    /// Writes the cookies back as a Netscape cookie file, for yt-dlp.
    pub fn to_netscape(&self) -> String {
        let mut out = String::from(
            "# Netscape HTTP Cookie File\n# Written by YtFast: YouTube cookies only.\n\n",
        );
        for c in &self.cookies {
            let tf = |b: bool| if b { "TRUE" } else { "FALSE" };
            let domain = if c.include_subdomains {
                format!(".{}", c.domain)
            } else {
                c.domain.clone()
            };
            let prefix = if c.http_only { "#HttpOnly_" } else { "" };
            out.push_str(&format!(
                "{prefix}{domain}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                tf(c.include_subdomains),
                c.path,
                tf(c.secure),
                c.expires,
                c.name,
                c.value
            ));
        }
        out
    }

    /// Adds `SOCS=CAI`, YouTube's "consent already answered" cookie, when the
    /// browser has none, so no request lands on the consent page instead of
    /// YouTube Music. yt-dlp and ytmusicapi send the same value.
    pub fn with_consent(mut self) -> Self {
        if !self.has("music.youtube.com", "SOCS") {
            self.cookies.push(Cookie {
                domain: "youtube.com".into(),
                include_subdomains: true,
                path: "/".into(),
                secure: true,
                http_only: false,
                expires: 0,
                name: "SOCS".into(),
                value: "CAI".into(),
            });
        }
        self
    }
}

/// Whether this app may read other apps' data (a Mac's Full Disk Access),
/// as far as can be told: `Some(false)` when the Mac refuses a file only
/// that permission opens, `Some(true)` when it allows one, `None` when it
/// cannot be told, and on Windows. Without it, a Mac hides Safari's cookies
/// and may hide another browser's data from yt-dlp, which then reads as
/// missing. Asking costs nothing and asks the listener nothing: there is no
/// question an app can put for this permission.
pub fn full_disk_access() -> Option<bool> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    let mut told = None;
    for protected in [
        "Library/Application Support/com.apple.TCC/TCC.db",
        "Library/Safari",
    ] {
        let path = home.join(protected);
        let opened = if path.is_dir() {
            std::fs::read_dir(&path).map(|_| ())
        } else {
            std::fs::File::open(&path).map(|_| ())
        };
        match opened {
            Ok(()) => told = Some(true),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Some(false),
            Err(_) => {}
        }
    }
    told
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# Netscape HTTP Cookie File\n\
        # This file is generated by yt-dlp.  Do not edit.\n\
        \n\
        .youtube.com\tTRUE\t/\tTRUE\t4102444800\tSAPISID\tsap-value\n\
        .youtube.com\tTRUE\t/\tTRUE\t4102444800\t__Secure-1PAPISID\tone-p\n\
        .youtube.com\tTRUE\t/\tTRUE\t4102444800\t__Secure-3PAPISID\tthree-p\n\
        #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t4102444800\tLOGIN_INFO\tlogin-value\n\
        music.youtube.com\tFALSE\t/\tTRUE\t0\tPREF\tmusic-only\n\
        .youtube.com\tTRUE\t/\tFALSE\t1000\tOLD\texpired\n\
        .google.com\tTRUE\t/\tTRUE\t4102444800\tSID\tgoogle-session\n\
        .bank.example\tTRUE\t/\tTRUE\t4102444800\tsession\tsecret\n\
        not a cookie line\n";

    #[test]
    fn keeps_only_youtube_cookies() {
        let jar = CookieJar::parse_netscape(FILE).youtube_only();
        let names: Vec<&str> = jar.cookies.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "SAPISID",
                "__Secure-1PAPISID",
                "__Secure-3PAPISID",
                "LOGIN_INFO",
                "PREF"
            ]
        );
        assert!(!jar.to_netscape().contains("google-session"));
        assert!(!jar.to_netscape().contains("secret"));
    }

    #[test]
    fn http_only_lines_are_cookies_not_comments() {
        let jar = CookieJar::parse_netscape(FILE);
        let login = jar.cookies.iter().find(|c| c.name == "LOGIN_INFO").unwrap();
        assert!(login.http_only);
        assert!(jar.youtube_only().looks_signed_in());
    }

    #[test]
    fn host_matching_follows_the_subdomain_flag() {
        let jar = CookieJar::parse_netscape(FILE).youtube_only();
        assert_eq!(jar.get("music.youtube.com", "PREF"), Some("music-only"));
        assert_eq!(jar.get("www.youtube.com", "PREF"), None);
        assert_eq!(jar.get("music.youtube.com", "SAPISID"), Some("sap-value"));
        // A lookalike domain must not receive YouTube's cookies.
        assert_eq!(jar.get("evilyoutube.com", "SAPISID"), None);
    }

    #[test]
    fn header_has_one_value_per_name() {
        let jar = CookieJar::parse_netscape(FILE).youtube_only();
        let header = jar.header_for("music.youtube.com").unwrap();
        assert!(header.contains("SAPISID=sap-value"));
        assert!(header.contains("PREF=music-only"));
        assert!(!header.contains("OLD="));
        assert_eq!(header.matches("SAPISID=").count(), 1);
    }

    #[test]
    fn sid_cookies_fall_back_to_3p() {
        let text = ".youtube.com\tTRUE\t/\tTRUE\t4102444800\t__Secure-3PAPISID\tthree-p\n";
        let sids = CookieJar::parse_netscape(text).sid_cookies("www.youtube.com");
        assert_eq!(sids.sapisid.as_deref(), Some("three-p"));
        assert_eq!(sids.sapisid_1p, None);
    }

    #[test]
    fn round_trips_through_netscape_text() {
        let jar = CookieJar::parse_netscape(FILE).youtube_only();
        assert_eq!(CookieJar::parse_netscape(&jar.to_netscape()), jar);
    }

    #[test]
    fn debug_never_shows_values() {
        let jar = CookieJar::parse_netscape(FILE);
        let shown = format!("{jar:?} {:?}", jar.sid_cookies("www.youtube.com"));
        assert!(!shown.contains("sap-value"));
        assert!(!shown.contains("login-value"));
    }

    #[test]
    fn adds_consent_only_when_missing() {
        let jar = CookieJar::parse_netscape(FILE)
            .youtube_only()
            .with_consent();
        assert_eq!(jar.get("music.youtube.com", "SOCS"), Some("CAI"));
        let own = ".youtube.com\tTRUE\t/\tTRUE\t4102444800\tSOCS\tmine\n";
        let jar = CookieJar::parse_netscape(own).with_consent();
        assert_eq!(jar.get("music.youtube.com", "SOCS"), Some("mine"));
    }
}
