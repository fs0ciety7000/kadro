// SPDX-License-Identifier: GPL-3.0-or-later

//! The redacted debug journal of an upload (`-v`, `-vv`; `docs/security/secrets.md` section 6).
//!
//! No `tracing` here: the journal is a list of lines that the caller prints on stderr, and nothing
//! in it can reach the network. Every line goes through the detector of [`MaskedText`] with **every
//! secret loaded in the process** (N5) before it is stored, so that a line cannot be written
//! without the final pass. What is built on top of it:
//!
//! - headers on a white list (`content-type`, `content-length`, `accept`, `user-agent`,
//!   `retry-after`, `location` without query); every other value is `***` (`Authorization`,
//!   `Cookie`, `Set-Cookie`, the headers declared as secrets...);
//! - URLs without user info or fragment, with the value of a sensitive query parameter and any
//!   path segment that looks like a token replaced by `***`;
//! - never the body of a request; the body of an answer only when it is an error, masked and cut.

use std::sync::Mutex;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use url::Url;

use crate::masked::{MASK, MaskedText, SecretSet};
use crate::sensitive::{is_sensitive_name, looks_like_token_segment};

/// Longest line kept, in characters.
const LINE_LIMIT: usize = 1000;
/// Characters of an error body kept.
pub(crate) const BODY_LIMIT: usize = 200;

/// Header names whose value may be written.
const LOGGED_HEADERS: [&str; 6] = [
    "content-type",
    "content-length",
    "accept",
    "user-agent",
    "retry-after",
    "location",
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
            .map(|c| if c.is_control() { ' ' } else { c })
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

/// One header for the journal: the value only for the names of the white list.
fn redact_header(name: &HeaderName, value: &HeaderValue) -> String {
    let shown = LOGGED_HEADERS
        .contains(&name.as_str())
        .then(|| value.to_str().ok())
        .flatten()
        .filter(|_| !value.is_sensitive());
    match shown {
        // Without the query or fragment of a `location`: they may carry a token.
        Some(text) if name.as_str() == "location" => {
            format!(
                "{name}: {}",
                text.split(['?', '#']).next().unwrap_or_default()
            )
        }
        Some(text) => format!("{name}: {text}"),
        None => format!("{name}: {MASK}"),
    }
}

/// Headers of a request or an answer, as `name: value; name: ***`.
pub(crate) fn redact_headers<'a>(
    headers: impl IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
) -> String {
    headers
        .into_iter()
        .map(|(n, v)| redact_header(n, v))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Headers of an answer.
pub(crate) fn redact_header_map(headers: &HeaderMap) -> String {
    redact_headers(headers.iter())
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
            ("cookie", "a=b"),
            ("set-cookie", "s=t"),
            ("x-declared", "value"),
            ("location", "https://h.example.org/p?token=zz"),
        ] {
            map.append(
                HeaderName::from_bytes(n.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        let line = redact_header_map(&map);
        for hidden in ["something", "a=b", "s=t", "value", "zz"] {
            assert!(!line.contains(hidden), "{line}");
        }
        for shown in [
            "content-type: image/png",
            "authorization: ***",
            "cookie: ***",
            "set-cookie: ***",
            "x-declared: ***",
            "location: https://h.example.org/p",
        ] {
            assert!(line.contains(shown), "{shown} in {line}");
        }
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
        let lines = log.take();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].1.contains(&token), "{:?}", lines[0]);
        assert!(lines[0].1.contains("***"), "{:?}", lines[0]);
        assert!(!lines[0].1.chars().any(char::is_control), "{:?}", lines[0]);
        assert!(DebugLog::new(0).take().is_empty());
        let silent = DebugLog::new(0);
        silent.record(1, "x");
        assert!(silent.take().is_empty());
    }
}
