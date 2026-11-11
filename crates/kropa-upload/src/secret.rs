// SPDX-License-Identifier: GPL-3.0-or-later

//! Secret values and where they may travel (`docs/security/secrets.md` sections 2.3, 4.1 and 4.3).
//!
//! The store itself is lot 1.8. Here lives the contract the upload engine needs from it
//! ([`SecretSource`]), an in-memory implementation for tests ([`MemorySecretSource`]) and the
//! refusal used by the command line until the store exists ([`UnavailableSecretSource`]).

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Shortest secret accepted (in bytes): shorter ones would match everywhere in the output detector.
pub const MIN_SECRET_LEN: usize = 4;
/// Below this length a secret is accepted with a warning (false positives become possible).
pub const SHORT_SECRET_LEN: usize = 8;

/// A secret value. Its `Debug` and `Display` are `***`; it has no `Serialize` and no `Clone`.
/// The memory is wiped on drop, as far as Rust allows (`docs/security/secrets.md` section 3.5).
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// A secret of at least [`MIN_SECRET_LEN`] bytes.
    ///
    /// # Errors
    /// [`ShortSecret`] when the value is shorter.
    pub fn new(value: impl Into<String>) -> Result<Self, ShortSecret> {
        let value = value.into();
        if value.len() < MIN_SECRET_LEN {
            return Err(ShortSecret);
        }
        Ok(Self(Zeroizing::new(value)))
    }

    /// The value. Keep the borrow short and never log it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false: a secret has at least [`MIN_SECRET_LEN`] bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// A secret of fewer than [`MIN_SECRET_LEN`] bytes was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a secret must have at least {MIN_SECRET_LEN} bytes")]
pub struct ShortSecret;

/// Where in a request a secret may be placed (`docs/security/secrets.md` section 4.3). Hosts and
/// paths of the URL are never allowed (decision U2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Placement {
    /// A request header (name compared without regard to case).
    Header(String),
    /// A query parameter.
    Query(String),
    /// A multipart or urlencoded field.
    Form(String),
    /// The body template, bound to the SHA-256 (hexadecimal) of the template text: changing the
    /// template changes the placement.
    Body(String),
}

impl Placement {
    /// A header placement, name lower-cased.
    #[must_use]
    pub fn header(name: &str) -> Self {
        Self::Header(name.to_ascii_lowercase())
    }

    /// The body placement of a template text.
    #[must_use]
    pub fn body(template: &str) -> Self {
        Self::Body(hex(&Sha256::digest(template.as_bytes())))
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

impl fmt::Display for Placement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header(n) => write!(f, "header:{n}"),
            Self::Query(n) => write!(f, "query:{n}"),
            Self::Form(n) => write!(f, "form:{n}"),
            Self::Body(h) => write!(f, "body:{h}"),
        }
    }
}

impl FromStr for Placement {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let (kind, rest) = s.split_once(':').ok_or(())?;
        if rest.is_empty() {
            return Err(());
        }
        match kind {
            "header" => Ok(Self::header(rest)),
            "query" => Ok(Self::Query(rest.to_owned())),
            "form" => Ok(Self::Form(rest.to_owned())),
            "body" => Ok(Self::Body(rest.to_owned())),
            _ => Err(()),
        }
    }
}

/// One stored secret with the origins and places it is allowed to be sent to.
#[derive(Debug)]
pub struct SecretEntry {
    /// The value.
    pub value: SecretValue,
    /// Canonical origins (`https://host:port`, see [`crate::origin`]).
    pub origins: Vec<String>,
    /// Allowed places.
    pub placements: Vec<Placement>,
}

/// A secret to store, from an import. `value: None` is an empty entry to fill later (`{secret:x}` of a
/// received TOML, `{inputbox:}`): it exists for the profile but holds nothing.
#[derive(Debug)]
pub struct NewSecret {
    /// The value, if the file had one.
    pub value: Option<SecretValue>,
    /// Canonical origins it may be sent to.
    pub origins: Vec<String>,
    /// Places it may be put in.
    pub placements: Vec<Placement>,
}

/// Why a secret could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SecretError {
    /// There is no secret store yet (lot 1.8).
    #[error("the secret store is not available yet")]
    NotDelivered,
    /// The store failed; the text never holds a secret.
    #[error("{0}")]
    Unavailable(String),
}

/// Where the upload engine reads secrets from. The real implementation (keyring, `age` vault) is
/// lot 1.8; an entry is looked up by profile and name only, never across profiles.
pub trait SecretSource: Send + Sync {
    /// The entry `<profile>/<name>`; `Ok(None)` when it does not exist or is still empty.
    ///
    /// # Errors
    /// [`SecretError`] when the store cannot be read.
    fn load(&self, profile: &str, name: &str) -> Result<Option<SecretEntry>, SecretError>;

    /// Whether the store holds any entry `<profile>/*` (an orphan of a removed profile), so that an
    /// import never inherits it (`docs/security/secrets.md` section 5, N2).
    ///
    /// # Errors
    /// [`SecretError`] when the store cannot be read.
    fn profile_exists(&self, profile: &str) -> Result<bool, SecretError>;

    /// Creates the entry `<profile>/<name>`. An import never reuses an entry that exists: it fails if
    /// there is one.
    ///
    /// # Errors
    /// [`SecretError`] when the store cannot be written, or the entry exists.
    fn put(&self, profile: &str, name: &str, entry: NewSecret) -> Result<(), SecretError>;
}

/// The source used until the store exists: nothing to read, and a clear error when a secret is
/// needed.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSecretSource;

impl SecretSource for UnavailableSecretSource {
    fn load(&self, _profile: &str, _name: &str) -> Result<Option<SecretEntry>, SecretError> {
        Err(SecretError::NotDelivered)
    }

    fn profile_exists(&self, _profile: &str) -> Result<bool, SecretError> {
        Ok(false)
    }

    fn put(&self, _profile: &str, _name: &str, _entry: NewSecret) -> Result<(), SecretError> {
        Err(SecretError::NotDelivered)
    }
}

type MemoryKey = (String, String);
type MemoryValue = (Option<String>, Vec<String>, Vec<Placement>);

/// An in-memory store, for tests and examples.
#[derive(Default)]
pub struct MemorySecretSource {
    entries: Mutex<HashMap<MemoryKey, MemoryValue>>,
}

impl MemorySecretSource {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `<profile>/<name>`.
    ///
    /// # Errors
    /// [`ShortSecret`] when the value has fewer than [`MIN_SECRET_LEN`] bytes.
    ///
    /// # Panics
    /// Never in practice: the lock is only poisoned if a test panicked while holding it.
    pub fn insert(
        &self,
        profile: &str,
        name: &str,
        value: &str,
        origins: &[&str],
        placements: Vec<Placement>,
    ) -> Result<(), ShortSecret> {
        if value.len() < MIN_SECRET_LEN {
            return Err(ShortSecret);
        }
        self.entries.lock().expect("lock").insert(
            (profile.to_owned(), name.to_owned()),
            (
                Some(value.to_owned()),
                origins.iter().map(|o| (*o).to_owned()).collect(),
                placements,
            ),
        );
        Ok(())
    }
}

impl SecretSource for MemorySecretSource {
    fn load(&self, profile: &str, name: &str) -> Result<Option<SecretEntry>, SecretError> {
        let entries = self.entries.lock().expect("lock");
        let Some((value, origins, placements)) =
            entries.get(&(profile.to_owned(), name.to_owned()))
        else {
            return Ok(None);
        };
        // An empty entry (created by an import, still to fill) holds no secret.
        let Some(value) = value else { return Ok(None) };
        let value = SecretValue::new(value.clone())
            .map_err(|_| SecretError::Unavailable("secret too short".into()))?;
        Ok(Some(SecretEntry {
            value,
            origins: origins.clone(),
            placements: placements.clone(),
        }))
    }

    fn profile_exists(&self, profile: &str) -> Result<bool, SecretError> {
        let entries = self.entries.lock().expect("lock");
        Ok(entries.keys().any(|(p, _)| p == profile))
    }

    fn put(&self, profile: &str, name: &str, entry: NewSecret) -> Result<(), SecretError> {
        let mut entries = self.entries.lock().expect("lock");
        let key = (profile.to_owned(), name.to_owned());
        if entries.contains_key(&key) {
            return Err(SecretError::Unavailable("the entry already exists".into()));
        }
        entries.insert(
            key,
            (
                entry.value.map(|v| v.expose().to_owned()),
                entry.origins,
                entry.placements,
            ),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySecretSource, Placement, SecretSource, SecretValue};

    #[test]
    fn debug_and_display_never_show_the_value() {
        let s = SecretValue::new("hunter2-hunter2").unwrap();
        assert_eq!(format!("{s:?}"), "***");
        assert_eq!(format!("{s}"), "***");
        assert_eq!(s.expose(), "hunter2-hunter2");
    }

    #[test]
    fn short_secrets_are_refused() {
        assert!(SecretValue::new("abc").is_err());
        assert!(SecretValue::new("abcd").is_ok());
        let store = MemorySecretSource::new();
        assert!(store.insert("p", "n", "abc", &[], vec![]).is_err());
    }

    #[test]
    fn placement_round_trip_and_case() {
        assert_eq!(
            Placement::header("X-Api-Key"),
            "header:x-api-key".parse().unwrap()
        );
        assert_eq!(
            "query:key".parse::<Placement>().unwrap().to_string(),
            "query:key"
        );
        assert!("nothing".parse::<Placement>().is_err());
        assert!("header:".parse::<Placement>().is_err());
        assert_ne!(Placement::body("a"), Placement::body("b"));
    }

    #[test]
    fn memory_store_is_scoped_by_profile() {
        let store = MemorySecretSource::new();
        store
            .insert("zipline", "token", "abcdefgh", &["https://z:443"], vec![])
            .unwrap();
        assert!(store.load("zipline", "token").unwrap().is_some());
        assert!(store.load("other", "token").unwrap().is_none());
        assert!(store.profile_exists("zipline").unwrap());
        assert!(!store.profile_exists("other").unwrap());
    }
}
