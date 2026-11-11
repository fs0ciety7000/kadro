// SPDX-License-Identifier: GPL-3.0-or-later

//! The document of the single Keychain item: `{"v":2,"entries":{"<profile>/<name>":{...}}}` with
//! every entry (`docs/security/secrets.md` section 2.3), and the operations the broker makes on it.
//!
//! The entries themselves are **opaque** here: they are carried as raw JSON, borrowed from the
//! buffer that holds the document, and copied byte for byte into the buffers that are built (all
//! `Zeroizing`, allocated once). Only the envelope (`v`, the keys) is looked at. A secret seen from
//! outside is a document with one entry; [`entries_of`] cuts it out, [`insert`] puts one in.
//!
//! Nothing here quotes the text of a document in an error.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use zeroize::Zeroizing;

use crate::item::{
    Failure, FailureKind, Item, ItemName, MAX_STORE_LEN, check_document, check_part, split_account,
};
use crate::wire::WriteMode;

/// The version this code writes and reads.
pub const VERSION: u8 = 2;

/// A store with no entry: what is written when the item is created, and kept when the last entry
/// is removed (deleting the item and creating it again would reset its access control list).
pub const EMPTY: &[u8] = br#"{"v":2,"entries":{}}"#;

#[derive(Deserialize)]
struct Whole<'a> {
    v: u8,
    #[serde(borrow)]
    entries: BTreeMap<Cow<'a, str>, &'a RawValue>,
}

#[derive(Serialize)]
struct WholeOut<'a> {
    v: u8,
    entries: BTreeMap<&'a str, &'a RawValue>,
}

fn invalid() -> Failure {
    Failure::new(
        FailureKind::StoreError,
        "the stored document is not a valid version 2 document",
    )
}

fn parse(bytes: &[u8]) -> Result<Whole<'_>, Failure> {
    if bytes.len() > MAX_STORE_LEN {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the stored document is too large",
        ));
    }
    let whole: Whole<'_> = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if whole.v != VERSION {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the stored document has an unsupported version",
        ));
    }
    Ok(whole)
}

fn serialize(entries: BTreeMap<&str, &RawValue>, capacity: usize) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut out = Zeroizing::new(Vec::with_capacity(capacity));
    serde_json::to_writer(
        &mut *out,
        &WholeOut {
            v: VERSION,
            entries,
        },
    )
    .map_err(|_| invalid())?;
    if out.len() > MAX_STORE_LEN {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the store is full: too many secrets",
        ));
    }
    Ok(out)
}

/// The document of one entry, as a document of its own.
fn single(key: &str, raw: &RawValue) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let entries = BTreeMap::from([(key, raw)]);
    serialize(entries, raw.get().len() + key.len() + 64)
}

/// The names of every entry (keys that are not `<profile>/<name>` are skipped).
///
/// # Errors
/// [`FailureKind::StoreError`] when the document is not a version 2 document.
pub fn names(whole: &[u8]) -> Result<Vec<ItemName>, Failure> {
    let whole = parse(whole)?;
    Ok(whole
        .entries
        .keys()
        .filter_map(|key| split_account(key))
        .collect())
}

/// The entries of `profile` (or the one called `name`), each as a document of its own.
///
/// # Errors
/// [`FailureKind::StoreError`] when the document is not a version 2 document.
pub fn entries_of(
    whole: &[u8],
    profile: &str,
    name: Option<&str>,
) -> Result<Vec<Item>, Failure> {
    let whole = parse(whole)?;
    let mut out = Vec::new();
    for (key, raw) in &whole.entries {
        let Some(found) = split_account(key) else {
            continue;
        };
        if found.profile != profile || name.is_some_and(|wanted| wanted != found.name) {
            continue;
        }
        out.push(Item {
            name: found.name,
            document: single(key, raw)?,
        });
    }
    Ok(out)
}

/// The store with the entry of `entry` (a document of one entry, keyed `<profile>/<name>`) added or
/// replaced.
///
/// # Errors
/// [`FailureKind::Exists`] for [`WriteMode::Create`] when there is one, [`FailureKind::Missing`] for
/// [`WriteMode::Replace`] when there is none, [`FailureKind::StoreError`] when a document is not
/// valid or `entry` is not exactly the entry `<profile>/<name>`.
pub fn insert(
    whole: &[u8],
    profile: &str,
    name: &str,
    entry: &[u8],
    mode: WriteMode,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    check_part("profile", profile)?;
    check_part("name", name)?;
    check_document(entry)?;
    let key = format!("{profile}/{name}");
    let incoming = parse(entry)?;
    let mut incoming = incoming.entries.into_iter();
    let (Some((found, raw)), None) = (incoming.next(), incoming.next()) else {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the entry to store is not exactly one entry",
        ));
    };
    if found != key {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the entry to store does not match its name",
        ));
    }
    let current = parse(whole)?;
    let exists = current.entries.contains_key(key.as_str());
    match (mode, exists) {
        (WriteMode::Create, true) => {
            return Err(Failure::new(FailureKind::Exists, "the entry already exists"));
        }
        (WriteMode::Replace, false) => return Err(Failure::new(FailureKind::Missing, name)),
        _ => {}
    }
    let mut entries: BTreeMap<&str, &RawValue> =
        current.entries.iter().map(|(k, v)| (&**k, *v)).collect();
    entries.insert(key.as_str(), raw);
    serialize(entries, whole.len() + entry.len() + 64)
}

/// The store without the entry `<profile>/<name>`; `None` when there was none.
///
/// # Errors
/// [`FailureKind::StoreError`] when the document is not a version 2 document.
pub fn delete(
    whole: &[u8],
    profile: &str,
    name: &str,
) -> Result<Option<Zeroizing<Vec<u8>>>, Failure> {
    check_part("profile", profile)?;
    check_part("name", name)?;
    let key = format!("{profile}/{name}");
    let current = parse(whole)?;
    if !current.entries.contains_key(key.as_str()) {
        return Ok(None);
    }
    let entries: BTreeMap<&str, &RawValue> = current
        .entries
        .iter()
        .filter(|(k, _)| k.as_ref() != key)
        .map(|(k, v)| (&**k, *v))
        .collect();
    serialize(entries, whole.len()).map(Some)
}

#[cfg(test)]
mod tests {
    use super::{EMPTY, delete, entries_of, insert, names};
    use crate::item::FailureKind;
    use crate::wire::WriteMode;

    fn entry(key: &str, value: &str) -> Vec<u8> {
        format!(
            r#"{{"v":2,"entries":{{"{key}":{{"origins":["https://z:443"],"placements":["header:authorization"],"created":"1","value":"{value}"}}}}}}"#
        )
        .into_bytes()
    }

    fn store(parts: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut whole = EMPTY.to_vec();
        for (p, n, v) in parts {
            whole = insert(&whole, p, n, &entry(&format!("{p}/{n}"), v), WriteMode::Create)
                .unwrap()
                .to_vec();
        }
        whole
    }

    #[test]
    fn an_empty_store_has_no_names_and_is_valid() {
        assert!(names(EMPTY).unwrap().is_empty());
        assert!(entries_of(EMPTY, "p", None).unwrap().is_empty());
    }

    #[test]
    fn entries_are_added_and_cut_out_one_by_one() {
        let whole = store(&[("zipline", "token", "AAAA-1"), ("s3", "access", "BBBB-2"), ("s3", "secret", "CCCC-3")]);
        let s3 = entries_of(&whole, "s3", None).unwrap();
        assert_eq!(s3.len(), 2);
        let text = String::from_utf8_lossy(&s3[0].document).into_owned();
        assert!(text.starts_with(r#"{"v":2,"entries":{"s3/"#), "{text}");
        assert!(!text.contains("AAAA") && !text.contains("zipline"), "{text}");
        let one = entries_of(&whole, "s3", Some("secret")).unwrap();
        assert_eq!(one.len(), 1);
        assert!(String::from_utf8_lossy(&one[0].document).contains("CCCC-3"));
        assert_eq!(names(&whole).unwrap().len(), 3);
    }

    #[test]
    fn a_profile_that_is_a_prefix_of_another_is_not_confused_with_it() {
        let whole = store(&[("zip", "t", "AAAA-1"), ("zipline", "t", "BBBB-2")]);
        let zip = entries_of(&whole, "zip", None).unwrap();
        assert_eq!(zip.len(), 1);
        assert!(!String::from_utf8_lossy(&zip[0].document).contains("BBBB"));
    }

    #[test]
    fn create_refuses_an_entry_that_exists_and_replace_one_that_does_not() {
        let whole = store(&[("p", "n", "AAAA-1")]);
        let again = insert(&whole, "p", "n", &entry("p/n", "X"), WriteMode::Create);
        assert_eq!(again.unwrap_err().kind, FailureKind::Exists);
        let missing = insert(&whole, "p", "m", &entry("p/m", "X"), WriteMode::Replace);
        assert_eq!(missing.unwrap_err().kind, FailureKind::Missing);
        let replaced = insert(&whole, "p", "n", &entry("p/n", "ZZZZ-9"), WriteMode::Replace).unwrap();
        let text = String::from_utf8_lossy(&replaced).into_owned();
        assert!(text.contains("ZZZZ-9") && !text.contains("AAAA-1"), "{text}");
    }

    #[test]
    fn an_entry_that_is_not_the_one_named_is_refused() {
        let whole = EMPTY;
        for bad in [
            entry("p/other", "x"),
            br#"{"v":2,"entries":{}}"#.to_vec(),
            br#"{"v":2,"entries":{"p/n":{},"p/m":{}}}"#.to_vec(),
            br#"{"v":1,"entries":{"p/n":{}}}"#.to_vec(),
            b"nope".to_vec(),
        ] {
            let e = insert(whole, "p", "n", &bad, WriteMode::Create).unwrap_err();
            assert_eq!(e.kind, FailureKind::StoreError);
        }
    }

    #[test]
    fn delete_removes_one_entry_and_keeps_an_empty_store() {
        let whole = store(&[("p", "n", "AAAA-1"), ("p", "m", "BBBB-2")]);
        let less = delete(&whole, "p", "n").unwrap().unwrap();
        assert_eq!(names(&less).unwrap().len(), 1);
        let none = delete(&less, "p", "m").unwrap().unwrap();
        assert_eq!(&**none, EMPTY);
        assert!(delete(&none, "p", "m").unwrap().is_none());
    }

    #[test]
    fn a_bad_store_document_is_refused_without_quoting_it() {
        for bad in [&b"hunter2-hunter2"[..], br#"{"v":3,"entries":{}}"#, br#"{"v":2,"entries":"hunter2"}"#] {
            let e = names(bad).unwrap_err();
            assert!(!e.detail.contains("hunter2"), "{}", e.detail);
        }
    }

    #[test]
    fn a_value_with_escapes_survives_a_round_trip_byte_for_byte() {
        let doc = br#"{"v":2,"entries":{"p/n":{"origins":[],"placements":[],"value":"a\"b\\cé\/\n"}}}"#;
        let whole = insert(EMPTY, "p", "n", doc, WriteMode::Create).unwrap();
        let back = entries_of(&whole, "p", Some("n")).unwrap();
        assert_eq!(&**back[0].document, &doc[..]);
    }
}
