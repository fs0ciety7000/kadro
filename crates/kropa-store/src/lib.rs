// SPDX-License-Identifier: GPL-3.0-or-later

//! Local storage of Kropa: secrets in the system keyring (lot 1.8), capture history (lot 4.6) and
//! the `age` file fallback (lot 2.8).
//!
//! The commands and the upload engine only know the traits of `kropa_upload::secret`
//! ([`SecretSource`](kropa_upload::secret::SecretSource) to read,
//! [`SecretStore`](kropa_upload::secret::SecretStore) to manage). [`open_system_store`] gives the
//! one of the platform:
//!
//! - Linux: [`SecretServiceStore`], the Secret Service over D-Bus.
//! - macOS: the Keychain, **only through a broker** (decision U1, `docs/security/secrets.md`
//!   section 2.2): [`KeychainBroker`] is the contract, its implementation is lot 1.8m. Until then
//!   the store refuses with exit 5 ([`UnavailableSecretSource`](kropa_upload::secret::UnavailableSecretSource)).
//!
//! There is never a silent switch from a store to another: the one that is chosen answers, or
//! fails with a reason.

#![allow(clippy::doc_markdown)]

pub mod broker;
pub mod document;
#[cfg(target_os = "linux")]
pub mod secret_service;

pub use broker::KeychainBroker;
#[cfg(target_os = "linux")]
pub use secret_service::{SERVICE, SecretServiceStore, Timeouts};

use std::sync::Arc;

use kropa_core::Clock;
use kropa_upload::secret::SecretStore;

/// The secret store of this platform. Opens nothing yet: each call connects by itself.
#[must_use]
pub fn open_system_store(
    clock: Arc<dyn Clock + Send + Sync>,
) -> Box<dyn SecretStore + Send + Sync> {
    #[cfg(target_os = "linux")]
    {
        Box::new(SecretServiceStore::new(clock))
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Lot 1.8m brings the Keychain broker of macOS; nothing is simulated until then.
        let _ = clock;
        Box::new(kropa_upload::secret::UnavailableSecretSource)
    }
}
