// SPDX-License-Identifier: GPL-3.0-or-later

//! An [`ItemStore`] in memory, for the tests of the broker, of `kropa-store` and of the commands.
//! It has the semantics of the real keychain that matter to them (one item per account, creation
//! fails when the item exists, replacement fails when it does not) and can be told to fail or to
//! hold two items under one name.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use zeroize::Zeroizing;

use crate::item::{Failure, FailureKind, Item, ItemName, ItemStore, check_document, check_part};

/// Documents by (profile, name); several under one name are a duplicate.
type Documents = BTreeMap<(String, String), Vec<Vec<u8>>>;

/// A keychain in memory.
#[derive(Default)]
pub struct MemoryItemStore {
    items: Mutex<Documents>,
    failure: Mutex<Option<Failure>>,
    interactive: AtomicBool,
    calls: AtomicUsize,
}

impl MemoryItemStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every following call fails with `failure`, until [`Self::heal`].
    ///
    /// # Panics
    /// Never in practice: the lock is only poisoned if a test panicked while holding it.
    pub fn fail_with(&self, failure: Failure) {
        *self.failure.lock().expect("lock") = Some(failure);
    }

    /// Calls work again.
    ///
    /// # Panics
    /// Never in practice (see [`Self::fail_with`]).
    pub fn heal(&self) {
        *self.failure.lock().expect("lock") = None;
    }

    /// Adds a second item under the same name, as two keychains of the search list would.
    ///
    /// # Panics
    /// Never in practice (see [`Self::fail_with`]).
    pub fn duplicate(&self, profile: &str, name: &str, document: &[u8]) {
        self.items
            .lock()
            .expect("lock")
            .entry((profile.to_owned(), name.to_owned()))
            .or_default()
            .push(document.to_vec());
    }

    /// How many calls reached the store, of any kind.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// What [`ItemStore::set_interactive`] was last told.
    #[must_use]
    pub fn interactive(&self) -> bool {
        self.interactive.load(Ordering::SeqCst)
    }

    /// The document of an item, if there is exactly one.
    ///
    /// # Panics
    /// Never in practice (see [`Self::fail_with`]).
    #[must_use]
    pub fn document(&self, profile: &str, name: &str) -> Option<Vec<u8>> {
        let items = self.items.lock().expect("lock");
        let docs = items.get(&(profile.to_owned(), name.to_owned()))?;
        (docs.len() == 1).then(|| docs[0].clone())
    }

    fn enter(&self) -> Result<(), Failure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.failure.lock().expect("lock").clone() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

impl ItemStore for MemoryItemStore {
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure> {
        self.enter()?;
        check_part("profile", profile)?;
        let items = self.items.lock().expect("lock");
        let mut out = Vec::new();
        for ((p, n), docs) in items.iter() {
            if p != profile || name.is_some_and(|wanted| wanted != n) {
                continue;
            }
            if docs.len() > 1 {
                return Err(Failure::new(
                    FailureKind::Ambiguous,
                    "2 entries in the keychain share one profile and name",
                ));
            }
            out.push(Item {
                name: n.clone(),
                document: Zeroizing::new(docs[0].clone()),
            });
        }
        Ok(out)
    }

    fn list(&self) -> Result<Vec<ItemName>, Failure> {
        self.enter()?;
        let items = self.items.lock().expect("lock");
        let mut out = Vec::new();
        for ((p, n), docs) in items.iter() {
            for _ in docs {
                out.push(ItemName {
                    profile: p.clone(),
                    name: n.clone(),
                });
            }
        }
        Ok(out)
    }

    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        self.enter()?;
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        let mut items = self.items.lock().expect("lock");
        let slot = items
            .entry((profile.to_owned(), name.to_owned()))
            .or_default();
        if !slot.is_empty() {
            return Err(Failure::new(
                FailureKind::Exists,
                "the entry already exists",
            ));
        }
        slot.push(document.to_vec());
        Ok(())
    }

    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        self.enter()?;
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        let mut items = self.items.lock().expect("lock");
        match items.get_mut(&(profile.to_owned(), name.to_owned())) {
            Some(docs) if docs.len() == 1 => {
                docs[0] = document.to_vec();
                Ok(())
            }
            Some(docs) if docs.len() > 1 => Err(Failure::new(
                FailureKind::Ambiguous,
                "2 entries in the keychain share one profile and name",
            )),
            _ => Err(Failure::new(FailureKind::Missing, name)),
        }
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure> {
        self.enter()?;
        check_part("profile", profile)?;
        check_part("name", name)?;
        let mut items = self.items.lock().expect("lock");
        let key = (profile.to_owned(), name.to_owned());
        match items.get(&key).map(Vec::len) {
            None | Some(0) => Ok(false),
            Some(1) => {
                items.remove(&key);
                Ok(true)
            }
            Some(_) => Err(Failure::new(
                FailureKind::Ambiguous,
                "2 entries in the keychain share one profile and name",
            )),
        }
    }

    fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryItemStore;
    use crate::item::{FailureKind, ItemStore};

    #[test]
    fn create_replace_read_remove() {
        let s = MemoryItemStore::new();
        s.create("p", "n", b"one").unwrap();
        assert_eq!(
            s.create("p", "n", b"x").unwrap_err().kind,
            FailureKind::Exists
        );
        s.replace("p", "n", b"two").unwrap();
        assert_eq!(
            s.replace("p", "m", b"x").unwrap_err().kind,
            FailureKind::Missing
        );
        let items = s.read("p", None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(&**items[0].document, b"two");
        assert!(s.read("q", None).unwrap().is_empty());
        assert!(s.remove("p", "n").unwrap());
        assert!(!s.remove("p", "n").unwrap());
    }

    #[test]
    fn a_duplicate_is_ambiguous() {
        let s = MemoryItemStore::new();
        s.create("p", "n", b"one").unwrap();
        s.duplicate("p", "n", b"other");
        assert_eq!(
            s.read("p", Some("n")).unwrap_err().kind,
            FailureKind::Ambiguous
        );
        assert_eq!(s.list().unwrap().len(), 2);
    }

    #[test]
    fn a_failure_is_given_until_healed() {
        let s = MemoryItemStore::new();
        s.fail_with(crate::item::Failure::new(FailureKind::Locked, "locked"));
        assert_eq!(s.list().unwrap_err().kind, FailureKind::Locked);
        s.heal();
        assert!(s.list().unwrap().is_empty());
        assert_eq!(s.calls(), 2);
    }
}
