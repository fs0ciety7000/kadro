// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS Keychain broker, seen from `kropa-store` (decision U1, `docs/security/secrets.md`
//! sections 2.2 and 2.2.1; the mechanism is in `kropa-keychain`).
//!
//! Under macOS **only the `kropa` binary reads the Keychain**, so that the user sees one Keychain
//! prompt per update and not one per binary (the access control list of an item is tied to the
//! `cdhash` of the binary). Two ways to reach it:
//!
//! - `kropa` itself (the command line) reads the Keychain directly: [`crate::open_system_store`].
//! - `kropa-app` never does. It starts `kropa __store-broker` ([`run_broker`]) for each operation, on
//!   a socket pair inherited as fd 3, sends one request and reads one reply:
//!   [`open_broker_store`], a [`KeychainBroker`].
//!
//! The broker refuses everything unless its caller is `kropa-app` signed with the same certificate
//! as itself, under the hardened runtime ([`kropa_keychain::guard`] says exactly what is checked
//! and what is not). With an ad hoc signature the broker serves nobody, and `kropa-app` gets
//! `keychain_broker_unavailable`: it is for the user to choose the `age` vault (lot 2.8) then;
//! nothing is switched by itself.

use std::sync::Arc;

use kropa_core::Clock;
use kropa_keychain::BrokerClient;
use kropa_keychain::item::Failure;
use kropa_upload::secret::SecretStore;

use crate::keychain_store::KeychainStore;

/// A [`SecretStore`] that goes through the Keychain broker. A marker trait: the operations are those
/// of [`SecretStore`] and [`SecretSource`](kropa_upload::secret::SecretSource).
pub trait KeychainBroker: SecretStore {}

impl KeychainBroker for KeychainStore<BrokerClient> {}

/// The store of `kropa-app`: the Keychain through `kropa __store-broker`, the `kropa` binary that sits
/// next to the running one (in `Kropa.app`).
///
/// # Errors
/// A [`Failure`] when the path of the running binary is unknown.
pub fn open_broker_store(
    clock: Arc<dyn Clock + Send + Sync>,
) -> Result<Box<dyn KeychainBroker + Send + Sync>, Failure> {
    Ok(Box::new(KeychainStore::new(
        BrokerClient::beside_current_exe()?,
        clock,
        "keychain-broker",
    )))
}

/// `kropa __store-broker`: serves one request on fd 3 and returns the exit status (0, or the status
/// of the refusal, `kropa_keychain::guard::Refusal::exit_code`). Nothing is read from the Keychain
/// before the launch has passed every check.
#[cfg(target_os = "macos")]
#[must_use]
pub fn run_broker() -> std::process::ExitCode {
    let backend = crate::macos_backend();
    std::process::ExitCode::from(kropa_keychain::server::run(
        &kropa_keychain::SystemFacts,
        backend.as_ref(),
        kropa_keychain::system::take_channel,
    ))
}
