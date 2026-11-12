// SPDX-License-Identifier: GPL-3.0-or-later

//! IPC between the `kropa` command line and the graphical application (`kropa-gtk` on Linux,
//! `kropa-app` on macOS): JSON-RPC 2.0 over a Unix socket, one message per line of at most
//! [`MAX_LINE`] bytes (`docs/security/secrets.md` section 1.2, `docs/lots/lot-2.md` task 2.1a).
//!
//! Layout: [`proto`] is the wire format and the `hello` messages, [`dir`] the place of the socket
//! and the checks on it, [`client`] the connection (UID of the peer checked before anything is
//! sent), [`server`] the listener (lock, orphan socket, UID of each client), [`launcher`] how the
//! application is started on demand, and [`probe`] what `kropa doctor` reports.
//!
//! Every failure to reach the application is exit code 14; see [`IpcError::reason`].

pub mod client;
pub mod dir;
pub mod error;
pub mod launcher;
pub mod probe;
pub mod proto;
pub mod server;

pub use client::Client;
pub use dir::{current_uid, runtime_dir, socket_path};
pub use error::IpcError;
#[cfg(target_os = "linux")]
pub use launcher::DbusActivation;
pub use launcher::{APP_ID, Launcher, Spawn, ensure_running, ensure_running_in};
pub use probe::{Probe, ProbeState, probe, probe_in, service_file_present};
pub use proto::{HelloParams, HelloReply, MAX_LINE, PROTOCOL, RpcError, loggable_params};
pub use server::{BoxFuture, Handler, Server, ServerConfig, ServerThread, Shutdown, serve};
