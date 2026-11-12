// SPDX-License-Identifier: GPL-3.0-or-later

//! Local storage of Kropa: secrets in the system keyring (lot 1.8), capture history (lot 4.6) and
//! the `age` file fallback (lot 2.8).
//!
//! The commands and the upload engine only know the traits of `kropa_upload::secret`
//! ([`SecretSource`](kropa_upload::secret::SecretSource) to read,
//! [`SecretStore`](kropa_upload::secret::SecretStore) to manage). [`open_system_store`] gives the
//! one of the platform:
//!
//! - Linux: [`SecretServiceStore`], the Secret Service over D-Bus (one item per secret).
//! - macOS: [`KeychainStore`] over the classic file keychain, **one single item** that holds every
//!   entry (so that an update costs one Keychain prompt, not one per secret). `kropa-app` reaches it
//!   through the broker, see [`broker`].
//!
//! - Everywhere, by choice of the user: the `age` vault ([`vault`]), one encrypted file.
//!
//! There is never a silent switch from a store to another: the one that is chosen answers, or
//! fails with a reason.

#![allow(clippy::doc_markdown)]

pub mod broker;
pub mod document;
pub mod keychain_store;
#[cfg(target_os = "linux")]
pub mod secret_service;
pub mod vault;

pub use broker::{KeychainBroker, open_broker_store};
pub use keychain_store::KeychainStore;
pub use kropa_keychain::BROKER_ARGUMENT;
#[cfg(target_os = "linux")]
pub use secret_service::{SERVICE, SecretServiceStore, Timeouts};
pub use vault::{
    PassphraseSource, Purpose, VaultItems, VaultOptions, VaultWarning, open_vault_store,
};

#[cfg(target_os = "macos")]
pub use broker::run_broker;
#[cfg(target_os = "macos")]
pub use kropa_keychain::item::SERVICE;

use std::sync::Arc;

use kropa_core::Clock;
use kropa_upload::secret::SecretStore;

/// The Keychain of the user, as an item store: the real one, or, when the home folder is unknown
/// (so that the lock file has no place), one that says so on every call.
#[cfg(target_os = "macos")]
fn macos_backend() -> Box<dyn kropa_keychain::item::ItemStore> {
    use kropa_keychain::item::{Failure, FailureKind, SERVICE};
    use kropa_keychain::memory::MemoryItemStore;

    match directories::BaseDirs::new() {
        Some(dirs) => Box::new(kropa_keychain::FileKeychain::user_default(
            dirs.data_dir().join(SERVICE).join("store.lock"),
        )),
        None => {
            let unusable = MemoryItemStore::new();
            unusable.fail_with(Failure::new(
                FailureKind::Unavailable,
                "the home folder of the user is unknown",
            ));
            Box::new(unusable)
        }
    }
}

/// The secret store of this platform. Opens nothing yet: each call connects by itself.
///
/// On macOS this is the direct path of `kropa` (the command line); `kropa-app` uses
/// [`open_broker_store`].
#[must_use]
pub fn open_system_store(
    clock: Arc<dyn Clock + Send + Sync>,
) -> Box<dyn SecretStore + Send + Sync> {
    #[cfg(target_os = "linux")]
    {
        Box::new(SecretServiceStore::new(clock))
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(KeychainStore::new(macos_backend(), clock, "keychain"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // No store on this platform; nothing is simulated.
        let _ = clock;
        Box::new(kropa_upload::secret::UnavailableSecretSource)
    }
}
