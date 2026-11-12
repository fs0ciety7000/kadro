// SPDX-License-Identifier: GPL-3.0-or-later

//! Errors of the IPC. Every failure to reach the application is exit code 14 (`daemon_unavailable`,
//! `docs/cli.md` section 3); the cause goes in `error.reason`. An error returned by a method of the
//! application keeps the exit code that the application gave it.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::proto::RpcError;

/// Exit code of every failure to reach the application (`ExitCode::DaemonUnavailable`).
pub const DAEMON_UNAVAILABLE: u8 = 14;

/// What went wrong between the command line and the application.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// No place for the socket that is safe by construction (no `XDG_RUNTIME_DIR`, no home).
    #[error("no safe runtime directory: {0}")]
    NoRuntimeDir(String),
    /// The runtime directory or the socket path is not one Kropa may use.
    #[error("unsafe runtime directory {}: {why}", path.display())]
    UnsafeRuntimeDir {
        /// The directory (or socket path) that was refused.
        path: PathBuf,
        /// Why.
        why: String,
    },
    /// Nothing listens on the socket.
    #[error("the application is not running (socket {})", .0.display())]
    NotRunning(PathBuf),
    /// The launcher could not start the application, or another instance holds the lock.
    #[error("cannot start the application: {0}")]
    StartFailed(String),
    /// The application was launched but its socket did not answer in time.
    #[error("the application did not answer within {0:?}")]
    StartTimeout(Duration),
    /// The process at the other end of the socket is not ours.
    #[error("the socket belongs to user {peer}, not to user {expected}")]
    PeerUidMismatch {
        /// UID of the peer, as the kernel reports it.
        peer: u32,
        /// UID of this process.
        expected: u32,
    },
    /// The two sides speak different protocol versions.
    #[error("IPC protocol {server} on the server, {client} on the client")]
    VersionMismatch {
        /// Version announced by the server.
        server: u32,
        /// Version of this build.
        client: u32,
    },
    /// A message that is not valid JSON-RPC, is too long, or does not come in time.
    #[error("IPC protocol error: {0}")]
    Protocol(String),
    /// The socket failed.
    #[error("IPC {context}: {source}")]
    Io {
        /// What was being done.
        context: &'static str,
        /// The system error.
        source: io::Error,
    },
    /// The application answered with an error.
    #[error("{}", .0.message)]
    Rpc(RpcError),
}

impl IpcError {
    /// Process exit code for this error.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Rpc(e) => e.exit_code,
            _ => DAEMON_UNAVAILABLE,
        }
    }

    /// Machine sub-cause (`error.reason`).
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        Some(match self {
            Self::NoRuntimeDir(_) => "no_runtime_dir",
            Self::UnsafeRuntimeDir { .. } => "unsafe_runtime_dir",
            Self::NotRunning(_) => "daemon_not_running",
            Self::StartFailed(_) => "daemon_start_failed",
            Self::StartTimeout(_) => "daemon_start_timeout",
            Self::PeerUidMismatch { .. } => "peer_uid_mismatch",
            Self::VersionMismatch { .. } => "ipc_version_mismatch",
            Self::Protocol(_) | Self::Io { .. } => "ipc_protocol_error",
            Self::Rpc(e) => return e.reason.as_deref(),
        })
    }

    pub(crate) fn io(context: &'static str, source: io::Error) -> Self {
        Self::Io { context, source }
    }
}
