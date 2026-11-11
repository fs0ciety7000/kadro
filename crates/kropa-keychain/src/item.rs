// SPDX-License-Identifier: GPL-3.0-or-later

//! The Keychain as Kropa uses it: **one item per secret**, whatever holds the items (the real
//! Keychain, the broker that speaks for it, or the memory of a test).
//!
//! An item is a generic password (`kSecClassGenericPassword`) of service [`SERVICE`] and account
//! `<profile>/<name>`; its secret is the version 2 document of `kropa-store` (`docs/security/secrets.md`
//! section 2.3). This crate treats the document as opaque bytes: it never looks inside.

use std::fmt;

use zeroize::Zeroizing;

/// The service attribute of every item of Kropa (the application ID of the project).
pub const SERVICE: &str = "io.github.fs0ciety7000.Kropa";

/// Longest profile or secret name, in bytes.
pub const MAX_PART_LEN: usize = 128;

/// Largest document accepted, in bytes (the same limit as `kropa-store`'s documents).
pub const MAX_DOCUMENT_LEN: usize = 64 * 1024;

/// A stored secret: its name in the profile, and its document.
pub struct Item {
    /// The secret name (the part of the account after `<profile>/`).
    pub name: String,
    /// The version 2 document. Wiped when dropped.
    pub document: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Item")
            .field("name", &self.name)
            .field("document", &"***")
            .finish()
    }
}

/// The name of a stored secret, without its document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemName {
    /// Profile identifier.
    pub profile: String,
    /// Secret name.
    pub name: String,
}

/// Why an operation on the items failed. Each kind has a fixed `error.reason` of `docs/cli.md`
/// section 4 ([`FailureKind::reason`]), the same as the Linux store where the cause is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureKind {
    /// The user refused the Keychain prompt (exit 4, `keychain_denied`).
    Denied,
    /// The keychain is locked, or the call needed a prompt that nobody can answer (`keyring_locked`).
    Locked,
    /// The Keychain did not answer in time (`keyring_timeout`).
    Timeout,
    /// More than one item answers to the same profile and name (`keyring_ambiguous`).
    Ambiguous,
    /// No usable keychain (`keyring_unavailable`).
    Unavailable,
    /// The item to create exists already.
    Exists,
    /// The item to modify does not exist (`secret_missing`).
    Missing,
    /// Any other failure of the store (`secret_store_error`).
    StoreError,
    /// The broker refused to serve this caller (`keychain_broker_refused`, exit 7).
    BrokerRefused,
    /// The broker cannot be used at all: absent, not signed by the project (`keychain_broker_unavailable`).
    BrokerUnavailable,
}

impl FailureKind {
    /// `error.reason` of the JSON output.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Denied => "keychain_denied",
            Self::Locked => "keyring_locked",
            Self::Timeout => "keyring_timeout",
            Self::Ambiguous => "keyring_ambiguous",
            Self::Unavailable => "keyring_unavailable",
            Self::Missing => "secret_missing",
            Self::BrokerRefused => "keychain_broker_refused",
            Self::BrokerUnavailable => "keychain_broker_unavailable",
            Self::Exists | Self::StoreError => "secret_store_error",
        }
    }

    /// The name of the kind on the broker's wire.
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Locked => "locked",
            Self::Timeout => "timeout",
            Self::Ambiguous => "ambiguous",
            Self::Unavailable => "unavailable",
            Self::Exists => "exists",
            Self::Missing => "missing",
            Self::StoreError => "store_error",
            Self::BrokerRefused => "broker_refused",
            Self::BrokerUnavailable => "broker_unavailable",
        }
    }

    pub(crate) fn from_wire_name(name: &str) -> Option<Self> {
        Some(match name {
            "denied" => Self::Denied,
            "locked" => Self::Locked,
            "timeout" => Self::Timeout,
            "ambiguous" => Self::Ambiguous,
            "unavailable" => Self::Unavailable,
            "exists" => Self::Exists,
            "missing" => Self::Missing,
            "store_error" => Self::StoreError,
            "broker_refused" => Self::BrokerRefused,
            "broker_unavailable" => Self::BrokerUnavailable,
            _ => return None,
        })
    }
}

/// A failed operation: its kind and a text in English that never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
pub struct Failure {
    /// What kind of failure.
    pub kind: FailureKind,
    /// What happened.
    pub detail: String,
}

impl Failure {
    /// A failure of `kind`.
    #[must_use]
    pub fn new(kind: FailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// What the Keychain does for Kropa. Nothing in a method returns more than it must: `read` gives
/// documents of one profile (or one secret), `list` gives names and never a value.
pub trait ItemStore: Send + Sync {
    /// The items of `profile`, or only the one called `name`. Empty when there is none; an error
    /// when two items answer to the same name ([`FailureKind::Ambiguous`]).
    ///
    /// # Errors
    /// [`Failure`] when the keychain cannot be read or the user refuses access.
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure>;

    /// The names of every item of Kropa. Looks at attributes only, never at a secret, and never
    /// asks the user anything.
    ///
    /// # Errors
    /// [`Failure`] when the keychain cannot be listed.
    fn list(&self) -> Result<Vec<ItemName>, Failure>;

    /// Creates the item `<profile>/<name>`; [`FailureKind::Exists`] when there is one.
    ///
    /// # Errors
    /// [`Failure`] when the item cannot be created.
    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure>;

    /// Replaces the document of an item **in place** (never delete and re-create: that resets its
    /// access control list); [`FailureKind::Missing`] when there is none.
    ///
    /// # Errors
    /// [`Failure`] when the item cannot be modified.
    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure>;

    /// Removes the item; `Ok(false)` when there was none.
    ///
    /// # Errors
    /// [`Failure`] when the item cannot be removed.
    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure>;

    /// Whether the keychain may show a prompt (unlock, access to an item). Without it a call that
    /// needs one fails at once with [`FailureKind::Locked`].
    fn set_interactive(&self, _interactive: bool) {}
}

/// The account attribute of `<profile>/<name>`.
#[must_use]
pub fn account(profile: &str, name: &str) -> String {
    format!("{profile}/{name}")
}

/// Splits an account into profile and name; `None` when it is not one of ours.
#[must_use]
pub fn split_account(account: &str) -> Option<ItemName> {
    let (profile, name) = account.split_once('/')?;
    if check_part("profile", profile).is_err() || check_part("name", name).is_err() {
        return None;
    }
    Some(ItemName {
        profile: profile.to_owned(),
        name: name.to_owned(),
    })
}

/// A profile identifier or secret name must be a short, non-empty text without `/` and without
/// control characters (`/` is the separator of the account).
///
/// # Errors
/// [`FailureKind::StoreError`] naming the rule that failed (never the text).
pub fn check_part(what: &str, part: &str) -> Result<(), Failure> {
    let bad = |why: &str| Failure::new(FailureKind::StoreError, format!("invalid {what}: {why}"));
    if part.is_empty() {
        return Err(bad("empty"));
    }
    if part.len() > MAX_PART_LEN {
        return Err(bad("too long"));
    }
    if part.contains('/') {
        return Err(bad("contains '/'"));
    }
    if part.chars().any(char::is_control) {
        return Err(bad("contains a control character"));
    }
    Ok(())
}

/// A document must not exceed [`MAX_DOCUMENT_LEN`].
///
/// # Errors
/// [`FailureKind::StoreError`] when it does.
pub fn check_document(document: &[u8]) -> Result<(), Failure> {
    if document.len() > MAX_DOCUMENT_LEN {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the document is too large",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FailureKind, account, check_part, split_account};

    #[test]
    fn an_account_is_profile_slash_name() {
        assert_eq!(account("zipline", "token"), "zipline/token");
        let n = split_account("zipline/token").unwrap();
        assert_eq!((n.profile.as_str(), n.name.as_str()), ("zipline", "token"));
    }

    #[test]
    fn accounts_that_are_not_ours_are_not_split() {
        for bad in ["", "x", "/n", "p/", "p/n/m", "p/\u{7}n"] {
            assert!(split_account(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn parts_are_checked() {
        assert!(check_part("name", "token").is_ok());
        for bad in ["", "a/b", "a\nb", &"x".repeat(129)] {
            assert!(check_part("name", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_check_failure_never_quotes_the_text() {
        let e = check_part("name", "se/cret").unwrap_err();
        assert!(!e.detail.contains("se/cret"), "{}", e.detail);
    }

    #[test]
    fn kinds_survive_the_wire_and_have_a_reason() {
        let kinds = [
            FailureKind::Denied,
            FailureKind::Locked,
            FailureKind::Timeout,
            FailureKind::Ambiguous,
            FailureKind::Unavailable,
            FailureKind::Exists,
            FailureKind::Missing,
            FailureKind::StoreError,
            FailureKind::BrokerRefused,
            FailureKind::BrokerUnavailable,
        ];
        for kind in kinds {
            assert_eq!(FailureKind::from_wire_name(kind.wire_name()), Some(kind));
            assert!(!kind.reason().is_empty());
        }
        assert_eq!(FailureKind::from_wire_name("other"), None);
    }
}
