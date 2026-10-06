//! Keeps secrets out of anything shown on screen or saved in a report.
//!
//! YouTube's stream and report addresses carry session details in their
//! query strings, and error messages from libraries can repeat them. Every
//! message that leaves the core goes through here.

/// Shortens every web address in `text` to its scheme and host.
pub fn urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find_scheme(rest) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']'))
            .unwrap_or(tail.len());
        let address = &tail[..end];
        let after_scheme = address.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = address[after_scheme..]
            .find(['/', '?', '#'])
            .map(|i| after_scheme + i)
            .unwrap_or(address.len());
        out.push_str(&address[..host_end]);
        if host_end < address.len() {
            out.push_str("/…");
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn find_scheme(text: &str) -> Option<usize> {
    match (text.find("https://"), text.find("http://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_scheme_and_host() {
        assert_eq!(
            urls(
                "error sending request for url (https://rr3---sn-x.googlevideo.com/videoplayback?expire=1&ip=1.2.3.4&sig=abc)"
            ),
            "error sending request for url (https://rr3---sn-x.googlevideo.com/…)"
        );
        assert_eq!(
            urls("see https://music.youtube.com now"),
            "see https://music.youtube.com now"
        );
        assert_eq!(urls("no address here"), "no address here");
        assert_eq!(
            urls("a http://x.example/p?q b https://y.example/"),
            "a http://x.example/… b https://y.example/…"
        );
    }
}
