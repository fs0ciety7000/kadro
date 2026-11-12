// SPDX-License-Identifier: GPL-3.0-or-later

//! The system keyring on Linux: the Secret Service over D-Bus (GNOME Keyring, KWallet of Plasma 6,
//! KeePassXC), through the `secret-service` crate, which needs no C library (RustCrypto for the
//! encrypted session).
//!
//! **One item per secret** in the default collection. Attributes (ours; a service may add its own,
//! such as `xdg:schema` for GNOME Keyring):
//!
//! | attribute | value |
//! |---|---|
//! | `service` | [`SERVICE`] |
//! | `profile` | the profile identifier |
//! | `name` | the name of the secret |
//!
//! The label is `Kropa: <profile>/<name>`. The secret of the item is the version 2 document of
//! [`crate::document`] (value, origins, places). Nothing else is written.
//!
//! Why not `keyring` 4.2 and its Secret Service store: that store searches and then calls
//! `unlock_all` on what it found, which shows a prompt and waits for a person, and its API cannot be
//! told not to. Here the search result is looked at first: a locked item with no way to ask the user
//! fails at once with `keyring_locked`. The store is the one `keyring` wraps, so the dependency tree
//! is the same.
//!
//! Every call to the service runs on its own thread with a deadline ([`Timeouts`]): a service that
//! never answers ends as `keyring_timeout`, a person who never answers a prompt too. A thread that
//! missed its deadline is left to end with the process, which exits right after reporting.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kropa_core::Clock;
use kropa_upload::secret::{
    Authorization, EntryInfo, NewSecret, Placement, ProfileCount, SecretEntry, SecretError,
    SecretSource, SecretStore, SecretValue, StoreState, StoreStatus,
};
use secret_service::blocking::{Collection, Item, SecretService};
use secret_service::{EncryptionType, Error as SsError};
use zeroize::Zeroizing;

use crate::document::{self, DocumentError, Record};

/// The `service` attribute of every item of Kropa.
pub const SERVICE: &str = "io.github.fs0ciety7000.Kropa";

/// Content type recorded on the items.
const CONTENT_TYPE: &str = "application/json";

/// Deadlines of a call to the Secret Service.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// A call that shows nothing to the user (`docs/security/secrets.md` section 2.1: 5 s).
    pub call: Duration,
    /// Once a prompt is shown (120 s).
    pub prompt: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            call: Duration::from_secs(5),
            prompt: Duration::from_secs(120),
        }
    }
}

/// What the worker thread tells the caller besides the result.
enum Event<T> {
    /// A prompt is about to be shown: the deadline becomes the longer one.
    Prompting,
    Done(Result<T, SecretError>),
}

/// The keyring of the user, through the Secret Service.
pub struct SecretServiceStore {
    clock: Arc<dyn Clock + Send + Sync>,
    interactive: AtomicBool,
    timeouts: Timeouts,
}

impl SecretServiceStore {
    /// A store that connects to the session bus at each call (nothing is opened until then).
    #[must_use]
    pub fn new(clock: Arc<dyn Clock + Send + Sync>) -> Self {
        Self::with_timeouts(clock, Timeouts::default())
    }

    /// Like [`Self::new`] with other deadlines (tests).
    #[must_use]
    pub fn with_timeouts(clock: Arc<dyn Clock + Send + Sync>, timeouts: Timeouts) -> Self {
        Self {
            clock,
            interactive: AtomicBool::new(false),
            timeouts,
        }
    }

    fn now(&self) -> String {
        self.clock.now().timestamp().to_string()
    }

    /// Runs `op` on a worker thread, with a connection of its own, under the deadlines.
    fn run<T, F>(&self, op: F) -> Result<T, SecretError>
    where
        T: Send + 'static,
        F: FnOnce(&Session<'_, T>) -> Result<T, SecretError> + Send + 'static,
    {
        let interactive = self.interactive.load(Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Event<T>>();
        let worker = std::thread::Builder::new()
            .name("kropa-secret-service".to_owned())
            .spawn(move || {
                let outcome = SecretService::connect(EncryptionType::Dh)
                    .map_err(|e| map_error(&e, Stage::Connect))
                    .and_then(|service| {
                        op(&Session {
                            service: &service,
                            interactive,
                            events: &tx,
                        })
                    });
                // The caller may have given up; nobody to tell then.
                let _ = tx.send(Event::Done(outcome));
            });
        if worker.is_err() {
            return Err(store_error(
                "secret_store_error",
                "could not start a thread",
            ));
        }
        let mut deadline = Instant::now() + self.timeouts.call;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(Event::Prompting) => deadline = Instant::now() + self.timeouts.prompt,
                Ok(Event::Done(result)) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(store_error(
                        "keyring_timeout",
                        "the Secret Service did not answer in time",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(store_error(
                        "secret_store_error",
                        "the Secret Service call ended without an answer",
                    ));
                }
            }
        }
    }
}

fn store_error(reason: &'static str, detail: &str) -> SecretError {
    SecretError::Store {
        reason,
        detail: detail.to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Reaching the service and opening the encrypted session.
    Connect,
    /// Any later call.
    Call,
}

/// The errors of `secret-service` and what they mean here. Their texts name D-Bus objects and
/// methods, never a secret.
fn map_error(error: &SsError, stage: Stage) -> SecretError {
    match error {
        SsError::Unavailable => store_error(
            "keyring_unavailable",
            "no Secret Service on the session bus",
        ),
        SsError::Locked => store_error("keyring_locked", "the keyring is locked"),
        SsError::Prompt => store_error(
            "keyring_locked",
            "the unlock prompt was dismissed or refused",
        ),
        SsError::Crypto(_) => store_error(
            "keyring_unavailable",
            "the Secret Service refused an encrypted session",
        ),
        // The text of the error names the address of the bus on this machine: not worth showing.
        _ if stage == Stage::Connect => store_error(
            "keyring_unavailable",
            "the Secret Service could not be reached on the session bus",
        ),
        other => SecretError::Store {
            reason: "secret_store_error",
            detail: format!("the Secret Service failed: {other}"),
        },
    }
}

/// An open connection and what the operation may do with it.
struct Session<'a, T> {
    service: &'a SecretService<'a>,
    interactive: bool,
    events: &'a mpsc::Sender<Event<T>>,
}

fn call<R>(result: Result<R, SsError>) -> Result<R, SecretError> {
    result.map_err(|e| map_error(&e, Stage::Call))
}

fn attributes<'a>(profile: Option<&'a str>, name: Option<&'a str>) -> HashMap<&'a str, &'a str> {
    let mut map = HashMap::from([("service", SERVICE)]);
    if let Some(profile) = profile {
        map.insert("profile", profile);
    }
    if let Some(name) = name {
        map.insert("name", name);
    }
    map
}

impl<'a, T> Session<'a, T> {
    /// May show a prompt: tells the caller to wait longer, or refuses when nobody can answer.
    fn may_prompt(&self) -> Result<(), SecretError> {
        if !self.interactive {
            return Err(store_error(
                "keyring_locked",
                "the keyring is locked and there is no way to ask for it to be unlocked",
            ));
        }
        let _ = self.events.send(Event::Prompting);
        Ok(())
    }

    /// The default collection, unlocked (with a prompt if allowed).
    fn collection(&self) -> Result<Collection<'a>, SecretError> {
        let collection = match self.service.get_default_collection() {
            Ok(c) => c,
            Err(SsError::NoResult) => {
                return Err(store_error(
                    "keyring_unavailable",
                    "the Secret Service has no default collection",
                ));
            }
            Err(e) => return Err(map_error(&e, Stage::Call)),
        };
        if call(collection.is_locked())? {
            self.may_prompt()?;
            call(collection.unlock())?;
            if call(collection.is_locked())? {
                return Err(store_error("keyring_locked", "the keyring is still locked"));
            }
        }
        Ok(collection)
    }

    /// The items answering to `<profile>/<name>`, locked or not. Looks, and asks nothing.
    fn find(&self, profile: &str, name: &str) -> Result<Vec<Item<'a>>, SecretError> {
        let found = call(
            self.service
                .search_items(attributes(Some(profile), Some(name))),
        )?;
        Ok(found.unlocked.into_iter().chain(found.locked).collect())
    }

    /// The single item of `<profile>/<name>`: none, or an error when there are several.
    fn find_one(&self, profile: &str, name: &str) -> Result<Option<Item<'a>>, SecretError> {
        let mut items = self.find(profile, name)?;
        match items.len() {
            0 => Ok(None),
            1 => Ok(items.pop()),
            n => Err(SecretError::Store {
                reason: "keyring_ambiguous",
                detail: format!("{n} entries in the keyring share one profile and name"),
            }),
        }
    }

    fn unlock(&self, item: &Item<'_>) -> Result<(), SecretError> {
        if call(item.is_locked())? {
            self.may_prompt()?;
            call(item.unlock())?;
            if call(item.is_locked())? {
                return Err(store_error("keyring_locked", "the keyring is still locked"));
            }
        }
        Ok(())
    }

    fn read(&self, item: &Item<'_>, profile: &str, name: &str) -> Result<Record, SecretError> {
        self.unlock(item)?;
        let bytes = Zeroizing::new(call(item.get_secret())?);
        decode(profile, name, &bytes)
    }

    fn create(
        &'a self,
        profile: &str,
        name: &str,
        record: &Record,
        created: &str,
    ) -> Result<(), SecretError> {
        let document = encode(profile, name, record, created)?;
        let collection = self.collection()?;
        let label = format!("Kropa: {profile}/{name}");
        call(collection.create_item(
            &label,
            attributes(Some(profile), Some(name)),
            &document,
            false,
            CONTENT_TYPE,
        ))?;
        Ok(())
    }

    fn rewrite(
        &self,
        item: &Item<'_>,
        profile: &str,
        name: &str,
        record: &Record,
    ) -> Result<(), SecretError> {
        let created = record.created.as_deref().unwrap_or_default();
        let document = encode(profile, name, record, created)?;
        self.unlock(item)?;
        call(item.set_secret(&document, CONTENT_TYPE))
    }
}

fn decode(profile: &str, name: &str, bytes: &[u8]) -> Result<Record, SecretError> {
    document::decode(profile, name, bytes).map_err(document_error)
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

impl SecretSource for SecretServiceStore {
    fn load(&self, profile: &str, name: &str) -> Result<Option<SecretEntry>, SecretError> {
        let (profile, name) = (profile.to_owned(), name.to_owned());
        self.run(move |s| {
            let Some(item) = s.find_one(&profile, &name)? else {
                return Ok(None);
            };
            let record = s.read(&item, &profile, &name)?;
            Ok(record.value.map(|value| SecretEntry {
                value,
                origins: record.origins,
                placements: record.placements,
            }))
        })
    }

    fn profile_exists(&self, profile: &str) -> Result<bool, SecretError> {
        let profile = profile.to_owned();
        self.run(move |s| {
            let found = call(s.service.search_items(attributes(Some(&profile), None)))?;
            Ok(!(found.unlocked.is_empty() && found.locked.is_empty()))
        })
    }

    fn put(&self, profile: &str, name: &str, entry: NewSecret) -> Result<(), SecretError> {
        let (profile, name, created) = (profile.to_owned(), name.to_owned(), self.now());
        self.run(move |s| {
            if !s.find(&profile, &name)?.is_empty() {
                return Err(SecretError::Unavailable("the entry already exists".into()));
            }
            let record = Record {
                value: entry.value,
                origins: entry.origins,
                placements: entry.placements,
                created: Some(created.clone()),
            };
            s.create(&profile, &name, &record, &created)
        })
    }
}

impl SecretStore for SecretServiceStore {
    fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::Relaxed);
    }

    fn status(&self) -> StoreStatus {
        let report = |state, detail: Option<String>, profiles| StoreStatus {
            store: "keyring",
            state,
            detail,
            profiles,
        };
        let outcome = self.run(|s| {
            // Whether the keyring is there, and whether it would answer without asking.
            let collection = match s.service.get_default_collection() {
                Ok(c) => c,
                Err(SsError::NoResult) => {
                    return Err(store_error(
                        "keyring_unavailable",
                        "the Secret Service has no default collection",
                    ));
                }
                Err(e) => return Err(map_error(&e, Stage::Call)),
            };
            let collection_locked = call(collection.is_locked())?;
            let found = call(s.service.search_items(attributes(None, None)))?;
            let locked_items = found.locked.len();
            let mut profiles: BTreeMap<String, ProfileCount> = BTreeMap::new();
            let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
            let mut unreadable = 0_usize;
            for item in found.unlocked.iter().chain(found.locked.iter()) {
                // The attributes of a locked item may not be readable; that is a state, not a failure.
                let Ok(attrs) = item.get_attributes() else {
                    unreadable += 1;
                    continue;
                };
                let (Some(profile), Some(name)) = (attrs.get("profile"), attrs.get("name")) else {
                    continue;
                };
                if attrs.get("service").map(String::as_str) != Some(SERVICE) {
                    continue;
                }
                *seen.entry((profile.clone(), name.clone())).or_default() += 1;
                profiles.entry(profile.clone()).or_default().entries += 1;
            }
            let duplicates = seen.values().filter(|n| **n > 1).count();
            Ok((
                collection_locked || locked_items > 0 || unreadable > 0,
                duplicates,
                profiles,
            ))
        });
        match outcome {
            Ok((true, _, profiles)) => report(
                StoreState::Locked,
                Some("the keyring is locked".to_owned()),
                profiles,
            ),
            Ok((false, duplicates, profiles)) if duplicates > 0 => report(
                StoreState::Ambiguous,
                Some(format!(
                    "{duplicates} secrets exist more than once in the keyring"
                )),
                profiles,
            ),
            Ok((false, _, profiles)) => report(StoreState::Available, None, profiles),
            Err(SecretError::Store {
                reason: "keyring_locked",
                detail,
            }) => report(StoreState::Locked, Some(detail), BTreeMap::new()),
            Err(SecretError::Store {
                reason: "keyring_ambiguous",
                detail,
            }) => report(StoreState::Ambiguous, Some(detail), BTreeMap::new()),
            Err(error) => report(
                StoreState::Unavailable,
                Some(error.to_string()),
                BTreeMap::new(),
            ),
        }
    }

    fn names(&self, profile: &str) -> Result<Vec<String>, SecretError> {
        let profile = profile.to_owned();
        self.run(move |s| {
            let found = call(s.service.search_items(attributes(Some(&profile), None)))?;
            let mut names = Vec::new();
            for item in found.unlocked.iter().chain(found.locked.iter()) {
                // Without its attributes an entry cannot be named, and silently skipping it would
                // leave it behind when a profile is removed.
                let attrs = call(item.get_attributes())?;
                if attrs.get("service").map(String::as_str) != Some(SERVICE)
                    || attrs.get("profile").map(String::as_str) != Some(profile.as_str())
                {
                    continue;
                }
                if let Some(name) = attrs.get("name") {
                    names.push(name.clone());
                }
            }
            names.sort();
            names.dedup();
            Ok(names)
        })
    }

    fn inspect(&self, profile: &str, name: &str) -> Result<Option<EntryInfo>, SecretError> {
        let (profile, name) = (profile.to_owned(), name.to_owned());
        self.run(move |s| {
            let Some(item) = s.find_one(&profile, &name)? else {
                return Ok(None);
            };
            let record = s.read(&item, &profile, &name)?;
            Ok(Some(EntryInfo {
                has_value: record.value.is_some(),
                origins: record.origins,
                placements: record.placements,
            }))
        })
    }

    fn set(
        &self,
        profile: &str,
        name: &str,
        value: SecretValue,
        fresh: Authorization,
    ) -> Result<(), SecretError> {
        let (profile, name, created) = (profile.to_owned(), name.to_owned(), self.now());
        self.run(move |s| {
            if let Some(item) = s.find_one(&profile, &name)? {
                let mut record = s.read(&item, &profile, &name)?;
                record.value = Some(value);
                s.rewrite(&item, &profile, &name, &record)
            } else {
                let record = Record {
                    value: Some(value),
                    origins: fresh.origins,
                    placements: fresh.placements,
                    created: Some(created.clone()),
                };
                s.create(&profile, &name, &record, &created)
            }
        })
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, SecretError> {
        let (profile, name) = (profile.to_owned(), name.to_owned());
        self.run(move |s| {
            let Some(item) = s.find_one(&profile, &name)? else {
                return Ok(false);
            };
            s.unlock(&item)?;
            call(item.delete())?;
            Ok(true)
        })
    }

    fn authorize(
        &self,
        profile: &str,
        name: &str,
        origin: Option<&str>,
        placement: Option<Placement>,
    ) -> Result<(), SecretError> {
        let (profile, name) = (profile.to_owned(), name.to_owned());
        let origin = origin.map(str::to_owned);
        self.run(move |s| {
            let Some(item) = s.find_one(&profile, &name)? else {
                return Err(SecretError::Store {
                    reason: "secret_missing",
                    detail: name.clone(),
                });
            };
            let mut record = s.read(&item, &profile, &name)?;
            if let Some(origin) = origin
                && !record.origins.contains(&origin)
            {
                record.origins.push(origin);
            }
            if let Some(placement) = placement
                && !record.placements.contains(&placement)
            {
                record.placements.push(placement);
            }
            s.rewrite(&item, &profile, &name, &record)
        })
    }
}
