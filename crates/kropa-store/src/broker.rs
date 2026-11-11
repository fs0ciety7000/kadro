// SPDX-License-Identifier: GPL-3.0-or-later

//! The contract of the macOS Keychain broker (decision U1, `docs/security/secrets.md` sections 2.2
//! and 2.2.1). Lot 1.8m implements it; nothing here is simulated, and no code of this crate calls
//! the Keychain.
//!
//! Under macOS only the `kropa` binary opens the Keychain. `kropa-app` runs `kropa __store-broker`
//! over an inherited socket pair and gets from it what a profile needs, so that the user sees one
//! Keychain prompt per update instead of one per binary. The CLI itself reads the Keychain
//! directly: its direct path is the same code without the broker's checks of a caller.
//!
//! What an implementation must do, as tests of 1.8m:
//!
//! - one Keychain item per secret, service `io.github.fs0ciety7000.Kropa`, account
//!   `<profile>/<name>`, the secret being the version 2 document of [`crate::document`];
//! - modify an item in place (search then update), never delete and re-create it (that resets its
//!   access control list);
//! - refuse before any Keychain call when the caller is not `kropa-app` (section 2.2.1 points 1 to
//!   4), with `error.reason = "keychain_broker_refused"` (exit 7);
//! - a refused Keychain prompt is `keychain_denied` (exit 4), never a fallback to another store;
//! - answer with the entries of one profile at most, never the whole document;
//! - report [`StoreState::Denied`](kropa_upload::secret::StoreState::Denied) in `status` when the
//!   user refused.

use kropa_upload::secret::SecretStore;

/// A [`SecretStore`] that goes through the Keychain broker. A marker trait: the operations are those
/// of [`SecretStore`] and [`SecretSource`](kropa_upload::secret::SecretSource), and the macOS agent
/// of lot 1.8m implements them on the type that speaks to `kropa __store-broker`.
pub trait KeychainBroker: SecretStore {}
