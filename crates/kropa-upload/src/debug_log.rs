// SPDX-License-Identifier: GPL-3.0-or-later

//! The redacted debug journal of an upload (`-v`, `-vv`; `docs/security/secrets.md` section 6).
//!
//! No `tracing` here: the journal is a list of lines that the caller prints on stderr, and nothing
//! in it can reach the network. Every line goes through the detector of [`MaskedText`] with **every
//! secret loaded in the process** (N5) before it is stored, so that a line cannot be written
//! without the final pass. What is built on top of it:
//!
//! - headers on a white list (`content-type`, `content-length`, `retry-after`, `location` through
//!   [`redact_url`]); every other value is `***` (`Authorization`, `Cookie`, `Set-Cookie`, the
//!   headers declared as secrets...). The names of the headers of an answer are written only when
//!   they are on a list of well-known names; the others are counted, never written (a server could
//!   send a secret as a header name, which arrives in lower case and escapes the detector);
//! - URLs without user info or fragment, with the value of a sensitive query parameter and any
//!   path segment that looks like a token replaced by `***`;
//! - never the body of a request; the body of an answer only when it is an error, masked and cut.

use std::sync::Mutex;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use url::Url;

use crate::engine::is_hostile_char;
use crate::masked::{MASK, MaskedText, SecretSet};
use crate::sensitive::{is_sensitive_name, looks_like_token_segment};

/// Longest line kept, in characters.
const LINE_LIMIT: usize = 1000;
/// Characters of an error body kept.
pub(crate) const BODY_LIMIT: usize = 200;

/// Header names whose value may be written.
const LOGGED_HEADERS: [&str; 4] = ["content-type", "content-length", "retry-after", "location"];

/// Names of headers of an answer that are written (with `***` as the value unless they are on
/// [`LOGGED_HEADERS`]). Any other name is only counted.
const KNOWN_RESPONSE_HEADERS: [&str; 16] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "www-authenticate",
    "proxy-authenticate",
    "x-api-key",
    "server",
    "date",
    "etag",
    "cache-control",
    "vary",
    "connection",
    "content-encoding",
    "content-disposition",
    "transfer-encoding",
];

/// Lines of one run, already redacted.
#[derive(Debug, Default)]
pub struct DebugLog {
    level: u8,
    lines: Mutex<Vec<(u8, String)>>,
}

impl DebugLog {
    /// A journal that keeps the lines up to `level` (0: none, 1: `-v`, 2: `-vv`).
    #[must_use]
    pub fn new(level: u8) -> Self {
        Self {
            level,
            lines: Mutex::new(Vec::new()),
        }
    }

    /// Whether a line of `level` is kept.
    #[must_use]
    pub fn enabled(&self, level: u8) -> bool {
        level != 0 && self.level >= level
    }

    /// Redacts and keeps a line. The text is masked **before** anything else is done to it.
    pub(crate) fn record(&self, level: u8, text: &str) {
        if !self.enabled(level) {
            return;
        }
        let secrets = SecretSet::loaded();
        let masked = MaskedText::from_response(text.as_bytes(), &secrets, LINE_LIMIT);
        let line: String = masked
            .as_str()
            .chars()
            .map(|c| if is_hostile_char(c) { ' ' } else { c })
            .collect();
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((level, line));
    }

    /// Takes the lines kept so far, as `(level, text)`.
    #[must_use]
    pub fn take(&self) -> Vec<(u8, String)> {
        std::mem::take(
            &mut *self
                .lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// A URL for the journal: no user info, no fragment, `***` for the value of a sensitive parameter
/// and for a path segment that looks like a token.
#[must_use]
pub fn redact_url(url: &Url) -> String {
    let mut out = format!("{}://", url.scheme());
    if let Some(host) = url.host_str() {
        out.push_str(host);
    }
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    let path: Vec<&str> = url
        .path()
        .split('/')
        .map(|s| if looks_like_token_segment(s) { MASK } else { s })
        .collect();
    out.push_str(&path.join("/"));
    let pairs: Vec<String> = url
        .query_pairs()
        .map(|(k, v)| {
            if is_sensitive_name(&k) {
                format!("{k}={MASK}")
            } else {
                format!("{k}={v}")
            }
        })
        .collect();
    if !pairs.is_empty() {
        out.push('?');
        out.push_str(&pairs.join("&"));
    }
    out
}

/// One header for the journal: the value only for the names of the white list. A `location` is
/// resolved against `base` (the address of the request) and goes through [`redact_url`].
fn redact_header(name: &HeaderName, value: &HeaderValue, base: Option<&Url>) -> String {
    let shown = LOGGED_HEADERS
        .contains(&name.as_str())
        .then(|| value.to_str().ok())
        .flatten()
        .filter(|_| !value.is_sensitive());
    match shown {
        Some(text) if name.as_str() == "location" => match base.and_then(|b| b.join(text).ok()) {
            Some(next) => format!("{name}: {}", redact_url(&next)),
            None => format!("{name}: {MASK}"),
        },
        Some(text) => format!("{name}: {text}"),
        None => format!("{name}: {MASK}"),
    }
}

/// Headers of a request, as `name: value; name: ***`. The names come from the destination.
pub(crate) fn redact_headers<'a>(
    headers: impl IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
) -> String {
    headers
        .into_iter()
        .map(|(n, v)| redact_header(n, v, None))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Headers of an answer to the request to `base`: the names that are not well known are counted.
pub(crate) fn redact_header_map(headers: &HeaderMap, base: &Url) -> String {
    let mut parts = Vec::new();
    let mut others = 0usize;
    for (name, value) in headers {
        if LOGGED_HEADERS.contains(&name.as_str())
            || KNOWN_RESPONSE_HEADERS.contains(&name.as_str())
        {
            parts.push(redact_header(name, value, Some(base)));
        } else {
            others += 1;
        }
    }
    if others > 0 {
        parts.push(format!("{others} other header(s): {MASK}"));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use url::Url;

    use super::{DebugLog, redact_header_map, redact_headers, redact_url};
    use crate::masked::register_secret;

    #[test]
    fn urls_lose_user_info_fragment_and_sensitive_values() {
        let url = Url::parse(
            "https://u:p@h.example.org:8443/api/a8F3kQ9zTESTONLY0pX2vB7nC4m/up?api_key=abc&title=x&X-Amz-Signature=s#frag",
        )
        .unwrap();
        assert_eq!(
            redact_url(&url),
            "https://h.example.org:8443/api/***/up?api_key=***&title=x&X-Amz-Signature=***"
        );
    }

    #[test]
    fn only_the_white_list_of_headers_shows_a_value() {
        let mut map = HeaderMap::new();
        for (n, v) in [
            ("content-type", "image/png"),
            ("authorization", "Bearer something"),
            ("cookie", "ck=1"),
            ("set-cookie", "s=t"),
            ("x-declared", "value"),
            ("location", "https://u:pw@h.example.org/p?token=zz&a=b#frag"),
        ] {
            map.append(
                HeaderName::from_bytes(n.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        let base = Url::parse("https://h.example.org/up").unwrap();
        let line = redact_header_map(&map, &base);
        for hidden in [
            "something",
            "ck=1",
            "s=t",
            "value",
            "zz",
            "pw",
            "frag",
            "x-declared",
        ] {
            assert!(!line.contains(hidden), "{line}");
        }
        for shown in [
            "content-type: image/png",
            "authorization: ***",
            "cookie: ***",
            "set-cookie: ***",
            "1 other header(s): ***",
            "location: https://h.example.org/p?token=***&a=b",
        ] {
            assert!(line.contains(shown), "{shown} in {line}");
        }
        // The name of a header that is not well known is counted, never written.
        let mut odd = HeaderMap::new();
        odd.append(
            HeaderName::from_bytes(b"jrnl-zq7klm9xt2vrw").unwrap(),
            HeaderValue::from_static("v"),
        );
        let line = redact_header_map(&odd, &base);
        assert_eq!(line, "1 other header(s): ***");
        // A value flagged sensitive is hidden even under a name of the white list.
        let mut v = HeaderValue::from_static("text/secret");
        v.set_sensitive(true);
        let name = HeaderName::from_static("content-type");
        assert_eq!(redact_headers([(&name, &v)]), "content-type: ***");
    }

    #[test]
    fn a_line_is_masked_cut_and_has_no_control_character() {
        let token = format!("{}{}", "journal-", "Zq7kLm9Xt2vRw");
        register_secret(&token);
        let log = DebugLog::new(2);
        log.record(2, &format!("echo {token}\nnext line\r\u{1b}[31m"));
        log.record(3, "above the level");
        log.record(2, "a\u{202E}b\u{200E}c\u{2028}d\u{ad}e\u{9b}f");
        let lines = log.take();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].1, "a b c d e f");
        assert!(!lines[0].1.contains(&token), "{:?}", lines[0]);
        assert!(lines[0].1.contains("***"), "{:?}", lines[0]);
        assert!(!lines[0].1.chars().any(char::is_control), "{:?}", lines[0]);
        assert!(DebugLog::new(0).take().is_empty());
        let silent = DebugLog::new(0);
        silent.record(1, "x");
        assert!(silent.take().is_empty());
    }
}
