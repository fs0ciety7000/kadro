// SPDX-License-Identifier: GPL-3.0-or-later

//! Heuristics about names and values that may be secrets. They are a safety net around the real
//! rules (secrets only travel through `{secret:}`), used to refuse a `--var` aimed at a slot of
//! credentials and to spot a token in the path of an imported URL.

/// Words that make a name sensitive wherever they appear in it, ignoring case and separators.
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

/// Words that make a name sensitive only as a whole word (`key`, `sig`, `auth`): `monkey` is fine.
const SENSITIVE_WORDS: [&str; 7] = ["key", "sig", "auth", "pass", "pwd", "sid", "jwt"];

/// Schemes of an `Authorization` value.
pub const CREDENTIAL_SCHEMES: [&str; 5] = ["bearer", "basic", "token", "digest", "negotiate"];

/// The lower-case words of a name, split on non-alphanumerics and on `camelCase` boundaries.
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

/// Whether the name of a header, query parameter or field looks like the name of a secret.
#[must_use]
pub fn is_sensitive_name(name: &str) -> bool {
    let words = words(name);
    let joined = words.concat();
    SENSITIVE_SUBSTRINGS.iter().any(|w| joined.contains(w))
        || words.iter().any(|w| SENSITIVE_WORDS.contains(&w.as_str()))
}

/// Splits `Bearer abc` into `("Bearer ", "abc")`; `None` when the value has no scheme.
#[must_use]
pub fn split_scheme(value: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = value.split_once(' ')?;
    let rest = rest.trim_start();
    (CREDENTIAL_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) && !rest.is_empty())
        .then(|| (&value[..value.len() - rest.len()], rest))
}

/// Whether a path segment looks like a token: long, with several kinds of characters and a high
/// Shannon entropy. `upload`, `v1`, `file-2026-11-11.png` are not; `a8F3kQ9zL0pX2vB7nC4m` is.
#[must_use]
pub fn looks_like_token_segment(segment: &str) -> bool {
    const MIN_LEN: usize = 20;
    if segment.len() < MIN_LEN || !segment.bytes().all(|b| b.is_ascii_graphic()) {
        return false;
    }
    let has = |f: fn(&u8) -> bool| segment.bytes().any(|b| f(&b));
    let kinds = [
        has(u8::is_ascii_lowercase),
        has(u8::is_ascii_uppercase),
        has(u8::is_ascii_digit),
    ]
    .iter()
    .filter(|k| **k)
    .count();
    let mut counts = [0usize; 256];
    for b in segment.bytes() {
        counts[usize::from(b)] += 1;
    }
    let len = segment.len() as f64;
    let entropy: f64 = counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / len;
            -p * p.log2()
        })
        .sum();
    kinds >= 2 && entropy >= 3.5
}

/// Header names whose value is a plain setting, never a credential, when importing: the value stays
/// in the clear in the profile (`docs/security/secrets.md` section 5, white list).
#[must_use]
pub fn is_public_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "accept" | "accept-language" | "content-type" | "user-agent"
    ) || (lower.starts_with("x-zipline-") && lower != "x-zipline-password")
}

#[cfg(test)]
mod tests {
    use super::{is_public_header, is_sensitive_name, looks_like_token_segment, split_scheme};

    #[test]
    fn sensitive_names() {
        for name in [
            "Authorization",
            "X-Api-Key",
            "access_token",
            "AWSAccessKeyId",
            "key",
            "password",
            "X-Amz-Signature",
        ] {
            assert!(is_sensitive_name(name), "{name}");
        }
        for name in ["title", "visibility", "monkey", "Accept", "design", "file"] {
            assert!(!is_sensitive_name(name), "{name}");
        }
    }

    #[test]
    fn schemes() {
        assert_eq!(split_scheme("Bearer abc"), Some(("Bearer ", "abc")));
        assert_eq!(split_scheme("basic  xyz"), Some(("basic  ", "xyz")));
        assert_eq!(split_scheme("abc"), None);
        assert_eq!(split_scheme("Bearer "), None);
        assert_eq!(split_scheme("Other abc"), None);
    }

    #[test]
    fn tokens_in_paths() {
        assert!(looks_like_token_segment("a8F3kQ9zL0pX2vB7nC4m"));
        assert!(looks_like_token_segment("sk_live_51H8xQz2eZvKYlo2C"));
        for segment in ["upload", "v1", "api", "file-2026-11-11.png", "aaaaaaaaaaaaaaaaaaaaaaaa", "uploadimagefromsharex"] {
            assert!(!looks_like_token_segment(segment), "{segment}");
        }
    }

    #[test]
    fn public_headers() {
        assert!(is_public_header("Accept"));
        assert!(is_public_header("X-Zipline-Format"));
        assert!(!is_public_header("x-zipline-password"));
        assert!(!is_public_header("Authorization"));
    }
}
