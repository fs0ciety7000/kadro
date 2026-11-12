// SPDX-License-Identifier: GPL-3.0-or-later

//! The document a secret is stored as (`docs/security/secrets.md` section 2.3, version 2): the value
//! with the origins and places it may be sent to, as JSON.
//!
//! The same shape is the clear text of the `age` vault of lot 2.8, so that a migration copies
//! entries as they are. The keyring holds one document per secret, with one entry in it, keyed
//! `<profile>/<name>`; the key must match the attributes of the keyring item, so an item whose
//! attributes were edited to point at another name is refused.
//!
//! Only the types of this module serialise a secret. Buffers are `Zeroizing`, the document is read
//! with borrowed strings, and no error quotes anything of the text it was given: `serde_json`
//! reports the value it could not read (`invalid type: string "..."`), so its messages are dropped.

use std::borrow::Cow;
use std::collections::BTreeMap;

use kropa_upload::secret::{Placement, SecretValue};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The version this code writes and reads.
pub const VERSION: u8 = 2;

/// Largest document accepted, in bytes: a secret is at most 4 KiB (`docs/cli.md` section 1).
pub const MAX_DOCUMENT_LEN: usize = 64 * 1024;

/// A stored entry, decoded.
#[derive(Debug)]
pub struct Record {
    /// The value; `None` for an entry made by an import and not filled yet.
    pub value: Option<SecretValue>,
    /// Canonical origins.
    pub origins: Vec<String>,
    /// Places.
    pub placements: Vec<Placement>,
    /// When the entry was created (RFC 3339, UTC), `None` if the document did not say.
    pub created: Option<String>,
}

/// Why a document could not be written or read. The texts are fixed: they hold nothing of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// Not JSON of the expected shape.
    #[error("the stored entry is not a valid version 2 document")]
    Invalid,
    /// A version this build does not know.
    #[error("the stored entry has an unsupported version")]
    Version,
    /// The document does not hold exactly the entry the keyring item says it is.
    #[error("the stored entry does not match its name")]
    Mismatch,
    /// Larger than [`MAX_DOCUMENT_LEN`].
    #[error("the stored entry is too large")]
    TooLarge,
    /// The value is shorter than the minimum accepted.
    #[error("the stored secret is too short")]
    ShortValue,
}

#[derive(Serialize)]
struct WriteDocument<'a> {
    v: u8,
    entries: BTreeMap<&'a str, WriteEntry<'a>>,
}

#[derive(Serialize)]
struct WriteEntry<'a> {
    origins: &'a [String],
    placements: Vec<String>,
    created: &'a str,
    value: Option<&'a str>,
}

#[derive(Deserialize)]
struct ReadDocument<'a> {
    v: u8,
    #[serde(borrow)]
    entries: BTreeMap<Cow<'a, str>, ReadEntry<'a>>,
}

#[derive(Deserialize)]
struct ReadEntry<'a> {
    origins: Vec<String>,
    placements: Vec<String>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default, borrow)]
    value: Option<Cow<'a, str>>,
}

/// The document of `<profile>/<name>`, in a buffer that is wiped on drop.
///
/// # Errors
/// [`DocumentError::TooLarge`] when it would exceed [`MAX_DOCUMENT_LEN`].
pub fn encode(
    profile: &str,
    name: &str,
    record: &Record,
    created: &str,
) -> Result<Zeroizing<Vec<u8>>, DocumentError> {
    let key = format!("{profile}/{name}");
    let entry = WriteEntry {
        origins: &record.origins,
        placements: record.placements.iter().map(ToString::to_string).collect(),
        created,
        value: record.value.as_ref().map(SecretValue::expose),
    };
    let document = WriteDocument {
        v: VERSION,
        entries: BTreeMap::from([(key.as_str(), entry)]),
    };
    // Allocated once at the largest size, so that writing never reallocates (and leaves a copy of
    // the value in the freed buffer).
    let mut buffer = Zeroizing::new(Vec::with_capacity(MAX_DOCUMENT_LEN));
    serde_json::to_writer(&mut *buffer, &document).map_err(|_| DocumentError::Invalid)?;
    if buffer.len() > MAX_DOCUMENT_LEN {
        return Err(DocumentError::TooLarge);
    }
    Ok(buffer)
}

/// Reads the document of `<profile>/<name>`.
///
/// # Errors
/// [`DocumentError`] for a document that is too large, not of version 2, not shaped as expected, or
/// that holds anything but the entry `<profile>/<name>`.
pub fn decode(profile: &str, name: &str, bytes: &[u8]) -> Result<Record, DocumentError> {
    if bytes.len() > MAX_DOCUMENT_LEN {
        return Err(DocumentError::TooLarge);
    }
    let document: ReadDocument<'_> =
        serde_json::from_slice(bytes).map_err(|_| DocumentError::Invalid)?;
    if document.v != VERSION {
        return Err(DocumentError::Version);
    }
    let key = format!("{profile}/{name}");
    let mut entries = document.entries.into_iter();
    let (Some((found, entry)), None) = (entries.next(), entries.next()) else {
        return Err(DocumentError::Mismatch);
    };
    if found != key {
        return Err(DocumentError::Mismatch);
    }
    // Borrowed from the buffer: the one copy is the `String` that the `SecretValue` wipes.
    let value = entry
        .value
        .map(|text| SecretValue::new(text.into_owned()).map_err(|_| DocumentError::ShortValue))
        .transpose()?;
    let placements = entry
        .placements
        .iter()
        .map(|p| p.parse::<Placement>().map_err(|()| DocumentError::Invalid))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Record {
        value,
        origins: entry.origins,
        placements,
        created: entry.created,
    })
}

#[cfg(test)]
mod tests {
    use kropa_upload::secret::{Placement, SecretValue};

    use super::{DocumentError, MAX_DOCUMENT_LEN, Record, decode, encode};

    fn record(value: Option<&str>) -> Record {
        Record {
            value: value.map(|v| SecretValue::new(v).unwrap()),
            origins: vec!["https://zipline.example.org:443".to_owned()],
            placements: vec![Placement::header("Authorization")],
            created: None,
        }
    }

    #[test]
    fn round_trip_keeps_value_origins_and_places() {
        let doc = encode(
            "zipline",
            "token",
            &record(Some("abcd-efgh")),
            "2026-11-11T13:00:00Z",
        )
        .unwrap();
        let text = std::str::from_utf8(&doc).unwrap();
        assert!(
            text.starts_with(r#"{"v":2,"entries":{"zipline/token":{"#),
            "{text}"
        );
        let back = decode("zipline", "token", &doc).unwrap();
        assert_eq!(back.value.unwrap().expose(), "abcd-efgh");
        assert_eq!(back.origins, vec!["https://zipline.example.org:443"]);
        assert_eq!(back.placements, vec![Placement::header("authorization")]);
        assert_eq!(back.created.as_deref(), Some("2026-11-11T13:00:00Z"));
    }

    #[test]
    fn an_empty_entry_has_no_value() {
        let doc = encode("p", "n", &record(None), "2026-11-11T13:00:00Z").unwrap();
        assert!(decode("p", "n", &doc).unwrap().value.is_none());
    }

    #[test]
    fn a_value_with_escapes_survives() {
        let value = "ab\"c\\d\u{e9}/\n+";
        let doc = encode("p", "n", &record(Some(value)), "t").unwrap();
        assert_eq!(
            decode("p", "n", &doc).unwrap().value.unwrap().expose(),
            value
        );
    }

    #[test]
    fn the_key_must_match_the_name() {
        let doc = encode("p", "n", &record(Some("abcdefgh")), "t").unwrap();
        assert_eq!(
            decode("p", "other", &doc).unwrap_err(),
            DocumentError::Mismatch
        );
        assert_eq!(decode("q", "n", &doc).unwrap_err(), DocumentError::Mismatch);
        let two = br#"{"v":2,"entries":{"p/n":{"origins":[],"placements":[],"value":null},"p/m":{"origins":[],"placements":[],"value":null}}}"#;
        assert_eq!(decode("p", "n", two).unwrap_err(), DocumentError::Mismatch);
        let none = br#"{"v":2,"entries":{}}"#;
        assert_eq!(decode("p", "n", none).unwrap_err(), DocumentError::Mismatch);
    }

    #[test]
    fn other_versions_and_bad_shapes_are_refused_without_quoting_the_text() {
        let v1 = br#"{"v":1,"entries":{}}"#;
        assert_eq!(decode("p", "n", v1).unwrap_err(), DocumentError::Version);
        // serde_json would say `invalid type: string "hunter2-hunter2", expected a sequence`.
        let bad = br#"{"v":2,"entries":{"p/n":{"origins":"hunter2-hunter2","placements":[]}}}"#;
        let error = decode("p", "n", bad).unwrap_err();
        assert_eq!(error, DocumentError::Invalid);
        assert!(!error.to_string().contains("hunter2"));
        let place = br#"{"v":2,"entries":{"p/n":{"origins":[],"placements":["path:/x"]}}}"#;
        assert_eq!(decode("p", "n", place).unwrap_err(), DocumentError::Invalid);
        assert_eq!(
            decode("p", "n", b"not json").unwrap_err(),
            DocumentError::Invalid
        );
        let short = br#"{"v":2,"entries":{"p/n":{"origins":[],"placements":[],"value":"abc"}}}"#;
        assert_eq!(
            decode("p", "n", short).unwrap_err(),
            DocumentError::ShortValue
        );
    }

    #[test]
    fn a_document_over_the_limit_is_refused() {
        let big = vec![b' '; MAX_DOCUMENT_LEN + 1];
        assert_eq!(decode("p", "n", &big).unwrap_err(), DocumentError::TooLarge);
        let mut r = record(Some("abcdefgh"));
        r.origins = vec!["x".repeat(MAX_DOCUMENT_LEN)];
        assert_eq!(
            encode("p", "n", &r, "t").unwrap_err(),
            DocumentError::TooLarge
        );
    }
}
