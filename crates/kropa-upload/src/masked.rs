// SPDX-License-Identifier: GPL-3.0-or-later

//! Output control and masking (`docs/security/secrets.md` section 4.4 and 6).
//!
//! One detector serves both:
//!
//! - [`CheckedOutput`]: the only type that stdout, JSON, the history and the clipboard accept for
//!   text taken from a response. Its only constructor refuses text that holds a secret, in any of
//!   the forms below.
//! - [`MaskedText`]: the only type for the error text and the excerpt of a response. The secret is
//!   replaced by `***` on the **whole** body first, the mask is widened to the whole run of the
//!   alphabet of the form found, and only then is the text cut.
//!
//! Forms searched for every secret: raw; percent-encoded (twice, `+` as a space too); HTML entities;
//! JSON escapes (twice); base64, standard and URL-safe, with or without `=`, at the three
//! alignments (the stable part of the encoding); hexadecimal in both cases. White space is ignored
//! for base64 and hexadecimal. "Every secret" is every secret loaded since the program started
//! (N5), plus the values rendered into the request that held one (`Bearer <secret>`).

use std::fmt;
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

use crate::secret::MIN_SECRET_LEN;

/// Secrets loaded since the program started (N5).
static LOADED: Mutex<Vec<Zeroizing<Vec<u8>>>> = Mutex::new(Vec::new());

/// Remembers a secret for the detector, for the rest of the run. Values shorter than
/// [`MIN_SECRET_LEN`] are ignored (they are refused earlier).
///
/// A poisoned lock (a panic elsewhere while it was held) is taken over: the list of values stays
/// valid, and no later upload may fail for it.
pub fn register_secret(value: &str) {
    if value.len() < MIN_SECRET_LEN {
        return;
    }
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !loaded.iter().any(|v| v.as_slice() == value.as_bytes()) {
        loaded.push(Zeroizing::new(value.as_bytes().to_vec()));
    }
}

/// The secrets the detector looks for.
#[derive(Default)]
pub struct SecretSet {
    values: Vec<Zeroizing<Vec<u8>>>,
}

impl SecretSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A set of the given values (tests).
    #[must_use]
    pub fn from_values(values: &[&str]) -> Self {
        let mut set = Self::new();
        for v in values {
            set.add(v);
        }
        set
    }

    /// Every secret loaded so far.
    #[must_use]
    pub fn loaded() -> Self {
        let loaded = LOADED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            values: loaded.clone(),
        }
    }

    /// Adds a value (ignored when shorter than [`MIN_SECRET_LEN`]).
    pub fn add(&mut self, value: &str) {
        if value.len() >= MIN_SECRET_LEN {
            self.values.push(Zeroizing::new(value.as_bytes().to_vec()));
        }
    }

    /// Adds every value of `other`.
    pub fn extend(&mut self, other: &Self) {
        for v in &other.values {
            if !self.values.iter().any(|x| x.as_slice() == v.as_slice()) {
                self.values.push(v.clone());
            }
        }
    }

    /// Whether the set has no value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl fmt::Debug for SecretSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretSet({} values)", self.values.len())
    }
}

// ---------------------------------------------------------------------------------------------
// Views: the text as the server may have encoded it, decoded back
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dec {
    Percent,
    PercentPlus,
    Html,
    Json,
}

const DECODERS: [Dec; 4] = [Dec::Percent, Dec::PercentPlus, Dec::Html, Dec::Json];

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A decoded text with, for each byte, the interval of the original text it comes from.
struct View {
    bytes: Vec<u8>,
    /// `None`: identity (the original text).
    spans: Option<Vec<(u32, u32)>>,
    chain: Vec<Dec>,
}

impl View {
    fn original(text: &[u8]) -> Self {
        Self {
            bytes: text.to_vec(),
            spans: None,
            chain: Vec::new(),
        }
    }

    /// Interval of the original text covered by the bytes `from..to` of this view.
    fn span(&self, from: usize, to: usize) -> (usize, usize) {
        match &self.spans {
            None => (from, to),
            Some(spans) => {
                let slice = &spans[from..to];
                let start = slice.iter().map(|s| s.0).min().unwrap_or(0);
                let end = slice.iter().map(|s| s.1).max().unwrap_or(0);
                (start as usize, end as usize)
            }
        }
    }

    fn at(&self, i: usize) -> (u32, u32) {
        match &self.spans {
            None => (to_u32(i), to_u32(i + 1)),
            Some(s) => s[i],
        }
    }
}

fn hex_val(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

fn decode_html(b: &[u8]) -> Option<(Vec<u8>, usize)> {
    if b[0] != b'&' {
        return None;
    }
    let end = b.iter().take(12).position(|c| *c == b';')?;
    let name = std::str::from_utf8(&b[1..end]).ok()?;
    let decoded: char = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    let mut out = Vec::new();
    push_char(&mut out, decoded);
    Some((out, end + 1))
}

fn decode_json(b: &[u8]) -> Option<(Vec<u8>, usize)> {
    if b[0] != b'\\' {
        return None;
    }
    let simple = |c: u8| Some((vec![c], 2));
    match *b.get(1)? {
        b'/' => simple(b'/'),
        b'"' => simple(b'"'),
        b'\\' => simple(b'\\'),
        b'b' => simple(8),
        b'f' => simple(12),
        b'n' => simple(b'\n'),
        b'r' => simple(b'\r'),
        b't' => simple(b'\t'),
        b'u' => {
            let unit = |at: usize| -> Option<u32> {
                let digits = b.get(at..at + 4)?;
                let mut v = 0u32;
                for d in digits {
                    v = v * 16 + u32::from(hex_val(*d)?);
                }
                Some(v)
            };
            let first = unit(2)?;
            let mut out = Vec::new();
            if (0xD800..0xDC00).contains(&first)
                && b.get(6) == Some(&b'\\')
                && b.get(7) == Some(&b'u')
                && let Some(second) = unit(8)
                && (0xDC00..0xE000).contains(&second)
            {
                let code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                push_char(&mut out, char::from_u32(code)?);
                return Some((out, 12));
            }
            push_char(&mut out, char::from_u32(first)?);
            Some((out, 6))
        }
        _ => None,
    }
}

/// Decodes one escape at the start of `b`; `None` when there is none.
fn decode_at(b: &[u8], dec: Dec) -> Option<(Vec<u8>, usize)> {
    match dec {
        Dec::Percent | Dec::PercentPlus => {
            if b[0] == b'%'
                && let (Some(hi), Some(lo)) = (
                    b.get(1).copied().and_then(hex_val),
                    b.get(2).copied().and_then(hex_val),
                )
            {
                return Some((vec![hi * 16 + lo], 3));
            }
            (dec == Dec::PercentPlus && b[0] == b'+').then(|| (vec![b' '], 1))
        }
        Dec::Html => decode_html(b),
        Dec::Json => decode_json(b),
    }
}

fn trigger(bytes: &[u8], dec: Dec) -> bool {
    match dec {
        Dec::Percent => bytes.contains(&b'%'),
        Dec::PercentPlus => bytes.contains(&b'%') || bytes.contains(&b'+'),
        Dec::Html => bytes.contains(&b'&'),
        Dec::Json => bytes.contains(&b'\\'),
    }
}

/// The view decoded once more, or `None` when it holds nothing to decode.
fn decode(view: &View, dec: Dec) -> Option<View> {
    if !trigger(&view.bytes, dec) {
        return None;
    }
    let b = &view.bytes;
    let mut out = Vec::with_capacity(b.len());
    let mut spans = Vec::with_capacity(b.len());
    let mut changed = false;
    let mut i = 0;
    while i < b.len() {
        if let Some((decoded, used)) = decode_at(&b[i..], dec) {
            changed = true;
            let (s, e) = view.span(i, i + used);
            for byte in decoded {
                out.push(byte);
                spans.push((to_u32(s), to_u32(e)));
            }
            i += used;
        } else {
            out.push(b[i]);
            spans.push(view.at(i));
            i += 1;
        }
    }
    if !changed {
        return None;
    }
    let mut chain = view.chain.clone();
    chain.push(dec);
    Some(View {
        bytes: out,
        spans: Some(spans),
        chain,
    })
}

/// The view without white space (base64 may be wrapped), or `None` when it has none.
fn without_blanks(view: &View) -> Option<View> {
    if !view.bytes.iter().any(u8::is_ascii_whitespace) {
        return None;
    }
    let mut out = Vec::with_capacity(view.bytes.len());
    let mut spans = Vec::with_capacity(view.bytes.len());
    for (i, byte) in view.bytes.iter().enumerate() {
        if !byte.is_ascii_whitespace() {
            out.push(*byte);
            spans.push(view.at(i));
        }
    }
    Some(View {
        bytes: out,
        spans: Some(spans),
        chain: view.chain.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// Needles: the forms of a secret
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Raw,
    Base64,
    Hex,
}

struct Needle {
    bytes: Vec<u8>,
    kind: Kind,
}

/// Shortest base64 stable part searched for (one group of four characters).
const MIN_STABLE_B64: usize = 4;

fn needles(secret: &[u8]) -> Vec<Needle> {
    let mut list = vec![Needle {
        bytes: secret.to_vec(),
        kind: Kind::Raw,
    }];
    for alignment in 0..3usize {
        let mut input = vec![0u8; alignment];
        input.extend_from_slice(secret);
        // Groups of three input bytes that lie entirely inside the secret encode to four
        // characters that no neighbour can change.
        let first = alignment.div_ceil(3);
        let last = (alignment + secret.len()) / 3;
        if last <= first || (last - first) * 4 < MIN_STABLE_B64 {
            continue;
        }
        for encoded in [
            STANDARD_NO_PAD.encode(&input),
            URL_SAFE_NO_PAD.encode(&input),
        ] {
            list.push(Needle {
                bytes: encoded.as_bytes()[first * 4..last * 4].to_vec(),
                kind: Kind::Base64,
            });
        }
    }
    let lower = crate::secret::hex(secret);
    list.push(Needle {
        bytes: lower.to_ascii_uppercase().into_bytes(),
        kind: Kind::Hex,
    });
    list.push(Needle {
        bytes: lower.into_bytes(),
        kind: Kind::Hex,
    });
    list
}

/// Calls `found(start, end)` for every occurrence of `needle` in `hay`; `found` returns `true` to
/// stop.
fn find_all(hay: &[u8], needle: &[u8], mut found: impl FnMut(usize, usize) -> bool) {
    let Some(first) = needle.first() else { return };
    let mut from = 0;
    while hay.len() >= from + needle.len() {
        let Some(offset) = hay[from..=hay.len() - needle.len()]
            .iter()
            .position(|b| b == first)
        else {
            return;
        };
        let at = from + offset;
        if &hay[at..at + needle.len()] == needle && found(at, at + needle.len()) {
            return;
        }
        from = at + 1;
    }
}

/// A place where a form of a secret was found, in the coordinates of the original text.
struct Hit {
    start: usize,
    end: usize,
    kind: Kind,
    chain: Vec<Dec>,
    secret: usize,
}

/// Looks in one view; returns `true` when the scan must stop.
fn scan_view(view: &View, all: &[Vec<Needle>], first_only: bool, hits: &mut Vec<Hit>) -> bool {
    let stripped = without_blanks(view);
    for (secret, list) in all.iter().enumerate() {
        for needle in list {
            let mut search = |v: &View| -> bool {
                let mut stop = false;
                find_all(&v.bytes, &needle.bytes, |a, b| {
                    let (start, end) = v.span(a, b);
                    hits.push(Hit {
                        start,
                        end,
                        kind: needle.kind,
                        chain: v.chain.clone(),
                        secret,
                    });
                    stop = first_only;
                    stop
                });
                stop
            };
            let mut stop = search(view);
            // Blanks are ignored for the encodings that may be wrapped, not for the raw form.
            if !stop
                && needle.kind != Kind::Raw
                && let Some(s) = &stripped
            {
                stop = search(s);
            }
            if stop {
                return true;
            }
        }
    }
    false
}

/// Looks for every form of every secret. With `first_only` it stops at the first hit.
fn scan(text: &[u8], secrets: &SecretSet, first_only: bool) -> Vec<Hit> {
    let all: Vec<Vec<Needle>> = secrets.values.iter().map(|s| needles(s)).collect();
    let mut hits = Vec::new();
    if all.is_empty() {
        return hits;
    }
    let root = View::original(text);
    if scan_view(&root, &all, first_only, &mut hits) {
        return hits;
    }
    for first in DECODERS {
        let Some(v1) = decode(&root, first) else {
            continue;
        };
        if scan_view(&v1, &all, first_only, &mut hits) {
            return hits;
        }
        for second in DECODERS {
            if let Some(v2) = decode(&v1, second)
                && scan_view(&v2, &all, first_only, &mut hits)
            {
                return hits;
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------------------------
// Widening of a mask to the run of the alphabet
// ---------------------------------------------------------------------------------------------

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// The characters a mask is widened over: the alphabet of the form that was found.
struct Alphabet<'a> {
    kind: Kind,
    chain: &'a [Dec],
    secret: &'a [u8],
}

impl Alphabet<'_> {
    fn percent(&self) -> bool {
        self.chain
            .iter()
            .any(|d| matches!(d, Dec::Percent | Dec::PercentPlus))
    }

    fn single(&self, b: u8) -> bool {
        unreserved(b)
            || (self.kind == Kind::Raw && self.secret.contains(&b))
            || (self.kind == Kind::Base64 && matches!(b, b'+' | b'/' | b'='))
            || (self.chain.contains(&Dec::PercentPlus) && b == b'+')
    }

    /// Length of the token that starts at `at`.
    fn token_at(&self, t: &[u8], at: usize) -> Option<usize> {
        let b = t[at];
        if self.single(b) {
            return Some(1);
        }
        if self.percent()
            && b == b'%'
            && t.get(at + 1).copied().and_then(hex_val).is_some()
            && t.get(at + 2).copied().and_then(hex_val).is_some()
        {
            return Some(3);
        }
        if self.chain.contains(&Dec::Html)
            && b == b'&'
            && let Some(end) = t[at..].iter().take(12).position(|c| *c == b';')
        {
            return Some(end + 1);
        }
        if self.chain.contains(&Dec::Json)
            && b == b'\\'
            && let Some(next) = t.get(at + 1)
        {
            if *next == b'u'
                && t.get(at + 2..at + 6)
                    .is_some_and(|d| d.iter().all(u8::is_ascii_hexdigit))
            {
                return Some(6);
            }
            if matches!(next, b'/' | b'"' | b'\\') {
                return Some(2);
            }
        }
        None
    }

    /// Length of the token that ends at `end` (exclusive).
    fn token_before(&self, t: &[u8], end: usize) -> Option<usize> {
        let b = t[end - 1];
        if self.single(b) {
            return Some(1);
        }
        if end >= 3
            && self.percent()
            && t[end - 3] == b'%'
            && hex_val(t[end - 2]).is_some()
            && hex_val(b).is_some()
        {
            return Some(3);
        }
        if self.chain.contains(&Dec::Html)
            && b == b';'
            && let Some(start) = t[..end].iter().rev().take(12).position(|c| *c == b'&')
        {
            return Some(start + 1);
        }
        if self.chain.contains(&Dec::Json) {
            if end >= 6
                && t[end - 6] == b'\\'
                && t[end - 5] == b'u'
                && t[end - 4..end].iter().all(u8::is_ascii_hexdigit)
            {
                return Some(6);
            }
            if end >= 2 && t[end - 2] == b'\\' && matches!(b, b'/' | b'"' | b'\\') {
                return Some(2);
            }
        }
        None
    }
}

fn widen(text: &[u8], hit: &Hit, secrets: &SecretSet) -> (usize, usize) {
    let alphabet = Alphabet {
        kind: hit.kind,
        chain: &hit.chain,
        secret: &secrets.values[hit.secret],
    };
    let (mut start, mut end) = (hit.start, hit.end);
    loop {
        let mut changed = false;
        if start > 0
            && let Some(n) = alphabet.token_before(text, start)
        {
            start -= n;
            changed = true;
        }
        if end < text.len()
            && let Some(n) = alphabet.token_at(text, end)
        {
            end += n;
            changed = true;
        }
        if !changed {
            return (start, end);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------------------------

/// Text taken from a response (an error message, an excerpt of the body), with every secret
/// replaced by `***`. It is the only form in which such text reaches stderr, the logs, the JSON
/// `error.message`, the history and notifications (N1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedText {
    text: String,
    masked: bool,
}

/// Mask used in place of a secret.
pub const MASK: &str = "***";

impl MaskedText {
    /// Masks the **whole** body, then cuts the result to `limit` characters (`…` is appended when it
    /// was cut). The body is decoded as UTF-8 with replacement first.
    #[must_use]
    pub fn from_response(body: &[u8], secrets: &SecretSet, limit: usize) -> Self {
        let text = String::from_utf8_lossy(body).into_owned();
        let bytes = text.as_bytes();
        let hits = scan(bytes, secrets, false);
        let mut spans: Vec<(usize, usize)> = hits
            .iter()
            .map(|hit| {
                let (s, e) = widen(bytes, hit, secrets);
                (floor_boundary(&text, s), ceil_boundary(&text, e))
            })
            .collect();
        spans.sort_unstable();
        let masked = !spans.is_empty();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (s, e) in spans {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for (s, e) in merged {
            out.push_str(&text[cursor..s]);
            out.push_str(MASK);
            cursor = e;
        }
        out.push_str(&text[cursor..]);
        let cut: String = out.chars().take(limit).collect();
        let text = if cut.len() < out.len() {
            format!("{cut}…")
        } else {
            out
        };
        Self { text, masked }
    }

    /// The masked text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether a secret was found and masked (warning `secret_echoed`).
    #[must_use]
    pub fn was_masked(&self) -> bool {
        self.masked
    }
}

impl fmt::Display for MaskedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn floor_boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// A text that holds a secret was refused by [`CheckedOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the text holds a secret")]
pub struct SecretInOutput;

/// Text that is allowed to leave the program: a URL, a link, a file name taken from a response. The
/// only constructor refuses text in which any form of any secret appears (B1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOutput(String);

impl CheckedOutput {
    /// Checks `text` against `secrets`.
    ///
    /// # Errors
    /// [`SecretInOutput`] when a secret appears in `text`, in any form.
    pub fn check(text: impl Into<String>, secrets: &SecretSet) -> Result<Self, SecretInOutput> {
        let text = text.into();
        if scan(text.as_bytes(), secrets, true).is_empty() {
            Ok(Self(text))
        } else {
            Err(SecretInOutput)
        }
    }

    /// The checked text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CheckedOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};

    use super::{CheckedOutput, MaskedText, SecretSet};

    const TOKEN: &str = concat!("Zq7!", "kLm9#", "Xt2+/vRw");

    fn set() -> SecretSet {
        SecretSet::from_values(&[TOKEN])
    }

    fn pct(s: &str, lower: bool) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(b));
            } else if lower {
                out.push_str(&format!("%{b:02x}"));
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// Every encoded form of `secret` the spec lists.
    fn forms(secret: &str) -> Vec<String> {
        let mut forms = vec![
            secret.to_owned(),
            pct(secret, false),
            pct(secret, true),
            pct(&pct(secret, false), false),
            serde_json::to_string(secret)
                .unwrap()
                .trim_matches('"')
                .to_owned(),
            secret.replace('/', "\\/"),
            secret
                .chars()
                .map(|c| format!("\\u{:04x}", u32::from(c)))
                .collect(),
            secret
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('"', "&quot;")
                .replace('#', "&#35;")
                .replace('!', "&#x21;"),
            crate::secret::hex(secret.as_bytes()),
            crate::secret::hex(secret.as_bytes()).to_uppercase(),
        ];
        for prefix in ["", "a", "ab"] {
            let input = format!("{prefix}{secret}");
            forms.push(STANDARD.encode(&input));
            forms.push(STANDARD_NO_PAD.encode(&input));
            forms.push(URL_SAFE_NO_PAD.encode(&input));
        }
        forms.push(STANDARD.encode(format!("user:{secret}")));
        forms
    }

    #[test]
    fn every_form_is_refused_in_an_output() {
        let set = set();
        for form in forms(TOKEN) {
            let text = format!("https://img.example.org/u/{form}.png");
            assert!(CheckedOutput::check(&text, &set).is_err(), "{form}");
        }
    }

    #[test]
    fn harmless_text_is_accepted() {
        let set = set();
        for text in [
            "https://img.example.org/u/abc.png",
            "![shot.png](https://img.example.org/u/abc.png)",
            "",
        ] {
            assert_eq!(CheckedOutput::check(text, &set).unwrap().as_str(), text);
        }
        assert!(CheckedOutput::check("anything", &SecretSet::new()).is_ok());
    }

    #[test]
    fn wrapped_base64_is_found() {
        let set = set();
        let encoded = STANDARD.encode(format!("user:{TOKEN}"));
        let wrapped = format!("{}\r\n{}", &encoded[..10], &encoded[10..]);
        assert!(CheckedOutput::check(wrapped.clone(), &set).is_err());
        let masked = MaskedText::from_response(wrapped.as_bytes(), &set, 500);
        assert!(masked.was_masked());
        assert!(!masked.as_str().contains(&encoded[12..22]), "{masked}");
    }

    #[test]
    fn masking_replaces_the_whole_run() {
        let set = set();
        let body = format!(r#"{{"error":"bad token {TOKEN} given"}}"#);
        let masked = MaskedText::from_response(body.as_bytes(), &set, 500);
        assert!(masked.was_masked());
        assert!(masked.as_str().contains("bad token"), "{masked}");
        assert!(!masked.as_str().contains("Zq7"), "{masked}");
        assert!(masked.as_str().contains(super::MASK));
    }

    #[test]
    fn no_secret_no_mask_and_cut_is_marked() {
        let set = set();
        let body = "x".repeat(600);
        let masked = MaskedText::from_response(body.as_bytes(), &set, 500);
        assert!(!masked.was_masked());
        assert_eq!(masked.as_str().chars().count(), 501);
        assert!(masked.as_str().ends_with('…'));
    }

    /// N1: the secret is masked on the whole body, so that cutting at 500 characters can never keep
    /// a piece of it, wherever it sits and however it is encoded.
    #[test]
    fn a_secret_placed_around_the_cut_leaves_no_fragment() {
        let set = set();
        let secret_forms: Vec<String> =
            forms(TOKEN).into_iter().filter(|f| f.len() >= 12).collect();
        for form in &secret_forms {
            for position in 495..=530 {
                let mut body = " ".repeat(1000);
                body.replace_range(position..position + form.len(), form);
                let masked = MaskedText::from_response(body.as_bytes(), &set, 500);
                assert!(masked.was_masked(), "{form} at {position}");
                let out = masked.as_str();
                for other in &secret_forms {
                    let chars: Vec<char> = other.chars().collect();
                    for window in chars.windows(4) {
                        let piece: String = window.iter().collect();
                        assert!(
                            !out.contains(&piece),
                            "fragment {piece:?} of {other} survives for {form} at {position}: {out}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn short_secrets_are_ignored_by_the_set() {
        let mut set = SecretSet::new();
        set.add("abc");
        assert!(set.is_empty());
    }

    #[test]
    fn registry_feeds_loaded_sets() {
        super::register_secret("registry-test-secret-0001");
        let loaded = SecretSet::loaded();
        assert!(CheckedOutput::check("x registry-test-secret-0001 y", &loaded).is_err());
    }
}
