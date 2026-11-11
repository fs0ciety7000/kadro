// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS Keychain for Kropa, and the broker that stands between `kropa-app` and it (lot 1.8m,
//! decision U1, `docs/security/secrets.md` sections 2.2 and 2.2.1).
//!
//! Layers, from the top:
//!
//! - [`item::ItemStore`]: what the Keychain does, per secret (`<profile>/<name>`), whatever holds
//!   it. [`memory::MemoryItemStore`] for tests; [`FileKeychain`] (macOS) for real; [`BrokerClient`]
//!   for a process that must not read the Keychain itself.
//! - [`wire`]: one request per launch, one reply, over a socket pair.
//! - [`guard`] and [`server`]: what the broker checks about how it was launched before it touches
//!   the Keychain, and how it serves one request. Portable: tested without a Mac.
//! - [`store_doc`], [`lockfile`], [`deadline`]: the single Keychain item (all the entries in one
//!   version 2 document), the lock that makes its read-modify-write atomic between processes, and the
//!   deadline of a call. Portable.
//! - [`system`], [`spawn`], [`apple`]: the facts of the process, starting the broker, and what only
//!   macOS can say (audit token, code signature). The only places with `unsafe`.
//!
//! `kropa-store` turns an [`item::ItemStore`] into the `SecretStore` of the commands.

#![cfg_attr(test, allow(clippy::assert_is_empty, clippy::naive_bytecount))]

pub mod client;
pub mod deadline;
pub mod guard;
pub mod item;
pub mod lockfile;
pub mod memory;
pub mod server;
pub mod spawn;
pub mod store_doc;
pub mod system;
pub mod wire;

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
pub mod keychain;

pub use client::{BROKER_ARGUMENT, BrokerClient};
#[cfg(target_os = "macos")]
pub use keychain::FileKeychain;
pub use system::SystemFacts;
