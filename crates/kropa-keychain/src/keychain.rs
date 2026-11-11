// SPDX-License-Identifier: GPL-3.0-or-later

//! The real Keychain: the classic file keychain of the user (`kSecClassGenericPassword`), through the
//! safe wrappers of `security-framework`. No `unsafe` here.
//!
//! **One single item**: service [`SERVICE`], account [`STORE_ACCOUNT`], whose secret is the version 2
//! document with every entry ([`crate::store_doc`]). The access control list of a Keychain item is
//! tied to the binary that may use it, and for code that no Apple identity vouches for (ad hoc or
//! self-signed) to its `cdhash`, which changes with every build: after an update macOS asks once
//! **per item** for the new binary. One item per secret would be one prompt per secret; one item is
//! one prompt (decision U1, `docs/security/secrets.md` section 2.2).
//!
//! What follows from it:
//!
//! - A read, even of one secret, reads the whole item (that is the one prompt); only the entries
//!   asked for leave this module.
//! - Creating, replacing or removing an entry is a read-modify-write of the item, made under the
//!   exclusive lock of [`crate::lockfile`] so that two processes (the CLI and the broker) do not lose
//!   each other's writes. The item is **modified in place** (`SecKeychainItemModifyAttributesAndData`,
//!   the item found first), never deleted and created again: that would reset its access control
//!   list and bring the prompt back. When the last entry goes, an empty store stays.
//! - Two items of the same service and account (two keychains of the search list) are
//!   [`FailureKind::Ambiguous`], never a guess.
//! - A call that must not prompt (`list`, and every call when [`ItemStore::set_interactive`] was not
//!   told `true`) runs with `SecKeychainSetUserInteractionAllowed(false)`: what would have asked
//!   fails with [`FailureKind::Locked`] at once, as a locked Secret Service does on Linux.
//! - Every call runs under a deadline ([`crate::deadline`]): 5 s, or 120 s when a prompt may show.
//!
//! Which `OSStatus` the system gives when the user presses "Deny" is **not verified** (it is read as
//! `errSecUserCanceled` or `errSecAuthFailed`); the manual list of `docs/testing/manual-macos.md`
//! records it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use security_framework::base::Error;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::os::macos::keychain_item::SecKeychainItem;
use security_framework::os::macos::passwords::find_generic_password;
use zeroize::Zeroizing;

use crate::deadline;
use crate::item::{
    Failure, FailureKind, Item, ItemName, ItemStore, SERVICE, STORE_ACCOUNT, check_document,
    check_part,
};
use crate::lockfile;
use crate::store_doc;
use crate::wire::WriteMode;

/// `errSecItemNotFound`.
const ITEM_NOT_FOUND: i32 = -25300;

/// A call that shows nothing to the user.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// A call that may show a prompt.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

/// The Keychain of the user, as the [`ItemStore`] of Kropa.
pub struct FileKeychain {
    keychain: Option<SecKeychain>,
    lock_path: PathBuf,
    interactive: AtomicBool,
}

/// What a call needs, cloned onto its thread.
#[derive(Clone)]
struct Target {
    keychain: Option<SecKeychain>,
    lock_path: PathBuf,
    interactive: bool,
}

impl FileKeychain {
    /// The default keychain of the user (the search list, for reading), with the lock file at
    /// `lock_path` (`~/Library/Application Support/io.github.fs0ciety7000.Kropa/store.lock`).
    #[must_use]
    pub fn user_default(lock_path: impl Into<PathBuf>) -> Self {
        Self {
            keychain: None,
            lock_path: lock_path.into(),
            interactive: AtomicBool::new(false),
        }
    }

    /// A given keychain only (tests use a temporary one).
    #[must_use]
    pub fn in_keychain(keychain: SecKeychain, lock_path: impl Into<PathBuf>) -> Self {
        Self {
            keychain: Some(keychain),
            lock_path: lock_path.into(),
            interactive: AtomicBool::new(false),
        }
    }

    fn target(&self, interactive: bool) -> Target {
        Target {
            keychain: self.keychain.clone(),
            lock_path: self.lock_path.clone(),
            interactive,
        }
    }

    /// Runs `call` under a deadline; `quiet` forces "no prompt" whatever the flag says.
    fn run<T, F>(&self, quiet: bool, call: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(&Target) -> Result<T, Failure> + Send + 'static,
    {
        let interactive = self.interactive.load(Ordering::Relaxed) && !quiet;
        let target = self.target(interactive);
        let limit = if interactive {
            PROMPT_TIMEOUT
        } else {
            CALL_TIMEOUT
        };
        deadline::run(limit, move || {
            // Held for the whole call: prompts are off until it ends.
            let _quiet = if target.interactive {
                None
            } else {
                Some(SecKeychain::disable_user_interaction().map_err(failure)?)
            };
            call(&target)
        })
    }

    /// Read-modify-write of the single item under the lock.
    fn update<T, F>(&self, change: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(Option<&[u8]>) -> Result<(Option<Zeroizing<Vec<u8>>>, T), Failure>
            + Send
            + 'static,
    {
        self.run(false, move |target| {
            let _lock = lockfile::acquire(&target.lock_path).map_err(|e| {
                Failure::new(
                    FailureKind::StoreError,
                    format!("cannot take the store lock ({})", e.kind()),
                )
            })?;
            let current = load(target.keychain.as_ref())?;
            let (new, out) = change(current.as_ref().map(|(doc, _)| doc.as_slice()))?;
            if let Some(new) = new {
                match current {
                    Some((_, mut item)) => item.set_password(&new).map_err(failure)?,
                    None => add(target.keychain.as_ref(), &new)?,
                }
            }
            Ok(out)
        })
    }
}

/// What an `OSStatus` of the Keychain means here. The codes carry no secret.
fn failure(error: Error) -> Failure {
    let code = error.code();
    match code {
        // errSecUserCanceled, errSecAuthFailed: the prompt was refused.
        -128 | -25293 => Failure::new(
            FailureKind::Denied,
            "access to the Keychain item was refused",
        ),
        // errSecInteractionNotAllowed, errSecInteractionRequired: a prompt was needed.
        -25308 | -25315 => Failure::new(
            FailureKind::Locked,
            "the keychain is locked or needs to ask for access, and nobody can answer",
        ),
        // errSecNotAvailable, errSecNoSuchKeychain, errSecInvalidKeychain, errSecNoDefaultKeychain.
        -25291 | -25294 | -25295 | -25307 => {
            Failure::new(FailureKind::Unavailable, "no usable keychain")
        }
        _ => Failure::new(
            FailureKind::StoreError,
            format!("the Keychain failed (error {code})"),
        ),
    }
}

fn options(keychain: Option<&SecKeychain>) -> ItemSearchOptions {
    let mut search = ItemSearchOptions::new();
    search
        .class(ItemClass::generic_password())
        .service(SERVICE)
        .account(STORE_ACCOUNT);
    if let Some(keychain) = keychain {
        search.keychains(std::slice::from_ref(keychain));
    }
    search
}

/// How many items answer to the store (attributes only: no secret read, no prompt for access).
fn count(keychain: Option<&SecKeychain>) -> Result<usize, Failure> {
    let mut search = options(keychain);
    search.load_attributes(true).limit(Limit::All);
    match search.search() {
        Ok(found) => Ok(found.len()),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(0),
        Err(e) => Err(failure(e)),
    }
}

/// The document of the store and the item that holds it.
type Stored = (Zeroizing<Vec<u8>>, SecKeychainItem);

/// The stored document and its item; `None` when there is no item.
fn load(keychain: Option<&SecKeychain>) -> Result<Option<Stored>, Failure> {
    match count(keychain)? {
        0 => return Ok(None),
        1 => {}
        n => {
            return Err(Failure::new(
                FailureKind::Ambiguous,
                format!("{n} Keychain items share the store of Kropa"),
            ));
        }
    }
    let found = match keychain {
        Some(keychain) => keychain.find_generic_password(SERVICE, STORE_ACCOUNT),
        None => find_generic_password(None, SERVICE, STORE_ACCOUNT),
    };
    match found {
        Ok((password, item)) => Ok(Some((Zeroizing::new(password.to_vec()), item))),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(failure(e)),
    }
}

/// Creates the item (the first write only).
fn add(keychain: Option<&SecKeychain>, document: &[u8]) -> Result<(), Failure> {
    let keychain = match keychain {
        Some(keychain) => keychain.clone(),
        None => SecKeychain::default().map_err(failure)?,
    };
    keychain
        .add_generic_password(SERVICE, STORE_ACCOUNT, document)
        .map_err(failure)
}

impl ItemStore for FileKeychain {
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure> {
        check_part("profile", profile)?;
        if let Some(name) = name {
            check_part("name", name)?;
        }
        let (profile, name) = (profile.to_owned(), name.map(str::to_owned));
        self.run(false, move |target| match load(target.keychain.as_ref())? {
            None => Ok(Vec::new()),
            Some((document, _)) => store_doc::entries_of(&document, &profile, name.as_deref()),
        })
    }

    fn list(&self) -> Result<Vec<ItemName>, Failure> {
        // Names live inside the secret of the item, so listing reads it: only if that needs no
        // prompt. A store that would have to ask says `locked`, and `status` reports it.
        self.run(true, |target| match load(target.keychain.as_ref())? {
            None => Ok(Vec::new()),
            Some((document, _)) => store_doc::names(&document),
        })
    }

    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        let (profile, name, document) = (
            profile.to_owned(),
            name.to_owned(),
            Zeroizing::new(document.to_vec()),
        );
        self.update(move |current| {
            let base = current.unwrap_or(store_doc::EMPTY);
            let new = store_doc::insert(base, &profile, &name, &document, WriteMode::Create)?;
            Ok((Some(new), ()))
        })
    }

    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        let (profile, name, document) = (
            profile.to_owned(),
            name.to_owned(),
            Zeroizing::new(document.to_vec()),
        );
        self.update(move |current| {
            let Some(current) = current else {
                return Err(Failure::new(FailureKind::Missing, name));
            };
            let new = store_doc::insert(current, &profile, &name, &document, WriteMode::Replace)?;
            Ok((Some(new), ()))
        })
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        let (profile, name) = (profile.to_owned(), name.to_owned());
        self.update(move |current| {
            let Some(current) = current else {
                return Ok((None, false));
            };
            match store_doc::delete(current, &profile, &name)? {
                Some(new) => Ok((Some(new), true)),
                None => Ok((None, false)),
            }
        })
    }

    fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::Relaxed);
    }
}

/// The path of a lock file for tests.
#[cfg(test)]
fn lock_in(dir: &std::path::Path) -> PathBuf {
    dir.join("Kropa").join("store.lock")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};

    use super::{FileKeychain, count, lock_in};
    use crate::item::{FailureKind, ItemStore};

    /// A keychain of its own in a temporary folder: nothing of the user's is touched, and items made by
    /// this very process are used by it without a prompt.
    fn temporary() -> (tempfile::TempDir, SecKeychain, FileKeychain) {
        let dir = tempfile::tempdir().expect("temporary folder");
        let keychain = CreateOptions::new()
            .password("kropa-test-password")
            .create(dir.path().join("kropa-test.keychain"))
            .expect("create a temporary keychain");
        let store = FileKeychain::in_keychain(keychain.clone(), lock_in(dir.path()));
        (dir, keychain, store)
    }

    fn entry(key: &str, value: &str) -> Vec<u8> {
        format!(
            r#"{{"v":2,"entries":{{"{key}":{{"origins":[],"placements":[],"value":"{value}"}}}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn entries_round_trip_and_stay_apart_by_profile() {
        let (_dir, _kc, store) = temporary();
        assert!(store.read("zipline", None).unwrap().is_empty());
        store
            .create("zipline", "token", &entry("zipline/token", "AAAA-1"))
            .unwrap();
        store
            .create("s3", "access", &entry("s3/access", "BBBB-2"))
            .unwrap();
        let zipline = store.read("zipline", None).unwrap();
        assert_eq!(zipline.len(), 1);
        assert!(String::from_utf8_lossy(&zipline[0].document).contains("AAAA-1"));
        assert_eq!(store.read("s3", Some("access")).unwrap().len(), 1);
        assert_eq!(store.list().unwrap().len(), 2);
        assert!(store.remove("s3", "access").unwrap());
        assert!(!store.remove("s3", "access").unwrap());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn every_secret_lives_in_one_single_item_that_is_modified_in_place() {
        let (_dir, kc, store) = temporary();
        for n in 0..5 {
            store
                .create("p", &format!("n{n}"), &entry(&format!("p/n{n}"), "AAAA-1"))
                .unwrap();
        }
        assert_eq!(count(Some(&kc)).unwrap(), 1, "one item for five secrets");
        store.replace("p", "n0", &entry("p/n0", "ZZZZ-9")).unwrap();
        for n in 0..5 {
            store.remove("p", &format!("n{n}")).unwrap();
        }
        // The store stays, empty: deleting it would reset its access control list.
        assert_eq!(count(Some(&kc)).unwrap(), 1);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn create_replace_and_missing_have_the_semantics_of_the_linux_store() {
        let (_dir, _kc, store) = temporary();
        assert_eq!(
            store
                .replace("p", "n", &entry("p/n", "AAAA-1"))
                .unwrap_err()
                .kind,
            FailureKind::Missing
        );
        store.create("p", "n", &entry("p/n", "AAAA-1")).unwrap();
        assert_eq!(
            store
                .create("p", "n", &entry("p/n", "BBBB-2"))
                .unwrap_err()
                .kind,
            FailureKind::Exists
        );
        store.replace("p", "n", &entry("p/n", "BBBB-2")).unwrap();
        let items = store.read("p", Some("n")).unwrap();
        assert!(String::from_utf8_lossy(&items[0].document).contains("BBBB-2"));
    }

    #[test]
    fn names_and_documents_that_are_not_acceptable_are_refused_before_the_keychain() {
        let (_dir, kc, store) = temporary();
        assert!(store.create("a/b", "n", b"{}").is_err());
        assert!(store.read("", None).is_err());
        assert!(store.create("p", "n", &entry("p/other", "x")).is_err());
        assert_eq!(count(Some(&kc)).unwrap(), 0, "nothing was written");
    }

    #[test]
    fn a_call_that_must_not_prompt_gives_the_user_interaction_back() {
        let (_dir, _kc, store) = temporary();
        store.list().unwrap();
        assert!(SecKeychain::user_interaction_allowed().unwrap());
        store.set_interactive(true);
        store.list().unwrap();
        assert!(SecKeychain::user_interaction_allowed().unwrap());
    }

    #[test]
    fn two_writers_do_not_lose_each_others_entries() {
        let (_dir, _kc, store) = temporary();
        let store = Arc::new(store);
        let threads: Vec<_> = (0..2)
            .map(|t| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for n in 0..8 {
                        let name = format!("t{t}n{n}");
                        store
                            .create("p", &name, &entry(&format!("p/{name}"), "AAAA-1"))
                            .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(store.list().unwrap().len(), 16);
    }
}
