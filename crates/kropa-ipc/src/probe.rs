// SPDX-License-Identifier: GPL-3.0-or-later

//! What `kropa doctor` shows about the IPC: nothing is launched, nothing is changed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::client::Client;
use crate::dir::{current_uid, dir_exists, runtime_dir, socket_path, verify_dir};
use crate::error::IpcError;
use crate::launcher::APP_ID;
use crate::proto::HelloReply;

/// State of the socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    /// No socket (the application is not running).
    Absent,
    /// The application answered `hello`.
    Listening,
    /// A socket file exists but nobody answers (a server that died).
    Orphan,
    /// The directory, the owner of the server or the runtime environment is not safe: see the reason.
    Unsafe,
    /// Someone answers but with another protocol, or badly.
    Incompatible,
}

impl ProbeState {
    /// Name in JSON and in Fluent ids.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Listening => "listening",
            Self::Orphan => "orphan",
            Self::Unsafe => "unsafe",
            Self::Incompatible => "incompatible",
        }
    }
}

/// The result of [`probe`].
#[derive(Debug)]
pub struct Probe {
    /// The socket path, when it could be worked out.
    pub socket: Option<PathBuf>,
    /// Its state.
    pub state: ProbeState,
    /// Machine reason for `Unsafe` and `Incompatible`.
    pub reason: Option<String>,
    /// What the server said about itself, when it is listening.
    pub server: Option<HelloReply>,
}

/// Looks at the socket of this user (see [`probe_in`]).
#[must_use]
pub fn probe() -> Probe {
    match runtime_dir() {
        Ok(dir) => probe_in(&dir),
        Err(e) => Probe {
            socket: None,
            state: ProbeState::Unsafe,
            reason: e.reason().map(str::to_owned),
            server: None,
        },
    }
}

/// Looks at the socket of `dir` by connecting and saying `hello`. Blocking, two seconds at most.
#[must_use]
pub fn probe_in(dir: &Path) -> Probe {
    let socket = socket_path(dir);
    let answer = |state, reason: Option<&str>, server| Probe {
        socket: Some(socket.clone()),
        state,
        reason: reason.map(str::to_owned),
        server,
    };
    if !dir_exists(dir) {
        return answer(ProbeState::Absent, None, None);
    }
    if let Err(e) = verify_dir(dir, current_uid()) {
        return answer(ProbeState::Unsafe, e.reason(), None);
    }
    if std::fs::symlink_metadata(&socket).is_err() {
        return answer(ProbeState::Absent, None, None);
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return answer(ProbeState::Unsafe, Some("ipc_protocol_error"), None);
    };
    let outcome = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), Client::connect(&socket))
            .await
            .unwrap_or_else(|_| Err(IpcError::Protocol("no answer to hello".to_owned())))
    });
    match outcome {
        Ok(client) => answer(ProbeState::Listening, None, Some(client.server().clone())),
        Err(IpcError::NotRunning(_)) => answer(ProbeState::Orphan, None, None),
        Err(e @ (IpcError::PeerUidMismatch { .. } | IpcError::UnsafeRuntimeDir { .. })) => {
            answer(ProbeState::Unsafe, e.reason(), None)
        }
        Err(e) => answer(ProbeState::Incompatible, e.reason(), None),
    }
}

/// Whether the D-Bus service file of the application is installed, in the directories where the
/// session bus looks for it. `None` where there is no session bus activation (macOS).
#[must_use]
pub fn service_file_present(lookup: &dyn Fn(&str) -> Option<String>) -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let file = format!("dbus-1/services/{APP_ID}.service");
    let mut dirs: Vec<PathBuf> = Vec::new();
    match lookup("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(home) => dirs.push(PathBuf::from(home)),
        None => {
            if let Some(home) = lookup("HOME").filter(|v| !v.is_empty()) {
                dirs.push(Path::new(&home).join(".local/share"));
            }
        }
    }
    let data_dirs = lookup("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    Some(dirs.iter().any(|d| d.join(&file).is_file()))
}
