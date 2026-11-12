// SPDX-License-Identifier: GPL-3.0-or-later

//! The `age` vault: the fallback of the secret store when there is no system keyring, chosen by the
//! user (lot 2.8a, `docs/security/secrets.md` section 3).
//!
//! [`VaultItems`] is an [`ItemStore`] over **one file**, `secrets.age`: an `age` file with a single
//! scrypt recipient (so that `age -d` or `rage -d` open it in an emergency) that holds the version 2
//! document of [`kropa_keychain::store_doc`], with every entry. [`open_vault_store`] gives it the
//! semantics of every other store: it is a [`KeychainStore`] named `vault`, so that `names()`, the
//! reasons and the rules of a secret are the ones of the keyring, without a line copied.
//!
//! - **Passphrase**: asked from a [`PassphraseSource`] at most once per [`VaultItems`] (so once per
//!   command), kept in a `Zeroizing` buffer and wiped when the value is dropped, or at once after a
//!   wrong one. [`Purpose::Create`] when the file does not exist yet, [`Purpose::Open`] otherwise.
//! - **File and folder**: `0600` in a `0700` folder, opened `O_NOFOLLOW`, owner and mode read with
//!   `fstat` **on the open descriptor** (never `stat` on the path); anything else is
//!   `vault_unsafe_file`.
//! - **Write**: read-modify-write under the `flock` of `store.lock` (the lock of the macOS Keychain
//!   item, same folder), a temporary file `O_EXCL`, `sync_all`, `rename`, `sync_all` of the folder.
//!   Temporary files older than one hour are removed under the lock. Nothing is ever written from a
//!   cache.
//! - **Work factor**: `log2 N` of scrypt is 18 when writing (a setting), at most 20 when reading (a
//!   setting, 22 at most). The factor is read from the header **before** anything is derived, and
//!   before the passphrase is asked: a file above the limit is refused at once. Below 16 a
//!   [`VaultWarning`] is kept for the command to show.
//! - **Reads take no lock**: the file is replaced by `rename`, a reader sees the old or the new one.
//!
//! Not guarded here: the strength of the passphrase (the source of the commands refuses a weak one,
//! `weak_passphrase`, lot 2.8b), and a process of the same user, who is outside the threat model.

use std::fs::{DirBuilder, File, OpenOptions};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::{self, Read, Write};
use std::iter;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use age::secrecy::SecretString;
use age::{DecryptError, Decryptor, Encryptor, scrypt};
use kropa_core::Clock;
use kropa_keychain::item::{
    Failure, FailureKind, Item, ItemName, ItemStore, MAX_STORE_LEN, check_document, check_part,
};
use kropa_keychain::store_doc::{self, EMPTY};
use kropa_keychain::wire::WriteMode;
use kropa_keychain::{lockfile, system};
use zeroize::Zeroizing;

use crate::keychain_store::KeychainStore;

/// The vault file, in the data folder of Kropa.
pub const FILE_NAME: &str = "secrets.age";

/// The lock file, the same as the one of the macOS Keychain item.
pub const LOCK_NAME: &str = "store.lock";

/// `log2 N` of scrypt when writing, by default (about 256 MiB, portable between machines).
pub const WRITE_WORK_FACTOR: u8 = 18;

/// The highest `log2 N` accepted when reading, by default.
pub const DEFAULT_MAX_WORK_FACTOR: u8 = 20;

/// The highest value the reading limit can be set to (`secrets.vault.max_work_factor`).
pub const LIMIT_WORK_FACTOR: u8 = 22;

/// Below this `log2 N` a vault is called weak, and a [`VaultWarning`] asks to rewrite it.
pub const WEAK_WORK_FACTOR: u8 = 16;

/// A temporary file older than this was left by a crash; it is removed.
const STALE_TEMP_SECS: i64 = 3600;

/// The biggest vault file read: the largest document, its encryption (one 16 byte tag per 64 KiB
/// chunk) and the header, with room to spare.
const MAX_FILE_LEN: u64 = MAX_STORE_LEN as u64 + 64 * 1024;

const MAGIC: &[u8] = b"age-encryption.org/v1\n";

/// Only one scrypt derivation at a time in a process (256 MiB each).
static DERIVING: Mutex<()> = Mutex::new(());

/// Why the passphrase is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The vault exists: its passphrase.
    Open,
    /// The vault does not exist yet: a new passphrase (two entries, a strength check).
    Create,
}

/// Where the passphrase comes from: the terminal, `--password-fd`, a test.
///
/// Called at most once per [`VaultItems`]. The source refuses with a [`FailureKind::VaultLocked`]
/// when nobody can answer (no terminal, no descriptor) and never prompts when it must not.
pub trait PassphraseSource: Send + Sync {
    /// The passphrase.
    ///
    /// # Errors
    /// A [`Failure`]: `VaultLocked` when it cannot be asked, or whatever the source reports.
    fn get(&self, purpose: Purpose) -> Result<Zeroizing<String>, Failure>;
}

/// The work factors of a vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultOptions {
    work_factor: u8,
    max_work_factor: u8,
}

impl Default for VaultOptions {
    fn default() -> Self {
        Self {
            work_factor: WRITE_WORK_FACTOR,
            max_work_factor: DEFAULT_MAX_WORK_FACTOR,
        }
    }
}

impl VaultOptions {
    /// `work_factor` is the `log2 N` of a write, `max_work_factor` the highest one a read accepts.
    ///
    /// # Errors
    /// [`FailureKind::VaultWorkFactor`] when `max_work_factor` is above [`LIMIT_WORK_FACTOR`], or
    /// when what is written could not be read back (`work_factor` above `max_work_factor`).
    pub fn new(work_factor: u8, max_work_factor: u8) -> Result<Self, Failure> {
        if work_factor == 0 || max_work_factor > LIMIT_WORK_FACTOR || work_factor > max_work_factor
        {
            return Err(Failure::new(
                FailureKind::VaultWorkFactor,
                format!(
                    "the work factors {work_factor} (write) and {max_work_factor} (read limit) \
                     are not valid: 1 <= write <= read limit <= {LIMIT_WORK_FACTOR}"
                ),
            ));
        }
        Ok(Self {
            work_factor,
            max_work_factor,
        })
    }

    /// `log2 N` used to write.
    #[must_use]
    pub fn work_factor(self) -> u8 {
        self.work_factor
    }

    /// The highest `log2 N` accepted to read.
    #[must_use]
    pub fn max_work_factor(self) -> u8 {
        self.max_work_factor
    }
}

/// Something the command should tell the user after a successful call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VaultWarning {
    /// The file was written with a work factor below [`WEAK_WORK_FACTOR`]: rewrite it.
    WeakWorkFactor {
        /// The `log2 N` read in the header.
        log_n: u8,
    },
}

/// The folder of the vault of this user: `$XDG_DATA_HOME/kropa` (default `~/.local/share/kropa`),
/// on macOS `~/Library/Application Support/<application ID>`. `None` when the home folder is unknown.
#[must_use]
pub fn default_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let name = if cfg!(target_os = "macos") {
        kropa_keychain::item::SERVICE
    } else {
        "kropa"
    };
    Some(base.data_dir().join(name))
}

/// The vault as a secret store named `vault`.
#[must_use]
pub fn open_vault_store(
    items: VaultItems,
    clock: Arc<dyn Clock + Send + Sync>,
) -> KeychainStore<VaultItems> {
    KeychainStore::new(items, clock, "vault")
}

/// The items of the vault: one `age` file.
pub struct VaultItems {
    dir: PathBuf,
    options: VaultOptions,
    source: Box<dyn PassphraseSource>,
    clock: Arc<dyn Clock + Send + Sync>,
    passphrase: Mutex<Option<Zeroizing<String>>>,
    warnings: Mutex<Vec<VaultWarning>>,
    /// A test makes the write stop before the `rename`, as a crash would.
    #[cfg(test)]
    crash_before_rename: AtomicBool,
}

impl VaultItems {
    /// A vault in `dir` (`secrets.age` and `store.lock` inside). Nothing is touched yet.
    #[must_use]
    pub fn new(
        dir: PathBuf,
        options: VaultOptions,
        source: Box<dyn PassphraseSource>,
        clock: Arc<dyn Clock + Send + Sync>,
    ) -> Self {
        Self {
            dir,
            options,
            source,
            clock,
            passphrase: Mutex::new(None),
            warnings: Mutex::new(Vec::new()),
            #[cfg(test)]
            crash_before_rename: AtomicBool::new(false),
        }
    }

    /// The path of the vault file.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    /// What happened that the user should know, once (the list is emptied).
    ///
    /// # Panics
    /// Never in practice: the lock is only poisoned if another thread panicked while holding it.
    #[must_use]
    pub fn take_warnings(&self) -> Vec<VaultWarning> {
        std::mem::take(&mut *self.warnings.lock().expect("lock"))
    }

    /// The `log2 N` written in the header of the file, without any passphrase; `None` when there is
    /// no file. For `kropa secrets status`.
    ///
    /// # Errors
    /// [`FailureKind::VaultUnsafeFile`] when the folder or the file is not safe;
    /// [`FailureKind::StoreError`] when the file cannot be read or has no valid header.
    pub fn work_factor(&self) -> Result<Option<u8>, Failure> {
        let Some(bytes) = self.read_file()? else {
            return Ok(None);
        };
        peek_work_factor(&bytes).map(Some).ok_or_else(altered)
    }

    /// A test lowers the cost of scrypt: see [`VaultOptions::new`].
    #[cfg(test)]
    fn for_tests(dir: PathBuf, source: Box<dyn PassphraseSource>, log_n: u8) -> Self {
        let clock =
            kropa_core::FixedClock::new("2026-11-12T10:00:00+01:00[+01:00]".parse().unwrap());
        Self::new(
            dir,
            VaultOptions::new(log_n, DEFAULT_MAX_WORK_FACTOR).unwrap(),
            source,
            Arc::new(clock),
        )
    }

    // ---- passphrase ----

    fn passphrase(&self, purpose: Purpose) -> Result<Zeroizing<String>, Failure> {
        let mut held = self.passphrase.lock().expect("lock");
        if let Some(phrase) = held.as_ref() {
            return Ok(phrase.clone());
        }
        let phrase = self.source.get(purpose)?;
        *held = Some(phrase.clone());
        Ok(phrase)
    }

    fn forget_passphrase(&self) {
        *self.passphrase.lock().expect("lock") = None;
    }

    // ---- reading ----

    /// The bytes of the vault file; `None` when the folder or the file does not exist.
    fn read_file(&self) -> Result<Option<Zeroizing<Vec<u8>>>, Failure> {
        if open_dir(&self.dir, false)?.is_none() {
            return Ok(None);
        }
        let Some(mut file) = open_file(&self.path())? else {
            return Ok(None);
        };
        let mut bytes = Zeroizing::new(Vec::new());
        (&mut file)
            .take(MAX_FILE_LEN + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_failure("cannot read the vault", &e))?;
        if bytes.len() as u64 > MAX_FILE_LEN {
            return Err(Failure::new(
                FailureKind::StoreError,
                "the vault file is too large",
            ));
        }
        Ok(Some(bytes))
    }

    /// The document inside the file: [`EMPTY`] when there is no file (no passphrase is asked then).
    fn document(&self) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let Some(bytes) = self.read_file()? else {
            return Ok(Zeroizing::new(EMPTY.to_vec()));
        };
        self.open_bytes(&bytes)
    }

    /// Checks the header, asks for the passphrase and decrypts.
    fn open_bytes(&self, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, Failure> {
        // Before the passphrase and before a single derivation: the factor is in the header.
        let log_n = peek_work_factor(bytes);
        if let Some(log_n) = log_n
            && log_n > self.options.max_work_factor
        {
            return Err(work_factor_refused(log_n, self.options.max_work_factor));
        }
        let phrase = self.passphrase(Purpose::Open)?;
        let result = decrypt(bytes, &phrase, self.options.max_work_factor);
        match &result {
            Ok(_) => {
                if let Some(log_n) = log_n
                    && log_n < WEAK_WORK_FACTOR
                {
                    self.warnings
                        .lock()
                        .expect("lock")
                        .push(VaultWarning::WeakWorkFactor { log_n });
                }
            }
            Err(f) if f.kind == FailureKind::VaultWrongPassphrase => self.forget_passphrase(),
            Err(_) => {}
        }
        result
    }

    // ---- writing ----

    fn exists(&self) -> Result<bool, Failure> {
        if open_dir(&self.dir, false)?.is_none() {
            return Ok(false);
        }
        Ok(open_file(&self.path())?.is_some())
    }

    /// Read-modify-write under the lock. `change` receives the current document and gives the new one,
    /// or `None` when there is nothing to write. Returns whether the file was written.
    ///
    /// With `creates` false, a missing vault is not created: `change` sees an empty store and
    /// whatever it answers is returned, without a lock, a passphrase or a file.
    fn modify(
        &self,
        creates: bool,
        change: impl FnOnce(&[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, Failure>,
    ) -> Result<bool, Failure> {
        if !creates && !self.exists()? {
            return change(EMPTY).map(|new| new.is_some());
        }
        let dir = open_dir(&self.dir, true)?.ok_or_else(|| {
            Failure::new(
                FailureKind::StoreError,
                "the vault folder cannot be created",
            )
        })?;
        let _lock = lockfile::acquire(&self.dir.join(LOCK_NAME)).map_err(|e| lock_failure(&e))?;
        self.remove_stale_temporaries();
        // The relecture under the lock: never from a cache.
        let (current, creating) = match self.read_file()? {
            Some(bytes) => (self.open_bytes(&bytes)?, false),
            None => (Zeroizing::new(EMPTY.to_vec()), true),
        };
        let Some(new) = change(&current)? else {
            return Ok(false);
        };
        let phrase = self.passphrase(if creating {
            Purpose::Create
        } else {
            Purpose::Open
        })?;
        let sealed = seal(&new, &phrase, self.options.work_factor)?;
        self.write_atomically(&dir, &sealed)?;
        Ok(true)
    }

    /// Temporary file `O_EXCL`, `sync_all`, `rename`, `sync_all` of the folder.
    fn write_atomically(&self, dir: &File, bytes: &[u8]) -> Result<(), Failure> {
        let (temporary, mut file) = self.create_temporary()?;
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| io_failure("cannot write the vault", &e));
        drop(file);
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(e);
        }
        #[cfg(test)]
        if self.crash_before_rename.load(Ordering::SeqCst) {
            return Err(Failure::new(
                FailureKind::StoreError,
                "simulated crash before the rename",
            ));
        }
        if let Err(e) = std::fs::rename(&temporary, self.path()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_failure("cannot replace the vault", &e));
        }
        dir.sync_all()
            .map_err(|e| io_failure("cannot sync the vault folder", &e))
    }

    fn create_temporary(&self) -> Result<(PathBuf, File), Failure> {
        let mut last = None;
        for _ in 0..8 {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u32(std::process::id());
            let path = self
                .dir
                .join(format!("{FILE_NAME}.{:016x}.tmp", hasher.finish()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
            {
                Ok(file) => return Ok((path, file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(io_failure("cannot create a temporary file", &e)),
            }
        }
        Err(io_failure(
            "cannot create a temporary file",
            &last.unwrap_or_else(|| io::Error::other("no name left")),
        ))
    }

    /// Removes what a crash left: `secrets.age.<random>.tmp`, ours, older than an hour. Under the lock.
    fn remove_stale_temporaries(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = self.clock.now().timestamp().as_second();
        let uid = system::effective_uid();
        let prefix = format!("{FILE_NAME}.");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(&prefix) || !is_tmp(name) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_file() || meta.uid() != uid {
                continue;
            }
            // The age of the file is the last time it was written.
            let Ok(age) = meta.modified().map(|m| {
                m.duration_since(std::time::UNIX_EPOCH)
                    .map_or(i64::MIN, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            }) else {
                continue;
            };
            if age != i64::MIN && now.saturating_sub(age) > STALE_TEMP_SECS {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn is_tmp(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tmp"))
}

impl ItemStore for VaultItems {
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure> {
        check_part("profile", profile)?;
        store_doc::entries_of(&self.document()?, profile, name)
    }

    fn list(&self) -> Result<Vec<ItemName>, Failure> {
        store_doc::names(&self.document()?)
    }

    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        self.modify(true, |whole| {
            store_doc::insert(whole, profile, name, document, WriteMode::Create).map(Some)
        })
        .map(|_| ())
    }

    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        check_document(document)?;
        self.modify(false, |whole| {
            store_doc::insert(whole, profile, name, document, WriteMode::Replace).map(Some)
        })
        .map(|_| ())
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure> {
        check_part("profile", profile)?;
        check_part("name", name)?;
        self.modify(false, |whole| store_doc::delete(whole, profile, name))
    }
}

// ---- the file system ----

fn unsafe_file(why: &str) -> Failure {
    Failure::new(FailureKind::VaultUnsafeFile, why)
}

fn io_failure(what: &str, error: &io::Error) -> Failure {
    Failure::new(FailureKind::StoreError, format!("{what}: {error}"))
}

fn lock_failure(error: &io::Error) -> Failure {
    if error.kind() == io::ErrorKind::PermissionDenied {
        unsafe_file(
            "the lock file of the vault is a link, belongs to someone else or is open to others",
        )
    } else {
        io_failure("cannot lock the vault", error)
    }
}

/// Whether an `open` failed because of `O_NOFOLLOW` or `O_DIRECTORY` (a link, or not a directory).
fn is_link_or_wrong_type(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == libc::ELOOP || code == libc::ENOTDIR || code == libc::EMLINK
    )
}

/// The folder of the vault, opened and checked on the descriptor: ours, `0700`. Created when `create`
/// (`0700`, with its parents); `None` when it is missing and not created.
fn open_dir(dir: &Path, create: bool) -> Result<Option<File>, Failure> {
    if create {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| io_failure("cannot create the vault folder", &e))?;
    }
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(dir)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
        Err(e) if is_link_or_wrong_type(&e) => {
            return Err(unsafe_file("the vault folder is a link or not a folder"));
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(unsafe_file(
                "the vault folder cannot be opened by this user",
            ));
        }
        Err(e) => return Err(io_failure("cannot open the vault folder", &e)),
    };
    let meta = file
        .metadata()
        .map_err(|e| io_failure("cannot read the vault folder", &e))?;
    if meta.uid() != system::effective_uid() {
        return Err(unsafe_file("the vault folder belongs to someone else"));
    }
    if meta.mode() & 0o777 != 0o700 {
        return Err(unsafe_file(
            "the vault folder is not private: it must be 0700 (chmod 700)",
        ));
    }
    Ok(Some(file))
}

/// The vault file, opened `O_NOFOLLOW` and checked on the descriptor: a regular file of ours that
/// group and others cannot reach. `None` when it does not exist.
fn open_file(path: &Path) -> Result<Option<File>, Failure> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) if is_link_or_wrong_type(&e) => {
            return Err(unsafe_file("the vault file is a link"));
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(unsafe_file("the vault file cannot be opened by this user"));
        }
        Err(e) => return Err(io_failure("cannot open the vault", &e)),
    };
    let meta = file
        .metadata()
        .map_err(|e| io_failure("cannot read the vault", &e))?;
    if !meta.is_file() {
        return Err(unsafe_file("the vault is not a regular file"));
    }
    if meta.uid() != system::effective_uid() {
        return Err(unsafe_file("the vault file belongs to someone else"));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(unsafe_file(
            "the vault file can be read by others: it must be 0600 (chmod 600)",
        ));
    }
    Ok(Some(file))
}

// ---- age ----

/// The `log2 N` of the scrypt stanza at the head of `bytes`, without deriving anything. `None` when
/// the header is not `age-encryption.org/v1` then `-> scrypt <salt> <log2 N>`: `age` itself judges.
fn peek_work_factor(bytes: &[u8]) -> Option<u8> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let line = &rest[..rest.iter().position(|b| *b == b'\n')?];
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split(' ');
    let (Some("->"), Some("scrypt"), Some(_salt), Some(log_n), None) = (
        words.next(),
        words.next(),
        words.next(),
        words.next(),
        words.next(),
    ) else {
        return None;
    };
    if log_n.is_empty() || log_n.len() > 3 || !log_n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    log_n.parse().ok()
}

fn work_factor_refused(log_n: u8, max: u8) -> Failure {
    Failure::new(
        FailureKind::VaultWorkFactor,
        format!(
            "the vault asks for a scrypt work factor of {log_n} (2^{log_n}), above the limit of {max}; \
             nothing was derived. To read it anyway, raise secrets.vault.max_work_factor \
             (at most {LIMIT_WORK_FACTOR}) for this command, only if you wrote this file yourself"
        ),
    )
}

/// The one failure of a wrong passphrase or an altered file: the same text for both.
fn altered() -> Failure {
    Failure::new(
        FailureKind::VaultWrongPassphrase,
        "wrong passphrase or altered file",
    )
}

fn decrypt_failure(error: DecryptError) -> Failure {
    match error {
        DecryptError::ExcessiveWork { required, .. } => {
            work_factor_refused(required, DEFAULT_MAX_WORK_FACTOR.min(LIMIT_WORK_FACTOR))
        }
        // The header cut short or garbled while it was read: an altered file.
        DecryptError::Io(e)
            if matches!(
                e.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData
            ) =>
        {
            altered()
        }
        DecryptError::Io(e) => io_failure("cannot read the vault", &e),
        // A wrong passphrase, a damaged header, a mixed recipient, an unknown format: nothing to tell
        // an attacker apart, and nothing for the user to do but the same thing.
        _ => altered(),
    }
}

fn secret(phrase: &Zeroizing<String>) -> SecretString {
    SecretString::from(phrase.as_str().to_owned())
}

fn deriving() -> MutexGuard<'static, ()> {
    DERIVING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn decrypt(
    bytes: &[u8],
    phrase: &Zeroizing<String>,
    max_work_factor: u8,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let decryptor = Decryptor::new_buffered(bytes).map_err(decrypt_failure)?;
    let mut identity = scrypt::Identity::new(secret(phrase));
    identity.set_max_work_factor(max_work_factor);
    let _one_at_a_time = deriving();
    let mut reader = decryptor
        .decrypt(iter::once(&identity as &dyn age::Identity))
        .map_err(|e| match e {
            DecryptError::ExcessiveWork { required, .. } => {
                work_factor_refused(required, max_work_factor)
            }
            other => decrypt_failure(other),
        })?;
    let mut out = Zeroizing::new(Vec::with_capacity(MAX_STORE_LEN + 1));
    (&mut reader)
        .take(MAX_STORE_LEN as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| match e.kind() {
            // A chunk that does not authenticate, a truncated file.
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => altered(),
            _ => io_failure("cannot read the vault", &e),
        })?;
    if out.len() > MAX_STORE_LEN {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the vault document is too large",
        ));
    }
    Ok(out)
}

/// Encrypts `document` to the passphrase alone: a fresh salt and a fresh file key each time.
fn seal(document: &[u8], phrase: &Zeroizing<String>, log_n: u8) -> Result<Vec<u8>, Failure> {
    let fail = |e: &dyn std::fmt::Display| {
        Failure::new(
            FailureKind::StoreError,
            format!("cannot encrypt the vault: {e}"),
        )
    };
    let mut recipient = scrypt::Recipient::new(secret(phrase));
    recipient.set_work_factor(log_n);
    let encryptor = Encryptor::with_recipients(iter::once(&recipient as &dyn age::Recipient))
        .map_err(|e| fail(&e))?;
    let mut out = Vec::with_capacity(document.len() + 1024);
    let _one_at_a_time = deriving();
    let mut writer = encryptor.wrap_output(&mut out).map_err(|e| fail(&e))?;
    writer.write_all(document).map_err(|e| fail(&e))?;
    writer.finish().map_err(|e| fail(&e))?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, UNIX_EPOCH};

    use age::secrecy::SecretString;
    use age::{Decryptor, Encryptor, scrypt, x25519};
    use kropa_core::FixedClock;
    use kropa_keychain::item::{Failure, FailureKind, ItemStore};
    use kropa_upload::secret::{
        Authorization, Placement, SecretError, SecretSource, SecretStore, SecretValue, StoreState,
    };
    use zeroize::Zeroizing;

    use super::{
        DEFAULT_MAX_WORK_FACTOR, FILE_NAME, PassphraseSource, Purpose, VaultItems, VaultOptions,
        VaultWarning, is_tmp, open_vault_store, peek_work_factor,
    };

    const PHRASE: &str = "correct horse battery staple";

    /// A source that always gives `phrase`, and counts the calls.
    struct Fixed {
        phrase: String,
        calls: Arc<AtomicUsize>,
        purposes: Arc<Mutex<Vec<Purpose>>>,
    }

    impl PassphraseSource for Fixed {
        fn get(&self, purpose: Purpose) -> Result<Zeroizing<String>, Failure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.purposes.lock().unwrap().push(purpose);
            Ok(Zeroizing::new(self.phrase.clone()))
        }
    }

    struct Refusing;

    impl PassphraseSource for Refusing {
        fn get(&self, _: Purpose) -> Result<Zeroizing<String>, Failure> {
            Err(Failure::new(
                FailureKind::VaultLocked,
                "no terminal and no --password-fd",
            ))
        }
    }

    struct Probe {
        calls: Arc<AtomicUsize>,
        purposes: Arc<Mutex<Vec<Purpose>>>,
    }

    fn source(phrase: &str) -> (Box<dyn PassphraseSource>, Probe) {
        let calls = Arc::new(AtomicUsize::new(0));
        let purposes = Arc::new(Mutex::new(Vec::new()));
        let source = Fixed {
            phrase: phrase.to_owned(),
            calls: calls.clone(),
            purposes: purposes.clone(),
        };
        (Box::new(source), Probe { calls, purposes })
    }

    /// A private folder for a vault: a subfolder `0700` of a temporary directory.
    fn vault_dir() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("kropa");
        (tmp, dir)
    }

    fn items(dir: &std::path::Path, phrase: &str, log_n: u8) -> (VaultItems, Probe) {
        let (source, probe) = source(phrase);
        (
            VaultItems::for_tests(dir.to_path_buf(), source, log_n),
            probe,
        )
    }

    fn store(items: VaultItems) -> impl SecretStore {
        let clock = FixedClock::new("2026-11-12T10:00:00+01:00[+01:00]".parse().unwrap());
        open_vault_store(items, Arc::new(clock))
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

    fn set(s: &impl SecretStore, name: &str, v: &str) {
        s.set("zipline", name, value(v), fresh()).unwrap();
    }

    fn kind<T: std::fmt::Debug>(r: Result<T, Failure>) -> FailureKind {
        r.unwrap_err().kind
    }

    /// The vault file with the work factor of its stanza rewritten (the MAC no longer matches: only
    /// the checks that come before the derivation are meant to see it).
    fn with_factor(dir: &std::path::Path, from: u8, to: u8) -> Vec<u8> {
        let mut bytes = std::fs::read(dir.join(FILE_NAME)).unwrap();
        let end = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
        let line_end = end + bytes[end..].iter().position(|b| *b == b'\n').unwrap();
        let line = String::from_utf8(bytes[end..line_end].to_vec()).unwrap();
        let line = line.strip_suffix(&format!(" {from}")).unwrap().to_owned();
        let new = format!("{line} {to}");
        bytes.splice(end..line_end, new.into_bytes());
        bytes
    }

    // ---- behaviour ----

    #[test]
    fn a_secret_set_in_the_vault_is_read_back_and_the_file_is_private_and_plain_age() {
        let (_tmp, dir) = vault_dir();
        let (v, probe) = items(&dir, PHRASE, 10);
        let s = store(v);
        assert_eq!(s.status().store, "vault");
        set(&s, "token", "abcd-efgh-SECRET");
        let entry = s.load("zipline", "token").unwrap().unwrap();
        assert_eq!(entry.value.expose(), "abcd-efgh-SECRET");
        assert_eq!(s.names("zipline").unwrap(), ["token"]);
        assert!(s.profile_exists("zipline").unwrap());
        assert!(s.remove("zipline", "token").unwrap());
        assert!(!s.remove("zipline", "token").unwrap());
        assert!(s.names("zipline").unwrap().is_empty());
        // One passphrase for the whole command, however many operations.
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);

        let path = dir.join(FILE_NAME);
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("store.lock")), 0o600);
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| is_tmp(n))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn the_file_opens_with_plain_age_and_the_value_is_not_in_clear() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        let s = store(v);
        set(&s, "token", "abcd-efgh-SECRET");
        let bytes = std::fs::read(dir.join(FILE_NAME)).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("SECRET"));
        assert!(bytes.starts_with(b"age-encryption.org/v1\n-> scrypt "));
        assert_eq!(peek_work_factor(&bytes), Some(10));

        // What `age -d` does: an scrypt identity, nothing of ours.
        let decryptor = Decryptor::new_buffered(&bytes[..]).unwrap();
        assert!(decryptor.is_scrypt());
        let identity = scrypt::Identity::new(SecretString::from(PHRASE.to_owned()));
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap();
        let mut clear = String::new();
        std::io::Read::read_to_string(&mut reader, &mut clear).unwrap();
        assert!(
            clear.starts_with(r#"{"v":2,"entries":{"zipline/token":"#),
            "{clear}"
        );
        assert!(clear.contains("abcd-efgh-SECRET"));
    }

    #[test]
    fn every_write_draws_a_new_salt() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        let s = store(v);
        set(&s, "a", "abcd-0001");
        let first = std::fs::read(dir.join(FILE_NAME)).unwrap();
        set(&s, "b", "abcd-0002");
        let second = std::fs::read(dir.join(FILE_NAME)).unwrap();
        let stanza = |b: &[u8]| {
            String::from_utf8_lossy(&b[..90])
                .lines()
                .nth(1)
                .unwrap()
                .to_owned()
        };
        assert_ne!(stanza(&first), stanza(&second));
    }

    #[test]
    fn the_passphrase_is_asked_to_create_then_to_open_and_never_for_nothing() {
        let (_tmp, dir) = vault_dir();
        // No vault yet: reading, removing and replacing need no passphrase and create nothing.
        let (v, probe) = items(&dir, PHRASE, 10);
        assert!(v.read("zipline", None).unwrap().is_empty());
        assert!(v.list().unwrap().is_empty());
        assert!(!v.remove("zipline", "t").unwrap());
        assert_eq!(
            kind(v.replace("zipline", "t", br#"{"v":2,"entries":{"zipline/t":{}}}"#)),
            FailureKind::Missing
        );
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert!(!dir.exists());

        let s = store(v);
        set(&s, "token", "abcd-efgh");
        assert_eq!(*probe.purposes.lock().unwrap(), [Purpose::Create]);

        let (v, probe) = items(&dir, PHRASE, 10);
        let s = store(v);
        assert!(s.load("zipline", "token").unwrap().is_some());
        assert_eq!(*probe.purposes.lock().unwrap(), [Purpose::Open]);
    }

    #[test]
    fn a_wrong_passphrase_and_an_altered_file_give_the_same_single_message() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-efgh");
        let good = std::fs::read(dir.join(FILE_NAME)).unwrap();

        let (wrong, _) = items(&dir, "another passphrase here", 10);
        let wrong = wrong.read("zipline", None).unwrap_err();
        assert_eq!(wrong.kind, FailureKind::VaultWrongPassphrase);

        let mut flipped = good.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        let truncated = good[..good.len() - 5].to_vec();
        let garbage = b"hunter2 is not an age file".to_vec();
        for bad in [flipped, truncated, garbage, Vec::new()] {
            std::fs::write(dir.join(FILE_NAME), &bad).unwrap();
            std::fs::set_permissions(dir.join(FILE_NAME), PermissionsExt::from_mode(0o600))
                .unwrap();
            let (v, _) = items(&dir, PHRASE, 10);
            let e = v.read("zipline", None).unwrap_err();
            assert_eq!(e.kind, FailureKind::VaultWrongPassphrase);
            assert_eq!(e.detail, wrong.detail);
            assert!(!e.detail.contains("hunter2") && !e.detail.contains(PHRASE));
        }
    }

    #[test]
    fn a_source_that_cannot_ask_makes_the_vault_locked() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-efgh");
        let locked = VaultItems::for_tests(dir.clone(), Box::new(Refusing), 10);
        let s = store(locked);
        let e = s.load("zipline", "token").unwrap_err();
        assert_eq!(reason(&e), "vault_locked");
        assert_eq!(s.status().state, StoreState::Locked);
        // Writing needs it too, and nothing was changed.
        let before = std::fs::read(dir.join(FILE_NAME)).unwrap();
        assert!(s.remove("zipline", "token").is_err());
        assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap(), before);
    }

    #[test]
    fn a_weak_work_factor_is_reported_after_a_good_read() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 12);
        set(&store(v), "token", "abcd-efgh");
        let (v, _) = items(&dir, PHRASE, 12);
        assert_eq!(v.work_factor().unwrap(), Some(12));
        assert!(v.take_warnings().is_empty());
        v.read("zipline", None).unwrap();
        assert_eq!(
            v.take_warnings(),
            [VaultWarning::WeakWorkFactor { log_n: 12 }]
        );
        assert!(v.take_warnings().is_empty());
    }

    #[test]
    fn the_default_factor_is_18_and_is_what_is_read_back() {
        // The only test at the real cost (about 256 MiB, a second or two).
        let (_tmp, dir) = vault_dir();
        let (source, _) = source(PHRASE);
        let clock = FixedClock::new("2026-11-12T10:00:00+01:00[+01:00]".parse().unwrap());
        let v = VaultItems::new(
            dir.clone(),
            VaultOptions::default(),
            source,
            Arc::new(clock),
        );
        let s = store(v);
        set(&s, "token", "abcd-efgh");
        let bytes = std::fs::read(dir.join(FILE_NAME)).unwrap();
        assert_eq!(peek_work_factor(&bytes), Some(18));
        assert_eq!(
            s.load("zipline", "token").unwrap().unwrap().value.expose(),
            "abcd-efgh"
        );
    }

    #[test]
    fn the_options_are_bounded() {
        assert!(VaultOptions::new(18, 22).is_ok());
        assert!(VaultOptions::new(18, 23).is_err());
        assert!(VaultOptions::new(21, 20).is_err());
        assert!(VaultOptions::new(0, 20).is_err());
        let default = VaultOptions::default();
        assert_eq!((default.work_factor(), default.max_work_factor()), (18, 20));
    }

    // ---- threats (secrets.md section 7) ----

    #[test]
    fn a_file_open_to_others_a_link_and_a_loose_folder_are_refused() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-efgh");
        let path = dir.join(FILE_NAME);
        let doc = br#"{"v":2,"entries":{"zipline/x":{"origins":[],"placements":[],"value":"v"}}}"#;

        // File 0644: reading and writing refuse, and the file is not touched.
        std::fs::set_permissions(&path, PermissionsExt::from_mode(0o644)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let (v, probe) = items(&dir, PHRASE, 10);
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);
        assert_eq!(
            kind(v.create("zipline", "x", doc)),
            FailureKind::VaultUnsafeFile
        );
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            0,
            "no passphrase for an unsafe file"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::set_permissions(&path, PermissionsExt::from_mode(0o600)).unwrap();

        // A link in place of the file.
        let real = dir.join("real.age");
        std::fs::rename(&path, &real).unwrap();
        symlink(&real, &path).unwrap();
        let (v, _) = items(&dir, PHRASE, 10);
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);
        assert_eq!(
            kind(v.create("zipline", "x", doc)),
            FailureKind::VaultUnsafeFile
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&real, &path).unwrap();

        // Folder 0755.
        std::fs::set_permissions(&dir, PermissionsExt::from_mode(0o755)).unwrap();
        let (v, _) = items(&dir, PHRASE, 10);
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);
        assert_eq!(
            kind(v.create("zipline", "x", doc)),
            FailureKind::VaultUnsafeFile
        );
        std::fs::set_permissions(&dir, PermissionsExt::from_mode(0o700)).unwrap();

        // The folder itself a link.
        let link = dir.parent().unwrap().join("link");
        symlink(&dir, &link).unwrap();
        let (v, _) = items(&link, PHRASE, 10);
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);

        // Back to normal: everything works again.
        let (v, _) = items(&dir, PHRASE, 10);
        assert_eq!(v.read("zipline", None).unwrap().len(), 1);
    }

    /// `nobody`, when `sudo -n -u nobody` works here; panics when `KROPA_REQUIRE_VAULT_UID_TESTS=1`
    /// and it does not, else says why and skips.
    fn nobody_available() -> bool {
        let required = std::env::var("KROPA_REQUIRE_VAULT_UID_TESTS").is_ok_and(|v| v == "1");
        let why = match Command::new("sudo")
            .args(["-n", "-u", "nobody", "--", "id", "-u"])
            .stdin(Stdio::null())
            .output()
        {
            Err(e) => format!("sudo cannot be run: {e}"),
            Ok(out) if !out.status.success() => format!(
                "`sudo -n -u nobody` failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Ok(out) => {
                if String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
                    == Some(kropa_keychain::system::effective_uid())
                {
                    "the tests already run as nobody".to_owned()
                } else {
                    return true;
                }
            }
        };
        assert!(!required, "KROPA_REQUIRE_VAULT_UID_TESTS=1 but: {why}");
        eprintln!("SKIPPED (set KROPA_REQUIRE_VAULT_UID_TESTS=1 to make this an error): {why}");
        false
    }

    #[test]
    fn a_vault_that_belongs_to_another_user_is_refused() {
        if !nobody_available() {
            return;
        }
        // Under /tmp, not $TMPDIR (it may be private): a folder `nobody` can write to.
        let tmp = tempfile::Builder::new()
            .prefix("kropa-vault-")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(tmp.path(), PermissionsExt::from_mode(0o777)).unwrap();
        let file = tmp.path().join(FILE_NAME);
        let made = Command::new("sudo")
            .args([
                "-n",
                "-u",
                "nobody",
                "--",
                "sh",
                "-c",
                "umask 077 && : > \"$1\"",
                "sh",
            ])
            .arg(&file)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(made.success());
        std::fs::set_permissions(tmp.path(), PermissionsExt::from_mode(0o700)).unwrap();
        let (v, probe) = items(tmp.path(), PHRASE, 10);
        // Another owner (root can open the file, others cannot): refused either way, before any
        // passphrase.
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);

        // A folder of another user, too.
        let theirs = tmp.path().join("theirs");
        let made = Command::new("sudo")
            .args([
                "-n",
                "-u",
                "nobody",
                "--",
                "sh",
                "-c",
                "mkdir -m 700 \"$1\"",
                "sh",
            ])
            .arg(&theirs)
            .stdin(Stdio::null())
            .status();
        // The folder is 0700 and ours: nobody cannot create there. Make it writable for the test.
        if made.is_ok_and(|s| !s.success()) {
            std::fs::set_permissions(tmp.path(), PermissionsExt::from_mode(0o777)).unwrap();
            let made = Command::new("sudo")
                .args([
                    "-n",
                    "-u",
                    "nobody",
                    "--",
                    "sh",
                    "-c",
                    "mkdir -m 700 \"$1\"",
                    "sh",
                ])
                .arg(&theirs)
                .stdin(Stdio::null())
                .status()
                .unwrap();
            assert!(made.success());
            std::fs::set_permissions(tmp.path(), PermissionsExt::from_mode(0o700)).unwrap();
        }
        let (v, _) = items(&theirs, PHRASE, 10);
        assert_eq!(kind(v.read("zipline", None)), FailureKind::VaultUnsafeFile);
        assert_eq!(
            kind(v.create("zipline", "x", br#"{"v":2,"entries":{}}"#)),
            FailureKind::VaultUnsafeFile
        );
        // Let the temporary directory be removed.
        let _ = Command::new("sudo")
            .args(["-n", "-u", "nobody", "--", "rm", "-rf"])
            .arg(&theirs)
            .stdin(Stdio::null())
            .status();
    }

    #[test]
    fn a_crash_before_the_rename_leaves_the_old_vault_intact() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        let s = store(v);
        set(&s, "first", "abcd-0001");
        let before = std::fs::read(dir.join(FILE_NAME)).unwrap();

        let (v, _) = items(&dir, PHRASE, 10);
        v.crash_before_rename.store(true, Ordering::SeqCst);
        let s = store(v);
        let e = s
            .set("zipline", "second", value("abcd-0002"), fresh())
            .unwrap_err();
        assert!(matches!(e, SecretError::Store { .. }));

        assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap(), before);
        let (v, _) = items(&dir, PHRASE, 10);
        let s = store(v);
        assert_eq!(s.names("zipline").unwrap(), ["first"]);
        // What the crash left is encrypted, ours and private.
        let temporaries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert_eq!(temporaries.len(), 1);
        let content = std::fs::read(temporaries[0].path()).unwrap();
        assert!(content.starts_with(b"age-encryption.org/v1\n"));
        assert!(!String::from_utf8_lossy(&content).contains("abcd-0002"));
    }

    #[test]
    fn temporary_files_older_than_an_hour_are_removed_and_the_others_kept() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "first", "abcd-0001");
        // The clock of the tests says 2026-11-12T09:00:00Z.
        let now = FixedClock::new("2026-11-12T10:00:00+01:00[+01:00]".parse().unwrap());
        let now = u64::try_from(kropa_core::Clock::now(&now).timestamp().as_second()).unwrap();
        let make = |name: &str, age: u64| {
            let path = dir.join(name);
            let file = std::fs::File::create(&path).unwrap();
            file.set_modified(UNIX_EPOCH + Duration::from_secs(now - age))
                .unwrap();
            path
        };
        let old = make("secrets.age.0000000000000001.tmp", 2 * 3600);
        let recent = make("secrets.age.0000000000000002.tmp", 600);
        let other = make("notes.tmp", 2 * 3600);
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "second", "abcd-0002");
        assert!(!old.exists());
        assert!(recent.exists());
        assert!(other.exists(), "only our temporary files are removed");
    }

    #[test]
    fn two_writers_of_100_entries_each_make_200() {
        let (_tmp, dir) = vault_dir();
        let dir = Arc::new(dir);
        let threads: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|who| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    // Its own handle, as a process of its own would have.
                    let (v, _) = items(&dir, PHRASE, 10);
                    let s = store(v);
                    for i in 0..100 {
                        s.set(
                            "zipline",
                            &format!("{who}-{i}"),
                            value(&format!("abcd-{who}-{i}")),
                            fresh(),
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let (v, _) = items(&dir, PHRASE, 10);
        let names = v.list().unwrap();
        assert_eq!(names.len(), 200);
        let s = store(v);
        assert_eq!(
            s.load("zipline", "a-99").unwrap().unwrap().value.expose(),
            "abcd-a-99"
        );
        assert_eq!(
            s.load("zipline", "b-0").unwrap().unwrap().value.expose(),
            "abcd-b-0"
        );
    }

    #[test]
    fn a_file_with_too_high_a_factor_is_refused_fast_before_any_passphrase() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-efgh");
        for log_n in [21, 30, 64, 255] {
            let bytes = with_factor(&dir, 10, log_n);
            let trap = dir.join(FILE_NAME);
            // Written to a private file of its own, so that the real one stays valid.
            let dir2 = dir.parent().unwrap().join(format!("f{log_n}"));
            std::fs::create_dir(&dir2).unwrap();
            std::fs::set_permissions(&dir2, PermissionsExt::from_mode(0o700)).unwrap();
            let file = dir2.join(FILE_NAME);
            std::fs::write(&file, &bytes).unwrap();
            std::fs::set_permissions(&file, PermissionsExt::from_mode(0o600)).unwrap();
            assert!(trap.exists());

            let (v, probe) = items(&dir2, PHRASE, 10);
            let started = Instant::now();
            let e = v.read("zipline", None).unwrap_err();
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "log2 N = {log_n}"
            );
            assert_eq!(
                e.kind,
                FailureKind::VaultWorkFactor,
                "log2 N = {log_n}: {e}"
            );
            assert!(e.detail.contains(&log_n.to_string()), "{}", e.detail);
            assert!(e.detail.contains("max_work_factor"), "{}", e.detail);
            assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
            // Writing refuses too, and leaves the file as it was.
            let doc =
                br#"{"v":2,"entries":{"zipline/x":{"origins":[],"placements":[],"value":"v"}}}"#;
            assert_eq!(
                kind(v.create("zipline", "x", doc)),
                FailureKind::VaultWorkFactor
            );
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
        }
        assert_eq!(DEFAULT_MAX_WORK_FACTOR, 20);
    }

    #[test]
    fn a_scrypt_stanza_mixed_with_another_is_refused() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-efgh");
        let mut bytes = std::fs::read(dir.join(FILE_NAME)).unwrap();

        // A real file for an x25519 recipient: take its stanza and add it after the scrypt one.
        let identity = x25519::Identity::generate();
        let recipient = identity.to_public();
        let mut other = Vec::new();
        let mut w = Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .unwrap()
            .wrap_output(&mut other)
            .unwrap();
        std::io::Write::write_all(&mut w, b"x").unwrap();
        w.finish().unwrap();
        let header_end = |b: &[u8]| b.windows(5).position(|w| w == b"\n--- ").unwrap() + 1;
        let stanza = &other[MAGIC_LEN..header_end(&other)];
        let at = header_end(&bytes);
        bytes.splice(at..at, stanza.iter().copied());
        assert_eq!(&bytes[..MAGIC_LEN], b"age-encryption.org/v1\n");
        std::fs::write(dir.join(FILE_NAME), &bytes).unwrap();
        std::fs::set_permissions(dir.join(FILE_NAME), PermissionsExt::from_mode(0o600)).unwrap();

        let (v, _) = items(&dir, PHRASE, 10);
        let e = v.read("zipline", None).unwrap_err();
        assert_eq!(e.kind, FailureKind::VaultWrongPassphrase);
        let (v, _) = items(&dir, PHRASE, 10);
        assert_eq!(
            kind(v.create("zipline", "x", br#"{"v":2,"entries":{}}"#)),
            FailureKind::VaultWrongPassphrase
        );
    }

    const MAGIC_LEN: usize = b"age-encryption.org/v1\n".len();

    #[test]
    fn the_header_peek_reads_only_a_well_formed_scrypt_stanza() {
        assert_eq!(
            peek_work_factor(b"age-encryption.org/v1\n-> scrypt c2FsdA 18\nbody"),
            Some(18)
        );
        for bad in [
            &b""[..],
            b"age-encryption.org/v1\n",
            b"age-encryption.org/v1\n-> X25519 abc\n",
            b"age-encryption.org/v1\n-> scrypt abc\n",
            b"age-encryption.org/v1\n-> scrypt abc 18 extra\n",
            b"age-encryption.org/v1\n-> scrypt abc 1x\n",
            b"age-encryption.org/v1\n-> scrypt abc 99999\n",
            b"age-encryption.org/v1\n-> scrypt abc -1\n",
            b"nope\n-> scrypt abc 18\n",
        ] {
            assert_eq!(
                peek_work_factor(bad),
                None,
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn nothing_secret_is_in_a_failure() {
        let (_tmp, dir) = vault_dir();
        let (v, _) = items(&dir, PHRASE, 10);
        set(&store(v), "token", "abcd-SECRET");
        let (v, _) = items(&dir, "wrong wrong wrong wrong", 10);
        let e = v.read("zipline", None).unwrap_err();
        let text = format!("{e} {e:?}");
        assert!(
            !text.contains("wrong wrong") && !text.contains("SECRET"),
            "{text}"
        );
    }
}
