// SPDX-License-Identifier: GPL-3.0-or-later

//! Detection of secrets given in arguments (`docs/cli.md` section 1, principle 5): a command line is
//! visible to every process of the machine and ends up in the shell history.
//!
//! The detection is a safety net, not a proof: it looks at the name of what is set (a query
//! parameter, a `--set` key) and at the shape of the value (a credential header, a well-known token
//! prefix). A value made only of `{secret:NAME}` references carries no secret and is always accepted.
//! The shape of a credential value and the percent-decoding live in `kropa-upload::sensitive`, shared
//! with the detector of the profiles.

/// Words that make a name sensitive wherever they appear in it, ignoring case and separators
/// (`X-Api-Key`, `access_token`, `AWSAccessKeyId`, `X-Amz-Signature`).
const SENSITIVE_SUBSTRINGS: [&str; 13] = [
    "authorization",
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "bearer",
    "credential",
    "accesskey",
    "privatekey",
    "signature",
    "cookie",
    "session",
];

/// Words that make a name sensitive only as a whole word (`key`, `sig`, `auth`), so that `monkey`
/// or `design` are left alone.
const SENSITIVE_WORDS: [&str; 7] = ["key", "sig", "auth", "pass", "pwd", "sid", "jwt"];

use kropa_upload::sensitive::{CREDENTIAL_SCHEMES, looks_like_credential_value, percent_decode};

/// The lower-case words of a name, split on anything that is not a letter or a digit and on
/// `camelCase` boundaries (also after an acronym): `AWSAccessKeyId` gives `aws`, `access`, `key`, `id`.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        // `accessKey`: lower then upper. `AWSAccess`: the last capital of a run that starts a word.
        let boundary = c.is_uppercase()
            && previous.is_some_and(|p| {
                p.is_lowercase() || (p.is_uppercase() && next.is_some_and(char::is_lowercase))
            });
        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Whether the name of a parameter, header or key looks like the name of a secret.
#[must_use]
pub fn is_sensitive_key(key: &str) -> bool {
    let key = percent_decode(key);
    let words = words(&key);
    let joined = words.concat();
    SENSITIVE_SUBSTRINGS.iter().any(|w| joined.contains(w))
        || words.iter().any(|w| SENSITIVE_WORDS.contains(&w.as_str()))
}

/// Whether `value` is made only of `{secret:NAME}` references, optionally behind an authorization
/// scheme (`Bearer {secret:token}`) or glued by colons (`{secret:user}:{secret:pass}`): there is
/// nothing to hide in it, the secret itself stays in the secret store.
#[must_use]
pub fn only_secret_references(value: &str) -> bool {
    let mut rest = String::new();
    let mut references = 0;
    let mut tail = value;
    while let Some(start) = tail.find("{secret:") {
        let (before, after) = tail.split_at(start);
        let Some(end) = after.find('}') else { break };
        let name = &after["{secret:".len()..end];
        let valid = (1..=64).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid {
            break;
        }
        rest.push_str(before);
        references += 1;
        tail = &after[end + 1..];
    }
    rest.push_str(tail);
    if references == 0 {
        return false;
    }
    let lower = rest.trim().to_lowercase();
    let without_scheme = CREDENTIAL_SCHEMES
        .iter()
        .find_map(|s| {
            lower
                .strip_prefix(s)
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        })
        .unwrap_or(&lower);
    without_scheme
        .chars()
        .all(|c| c.is_whitespace() || c == ':')
}

/// `NAME=VALUE` pairs of a query, a fragment or a matrix parameter list: separators `&` and `;`.
/// Pairs without a value are left out: a bare `?token` leaks nothing.
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.split(['&', ';'])
        .filter_map(|p| p.split_once('='))
        .filter(|(_, v)| !v.is_empty())
}

fn pair_carries_secret(key: &str, value: &str) -> bool {
    !only_secret_references(value) && (is_sensitive_key(key) || looks_like_credential_value(value))
}

/// A URL with `user:pass@` in it, or a query parameter, fragment parameter or matrix parameter
/// named or valued like a secret (`?key=`, `#access_token=`, `;jsessionid=`, `X-Amz-Signature`).
#[must_use]
pub fn url_carries_secret(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (location, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = location.split_once('/').unwrap_or((location, ""));
    if authority.contains('@') {
        return true;
    }
    // Matrix parameters: `/files;jsessionid=abc/x`.
    let matrix = path
        .split(';')
        .skip(1)
        .map(|chunk| chunk.split('/').next().unwrap_or_default());
    let fragment_query = fragment.split_once('?').map_or("", |(_, q)| q);
    matrix
        .flat_map(pairs)
        .chain(pairs(query))
        .chain(pairs(fragment))
        .chain(pairs(fragment_query))
        .any(|(k, v)| pair_carries_secret(k, v))
}

/// Whether `--set KEY=VALUE` puts a secret in the command line.
#[must_use]
pub fn set_carries_secret(key: &str, value: &str) -> bool {
    if value.is_empty() || only_secret_references(value) {
        return false;
    }
    is_sensitive_key(key) || looks_like_credential_value(value) || value_url_carries_secret(value)
}

/// Whether `--var NAME=VALUE` puts a secret in the command line. The name is not looked at: a
/// variable may be called `password` because it is answered on purpose. Refusing a variable that
/// feeds a secret slot of an imported profile depends on that profile (lot 1.7).
#[must_use]
pub fn var_carries_secret(value: &str) -> bool {
    !only_secret_references(value)
        && (looks_like_credential_value(value) || value_url_carries_secret(value))
}

fn value_url_carries_secret(value: &str) -> bool {
    let lower = value.trim_start().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && url_carries_secret(value)
}

#[cfg(test)]
mod tests {
    use super::{
        is_sensitive_key, looks_like_credential_value, only_secret_references, percent_decode,
        set_carries_secret, url_carries_secret, var_carries_secret, words,
    };

    #[test]
    fn urls_with_credentials_are_detected() {
        for bad in [
            "https://user:pw@host/x",
            "https://user@host",
            "https://host/p?token=abc",
            "https://host/p?a=1&X-Api-Key=abc",
            "host:1@x",
            "https://host/p?key=abc",
            "https://host/p?a=1;key=abc",
            "https://host/p?sig=abc",
            "https://host/p?auth=abc",
            "https://host/p?access_key=abc",
            "https://host/p?cookie=abc",
            "https://host/p?session=abc",
            "https://bucket.s3.amazonaws.com/x?X-Amz-Signature=abcdef",
            "https://bucket.s3.amazonaws.com/x?X-Amz-Credential=abcdef",
            "https://bucket.s3.amazonaws.com/x?AWSAccessKeyId=abcdef",
            "https://host/p?%74oken=abc",
            "https://host/p?%2574oken=abc",
            "https://host/p?api%2Dkey=abc",
            "https://host/p#access_token=abc",
            "https://host/p#a=1&token=abc",
            "https://host/app#/cb?token=abc",
            "https://host/x;jsessionid=abc",
            "https://host/p?next=ghp_0123456789abcdefghij",
            "https://host/p?x=sk-live-0123456789abcdef",
            "https://host/p?x=AKIAABCDEFGHIJKLMNOP",
            "https://host/p?x=eyJhbGciOi.eyJzdWIiOi.c2lnbmF0dXJl",
            "https://host/p?x=Bearer%20abcdef",
        ] {
            assert!(url_carries_secret(bad), "{bad}");
        }
        for good in [
            "https://host/path@v2",
            "https://host/p?a=1&b=2#frag@x",
            "https://zipline.example.org",
            "https://host/p?monkey=1&design=2&keyboard=3",
            "https://host/p?token=",
            "https://host/p?token",
            "https://host/p?q=sk-learn",
            "https://host/p?token={secret:t}",
            "https://host/p#section-2",
            "https://host/p?a=1;b=2",
        ] {
            assert!(!url_carries_secret(good), "{good}");
        }
    }

    #[test]
    fn sensitive_keys() {
        for bad in [
            "headers.Authorization",
            "API_KEY",
            "key",
            "sig",
            "auth",
            "X-Api-Key",
            "access_key",
            "AccessKey",
            "Cookie",
            "session_id",
            "client_secret",
            "x%2Dtoken",
        ] {
            assert!(is_sensitive_key(bad), "{bad}");
        }
        for good in [
            "response.url",
            "monkey",
            "design",
            "keyboard",
            "author",
            "note",
        ] {
            assert!(!is_sensitive_key(good), "{good}");
        }
        assert_eq!(words("AWSAccessKeyId"), ["aws", "access", "key", "id"]);
        assert_eq!(words("X-Amz-Signature"), ["x", "amz", "signature"]);
        assert_eq!(percent_decode("%2574oken%zz%"), "token%zz%");
    }

    #[test]
    fn credential_values() {
        for bad in [
            "Bearer abc",
            " basic abc",
            "Token abc",
            "digest abc",
            "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
            "sk-live-SENTINEL123",
            "xoxb-1234-5678-abcdefgh",
            "AKIAABCDEFGHIJKLMNOP",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln",
            "Bearer%20abc",
        ] {
            assert!(looks_like_credential_value(bad), "{bad}");
        }
        for good in [
            "bearers",
            "token",
            "sk-learn",
            "AKIA short",
            "eyJ-not-a-jwt-but-long-enough",
            "plain text value",
            "",
        ] {
            assert!(!looks_like_credential_value(good), "{good}");
        }
    }

    #[test]
    fn references_are_not_secrets() {
        for ok in [
            "{secret:token}",
            "Bearer {secret:token}",
            "bearer  {secret:a}",
            "Basic {secret:user}:{secret:pass}",
            "{secret:a}{secret:b}",
        ] {
            assert!(only_secret_references(ok), "{ok}");
            assert!(!set_carries_secret("headers.Authorization", ok), "{ok}");
        }
        for not in [
            "Bearer abc",
            "Bearer {secret:token}abc",
            "x{secret:t}",
            "{secret:}",
            "{secret:a b}",
            "{secret:t",
            "",
        ] {
            assert!(!only_secret_references(not), "{not}");
        }
        // A reference does not excuse a literal secret next to it.
        assert!(set_carries_secret(
            "headers.Authorization",
            "Bearer abc{secret:t}"
        ));
        assert!(!var_carries_secret("{secret:t}"));
    }

    #[test]
    fn set_and_var() {
        assert!(set_carries_secret("headers.Authorization", "anything"));
        assert!(set_carries_secret("note", "Bearer abc"));
        assert!(set_carries_secret("note", "sk-live-0123456789abcdef"));
        assert!(set_carries_secret("link", "https://u:p@host/"));
        assert!(!set_carries_secret("note", "hello"));
        assert!(!set_carries_secret("headers.Authorization", ""));
        assert!(var_carries_secret("Bearer abc"));
        assert!(var_carries_secret("https://host/?token=abc"));
        assert!(!var_carries_secret("my holiday"));
    }
}
