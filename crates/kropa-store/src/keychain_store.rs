// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret store of macOS: [`KeychainStore`] turns an [`ItemStore`] of `kropa-keychain` (the
//! Keychain itself, or the broker that speaks for it) into the [`SecretStore`] of the commands, with
//! the semantics of the Linux store ([`crate::secret_service`]): one secret per `<profile>/<name>`,
//! a version 2 document each ([`crate::document`]), the same reasons for the same failures.
//!
//! Nothing here is specific to macOS, so it is tested on every system against
//! [`MemoryItemStore`](kropa_keychain::memory::MemoryItemStore). That the real Keychain keeps **one
//! item** for all the secrets (one prompt per update, decision U1) is `kropa-keychain`'s business
//! ([`kropa_keychain::item`]); seen from here a secret is an entry.

use std::collections::BTreeMap;
use std::sync::Arc;

use kropa_core::Clock;
use kropa_keychain::item::{Failure, FailureKind, ItemStore};
use kropa_upload::secret::{
    Authorization, EntryInfo, NewSecret, Placement, ProfileCount, SecretEntry, SecretError,
    SecretSource, SecretStore, SecretValue, StoreState, StoreStatus,
};
use zeroize::Zeroizing;

use crate::document::{self, DocumentError, Record};

/// The store of `name` (`keychain` or `keychain-broker` in `kropa secrets status`) over `backend`.
pub struct KeychainStore<B> {
    backend: B,
    clock: Arc<dyn Clock + Send + Sync>,
    name: &'static str,
}

impl<B: ItemStore> KeychainStore<B> {
    /// A store over `backend`, called `name` in the status.
    #[must_use]
    pub fn new(backend: B, clock: Arc<dyn Clock + Send + Sync>, name: &'static str) -> Self {
        Self {
            backend,
            clock,
            name,
        }
    }

    fn now(&self) -> String {
        self.clock.now().timestamp().to_string()
    }

    /// The document of `<profile>/<name>`: none, or an error when there are several.
    fn find_one(&self, profile: &str, name: &str) -> Result<Option<Record>, SecretError> {
        let mut items = self.backend.read(profile, Some(name)).map_err(fail)?;
        match items.len() {
            0 => Ok(None),
            1 => {
                let item = items.remove(0);
                document::decode(profile, name, &item.document)
                    .map(Some)
                    .map_err(document_error)
            }
            n => Err(SecretError::Store {
                reason: "keyring_ambiguous",
                detail: format!("{n} entries in the keychain share one profile and name"),
            }),
        }
    }

    fn rewrite(&self, profile: &str, name: &str, record: &Record) -> Result<(), SecretError> {
        let created = record.created.as_deref().unwrap_or_default();
        let document = encode(profile, name, record, created)?;
        self.backend.replace(profile, name, &document).map_err(fail)
    }

    fn create(
        &self,
        profile: &str,
        name: &str,
        record: &Record,
        created: &str,
    ) -> Result<(), SecretError> {
        let document = encode(profile, name, record, created)?;
        self.backend
            .create(profile, name, &document)
            .map_err(|f| match f.kind {
                FailureKind::Exists => SecretError::Unavailable("the entry already exists".into()),
                _ => fail(f),
            })
    }
}

fn fail(failure: Failure) -> SecretError {
    SecretError::Store {
        reason: failure.kind.reason(),
        detail: failure.detail,
    }
}

fn encode(
    profile: &str,
    name: &str,
    record: &Record,
    created: &str,
) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    document::encode(profile, name, record, created).map_err(document_error)
}

fn document_error(error: DocumentError) -> SecretError {
    SecretError::Store {
        reason: "secret_store_error",
        detail: error.to_string(),
    }
}

impl<B: ItemStore> SecretSource for KeychainStore<B> {
    fn load(&self, profile: &str, name: &str) -> Result<Option<SecretEntry>, SecretError> {
        Ok(self.find_one(profile, name)?.and_then(|record| {
            record.value.map(|value| SecretEntry {
                value,
                origins: record.origins,
                placements: record.placements,
            })
        }))
    }

    fn profile_exists(&self, profile: &str) -> Result<bool, SecretError> {
        // Names only: no document is decoded.
        Ok(self
            .backend
            .list()
            .map_err(fail)?
            .iter()
            .any(|n| n.profile == profile))
    }

    fn put(&self, profile: &str, name: &str, entry: NewSecret) -> Result<(), SecretError> {
        let created = self.now();
        let record = Record {
            value: entry.value,
            origins: entry.origins,
            placements: entry.placements,
            created: Some(created.clone()),
        };
        self.create(profile, name, &record, &created)
    }
}

impl<B: ItemStore> SecretStore for KeychainStore<B> {
    fn set_interactive(&self, interactive: bool) {
        self.backend.set_interactive(interactive);
    }

    fn status(&self) -> StoreStatus {
        let report = |state, detail: Option<String>, profiles| StoreStatus {
            store: self.name,
            state,
            detail,
            profiles,
        };
        match self.backend.list() {
            Ok(names) => {
                let mut profiles: BTreeMap<String, ProfileCount> = BTreeMap::new();
                let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
                for n in &names {
                    *seen.entry((&n.profile, &n.name)).or_default() += 1;
                    profiles.entry(n.profile.clone()).or_default().entries += 1;
                }
                let duplicates = seen.values().filter(|c| **c > 1).count();
                if duplicates > 0 {
                    report(
                        StoreState::Ambiguous,
                        Some(format!(
                            "{duplicates} secrets exist more than once in the keychain"
                        )),
                        profiles,
                    )
                } else {
                    report(StoreState::Available, None, profiles)
                }
            }
            Err(failure) => {
                let state = match failure.kind {
                    FailureKind::Locked
                    | FailureKind::VaultLocked
                    | FailureKind::VaultWrongPassphrase => StoreState::Locked,
                    FailureKind::Denied => StoreState::Denied,
                    FailureKind::Ambiguous => StoreState::Ambiguous,
                    _ => StoreState::Unavailable,
                };
                report(state, Some(failure.detail), BTreeMap::new())
            }
        }
    }

    fn names(&self, profile: &str) -> Result<Vec<String>, SecretError> {
        // Names only: no document is decoded.
        let mut names: Vec<String> = self
            .backend
            .list()
            .map_err(fail)?
            .into_iter()
            .filter(|n| n.profile == profile)
            .map(|n| n.name)
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn inspect(&self, profile: &str, name: &str) -> Result<Option<EntryInfo>, SecretError> {
        Ok(self.find_one(profile, name)?.map(|record| EntryInfo {
            has_value: record.value.is_some(),
            origins: record.origins,
            placements: record.placements,
        }))
    }

    fn set(
        &self,
        profile: &str,
        name: &str,
        value: SecretValue,
        fresh: Authorization,
    ) -> Result<(), SecretError> {
        let created = self.now();
        if let Some(mut record) = self.find_one(profile, name)? {
            record.value = Some(value);
            self.rewrite(profile, name, &record)
        } else {
            let record = Record {
                value: Some(value),
                origins: fresh.origins,
                placements: fresh.placements,
                created: Some(created.clone()),
            };
            self.create(profile, name, &record, &created)
        }
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, SecretError> {
        self.backend.remove(profile, name).map_err(fail)
    }

    fn authorize(
        &self,
        profile: &str,
        name: &str,
        origin: Option<&str>,
        placement: Option<Placement>,
    ) -> Result<(), SecretError> {
        let Some(mut record) = self.find_one(profile, name)? else {
            return Err(SecretError::Store {
                reason: "secret_missing",
                detail: name.to_owned(),
            });
        };
        if let Some(origin) = origin
            && !record.origins.iter().any(|o| o == origin)
        {
            record.origins.push(origin.to_owned());
        }
        if let Some(placement) = placement
            && !record.placements.contains(&placement)
        {
            record.placements.push(placement);
        }
        self.rewrite(profile, name, &record)
    }

    fn revoke(
        &self,
        profile: &str,
        name: &str,
        origins: &[String],
        placements: &[Placement],
    ) -> Result<(), SecretError> {
        let Some(mut record) = self.find_one(profile, name)? else {
            return Err(SecretError::Store {
                reason: "secret_missing",
                detail: name.to_owned(),
            });
        };
        record.origins.retain(|o| !origins.contains(o));
        record.placements.retain(|p| !placements.contains(p));
        self.rewrite(profile, name, &record)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kropa_core::FixedClock;
    use kropa_keychain::item::{Failure, FailureKind, ItemStore};
    use kropa_keychain::memory::MemoryItemStore;
    use kropa_upload::secret::{
        Authorization, NewSecret, Placement, SecretError, SecretSource, SecretStore, SecretValue,
        StoreState,
    };

    use super::KeychainStore;

    fn store() -> KeychainStore<MemoryItemStore> {
        let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
        KeychainStore::new(MemoryItemStore::new(), Arc::new(clock), "keychain")
    }

    fn fresh() -> Authorization {
        Authorization {
            origins: vec!["https://z:443".into()],
            placements: vec![Placement::header("Authorization")],
        }
    }

    fn value(v: &str) -> SecretValue {
        SecretValue::new(v).unwrap()
    }

    fn reason(error: &SecretError) -> &'static str {
        match error {
            SecretError::Store { reason, .. } => reason,
            _ => "other",
        }
    }

    #[test]
    fn set_load_inspect_and_remove_round_trip() {
        let s = store();
        assert!(s.load("zipline", "token").unwrap().is_none());
        s.set("zipline", "token", value("abcd-efgh"), fresh())
            .unwrap();
        let entry = s.load("zipline", "token").unwrap().unwrap();
        assert_eq!(entry.value.expose(), "abcd-efgh");
        assert_eq!(entry.origins, ["https://z:443"]);
        let info = s.inspect("zipline", "token").unwrap().unwrap();
        assert!(info.has_value);
        assert!(s.profile_exists("zipline").unwrap());
        assert!(!s.profile_exists("other").unwrap());
        assert!(s.remove("zipline", "token").unwrap());
        assert!(!s.remove("zipline", "token").unwrap());
        assert!(s.inspect("zipline", "token").unwrap().is_none());
    }

    #[test]
    fn an_existing_entry_keeps_its_authorization_when_its_value_changes() {
        let s = store();
        s.set("p", "n", value("abcd-efgh"), fresh()).unwrap();
        let other = Authorization {
            origins: vec!["https://evil:443".into()],
            placements: vec![Placement::Query("k".into())],
        };
        s.set("p", "n", value("ijkl-mnop"), other).unwrap();
        let entry = s.load("p", "n").unwrap().unwrap();
        assert_eq!(entry.value.expose(), "ijkl-mnop");
        assert_eq!(entry.origins, ["https://z:443"]);
        assert_eq!(entry.placements, [Placement::header("authorization")]);
    }

    #[test]
    fn authorize_adds_without_duplicating_and_needs_the_entry() {
        let s = store();
        s.set("p", "n", value("abcd-efgh"), fresh()).unwrap();
        for _ in 0..2 {
            s.authorize(
                "p",
                "n",
                Some("https://n:443"),
                Some(Placement::Query("k".into())),
            )
            .unwrap();
        }
        let info = s.inspect("p", "n").unwrap().unwrap();
        assert_eq!(info.origins.len(), 2);
        assert_eq!(info.placements.len(), 2);
        let e = s.authorize("p", "nope", None, None).unwrap_err();
        assert_eq!(reason(&e), "secret_missing");
    }

    #[test]
    fn revoke_takes_out_what_is_named_and_needs_the_entry() {
        let s = store();
        s.set("p", "n", value("abcd-efgh"), fresh()).unwrap();
        s.authorize(
            "p",
            "n",
            Some("https://n:443"),
            Some(Placement::Query("k".into())),
        )
        .unwrap();
        s.revoke(
            "p",
            "n",
            &["https://z:443".to_owned(), "https://absent:443".to_owned()],
            &[Placement::header("authorization")],
        )
        .unwrap();
        let info = s.inspect("p", "n").unwrap().unwrap();
        assert_eq!(info.origins, ["https://n:443"]);
        assert_eq!(info.placements, [Placement::Query("k".into())]);
        assert!(s.load("p", "n").unwrap().is_some(), "the value stays");
        let e = s.revoke("p", "nope", &[], &[]).unwrap_err();
        assert_eq!(reason(&e), "secret_missing");
    }

    #[test]
    fn put_creates_an_empty_entry_and_never_reuses_one() {
        let s = store();
        let empty = || NewSecret {
            value: None,
            origins: vec!["https://z:443".into()],
            placements: vec![],
        };
        s.put("p", "n", empty()).unwrap();
        assert!(
            s.load("p", "n").unwrap().is_none(),
            "empty: no value to load"
        );
        assert!(!s.inspect("p", "n").unwrap().unwrap().has_value);
        assert!(s.put("p", "n", empty()).is_err());
        s.set("p", "n", value("abcd-efgh"), fresh()).unwrap();
        assert!(s.load("p", "n").unwrap().is_some());
    }

    #[test]
    fn status_counts_names_per_profile_and_never_a_value() {
        let s = store();
        s.set("s3", "access", value("AAAA-1"), fresh()).unwrap();
        s.set("s3", "secret", value("BBBB-2"), fresh()).unwrap();
        s.set("z", "token", value("CCCC-3"), fresh()).unwrap();
        let status = s.status();
        assert_eq!(status.store, "keychain");
        assert_eq!(status.state, StoreState::Available);
        assert_eq!(status.profiles["s3"].entries, 2);
        assert_eq!(status.profiles["z"].entries, 1);
        assert!(!format!("{status:?}").contains("AAAA"));
    }

    #[test]
    fn names_lists_the_entries_of_one_profile_and_nothing_else() {
        let s = store();
        s.set("s3", "secret", value("BBBB-2"), fresh()).unwrap();
        s.set("s3", "access", value("AAAA-1"), fresh()).unwrap();
        s.set("z", "token", value("CCCC-3"), fresh()).unwrap();
        s.put(
            "s3",
            "empty",
            NewSecret {
                value: None,
                origins: vec![],
                placements: vec![],
            },
        )
        .unwrap();
        assert_eq!(s.names("s3").unwrap(), ["access", "empty", "secret"]);
        assert_eq!(s.names("z").unwrap(), ["token"]);
        assert_eq!(s.names("nobody").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn failures_keep_the_reasons_of_the_linux_store() {
        for (kind, expected, state) in [
            (FailureKind::Locked, "keyring_locked", StoreState::Locked),
            (
                FailureKind::Timeout,
                "keyring_timeout",
                StoreState::Unavailable,
            ),
            (
                FailureKind::Ambiguous,
                "keyring_ambiguous",
                StoreState::Ambiguous,
            ),
            (
                FailureKind::Unavailable,
                "keyring_unavailable",
                StoreState::Unavailable,
            ),
            (FailureKind::Denied, "keychain_denied", StoreState::Denied),
            (
                FailureKind::BrokerRefused,
                "keychain_broker_refused",
                StoreState::Unavailable,
            ),
            (
                FailureKind::BrokerUnavailable,
                "keychain_broker_unavailable",
                StoreState::Unavailable,
            ),
            (
                FailureKind::StoreError,
                "secret_store_error",
                StoreState::Unavailable,
            ),
        ] {
            let s = store();
            s.backend.fail_with(Failure::new(kind, "why"));
            let error = s.load("p", "n").unwrap_err();
            assert_eq!(reason(&error), expected);
            assert_eq!(s.status().state, state, "{expected}");
            assert_eq!(s.status().detail.as_deref(), Some("why"));
        }
    }

    #[test]
    fn two_items_under_one_name_are_ambiguous_never_a_guess() {
        let s = store();
        s.set("p", "n", value("abcd-efgh"), fresh()).unwrap();
        let doc = s.backend.document("p", "n").unwrap();
        s.backend.duplicate("p", "n", &doc);
        assert_eq!(reason(&s.load("p", "n").unwrap_err()), "keyring_ambiguous");
        assert_eq!(s.status().state, StoreState::Ambiguous);
    }

    #[test]
    fn a_document_that_is_not_the_entry_it_is_filed_under_is_refused() {
        let s = store();
        s.backend
            .create(
                "p",
                "n",
                br#"{"v":2,"entries":{"p/other":{"origins":[],"placements":[]}}}"#,
            )
            .unwrap();
        assert_eq!(reason(&s.load("p", "n").unwrap_err()), "secret_store_error");
    }

    #[test]
    fn the_interactive_flag_reaches_the_backend() {
        let s = store();
        s.set_interactive(true);
        assert!(s.backend.interactive());
    }

    #[test]
    fn the_stored_document_holds_the_value_and_nothing_leaks_into_the_status_or_errors() {
        let s = store();
        s.set("p", "n", value("hunter2-hunter2"), fresh()).unwrap();
        let doc = String::from_utf8(s.backend.document("p", "n").unwrap()).unwrap();
        assert!(doc.contains("hunter2-hunter2") && doc.contains(r#""v":2"#));
        s.backend
            .fail_with(Failure::new(FailureKind::StoreError, "failed"));
        let error = s.load("p", "n").unwrap_err();
        assert!(!format!("{error:?}{:?}", s.status()).contains("hunter2"));
    }
}
