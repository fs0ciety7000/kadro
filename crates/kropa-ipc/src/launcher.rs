// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting the application when nobody listens, and waiting for it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::client::Client;
use crate::dir::{runtime_dir, socket_path};
use crate::error::IpcError;

/// Application ID (D-Bus name, `.desktop` and `.service` file names).
pub const APP_ID: &str = "io.github.fs0ciety7000.Kropa";

/// The usual budget of [`ensure_running`]: the whole call, from the first connection to the answer to
/// `hello` of an application that was started, takes at most this long (review 2.1a m12).
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Pause between two attempts to connect.
const POLL: Duration = Duration::from_millis(50);

/// Something that starts the application. It returns when the launch is done, not when the socket
/// answers: [`ensure_running`] waits for that.
pub trait Launcher {
    /// Starts the application, taking at most `budget` (what is left of the call's total budget).
    ///
    /// # Errors
    /// `daemon_start_failed`.
    fn launch(&self, budget: Duration) -> Result<(), IpcError>;
}

/// Launch through the session bus (`StartServiceByName`, decision D1): the application starts in the
/// clean environment of the bus, with none of the descriptors or variables of the command line.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct DbusActivation {
    app_id: String,
    bus_address: Option<String>,
    timeout: Duration,
}

#[cfg(target_os = "linux")]
impl DbusActivation {
    /// Activation of [`APP_ID`] on the session bus.
    #[must_use]
    pub fn new() -> Self {
        Self {
            app_id: APP_ID.to_owned(),
            bus_address: None,
            timeout: START_TIMEOUT,
        }
    }

    /// Activation of another name on another bus (the private bus of the tests).
    #[must_use]
    pub fn with_target(app_id: &str, bus_address: &str) -> Self {
        Self {
            app_id: app_id.to_owned(),
            bus_address: Some(bus_address.to_owned()),
            timeout: START_TIMEOUT,
        }
    }
}

#[cfg(target_os = "linux")]
impl Default for DbusActivation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl Launcher for DbusActivation {
    fn launch(&self, budget: Duration) -> Result<(), IpcError> {
        use zbus::connection::Builder;
        use zbus::fdo::DBusProxy;
        use zbus::names::WellKnownName;

        let failed = |what: &str, e: &dyn std::fmt::Display| {
            IpcError::StartFailed(format!("D-Bus activation of {}: {what}: {e}", self.app_id))
        };
        zbus::block_on(async {
            let builder = match &self.bus_address {
                Some(address) => Builder::address(address.as_str()),
                None => Builder::session(),
            };
            let connection = builder
                .method_timeout(self.timeout.min(budget))
                .build()
                .await
                .map_err(|e| failed("connection", &e))?;
            let name = WellKnownName::try_from(self.app_id.as_str())
                .map_err(|e| failed("application ID", &e))?;
            // `Success` and `AlreadyRunning` both mean that the name is owned.
            DBusProxy::new(&connection)
                .await
                .map_err(|e| failed("bus proxy", &e))?
                .start_service_by_name(name, 0)
                .await
                .map(|_| ())
                .map_err(|e| failed("StartServiceByName", &e))
        })
    }
}

/// Launch by running a program, detached (macOS, decision D13; `kropa-app` inside the `.app`).
/// Standard streams are closed and the child leaves the process group of the command line. The
/// descriptors are not inherited because `kropa` marks them close-on-exec at start.
#[derive(Debug, Clone)]
pub struct Spawn {
    program: PathBuf,
    args: Vec<String>,
}

impl Spawn {
    /// Runs `program` with `args`.
    #[must_use]
    pub fn new(program: &Path, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }
}

impl Launcher for Spawn {
    fn launch(&self, _budget: Duration) -> Result<(), IpcError> {
        use std::os::unix::process::CommandExt;
        let child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| {
                IpcError::StartFailed(format!("cannot run {}: {e}", self.program.display()))
            })?;
        // The child is not waited for: it is the application, and outlives the command line.
        drop(child);
        Ok(())
    }
}

/// Connects to the application of this user, launching it first if nobody listens.
///
/// `wait` is the budget of the whole call, not of one step: the first connection, the launch (which
/// is given what is left), the pauses and each new connection (`hello` included) all count against
/// one deadline. When it passes, the error is `daemon_start_timeout`; a graphical application that
/// needs more than `wait` to start therefore ends in that error even if it is up a moment later, and
/// the next command finds it running. The call never takes noticeably longer than `wait`
/// (`docs/cli.md`, "Delays").
///
/// # Errors
/// Everything [`Client::connect`] can refuse (nothing is launched for those), `daemon_start_failed`
/// when the launch fails, `daemon_start_timeout` when the deadline passes.
pub async fn ensure_running(launcher: &dyn Launcher, wait: Duration) -> Result<Client, IpcError> {
    ensure_running_in(&runtime_dir()?, launcher, wait).await
}

/// [`ensure_running`] for the socket of `dir`.
///
/// # Errors
/// See [`ensure_running`].
pub async fn ensure_running_in(
    dir: &Path,
    launcher: &dyn Launcher,
    wait: Duration,
) -> Result<Client, IpcError> {
    let deadline = Instant::now() + wait;
    let socket = socket_path(dir);
    if crate::dir::dir_exists(dir) {
        // An unsafe directory is refused before anything is launched into it.
        crate::dir::verify_dir(dir, crate::dir::current_uid())?;
    }
    // The directory may not exist yet: a missing socket is "not running", not a refusal.
    match connect_before(&socket, deadline, wait).await {
        Err(IpcError::NotRunning(_)) => {}
        other => return other,
    }
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(IpcError::StartTimeout(wait));
    }
    launcher.launch(left)?;
    loop {
        match connect_before(&socket, deadline, wait).await {
            Err(IpcError::NotRunning(_)) => {}
            other => return other,
        }
        if Instant::now() >= deadline {
            return Err(IpcError::StartTimeout(wait));
        }
        tokio::time::sleep(POLL.min(deadline.saturating_duration_since(Instant::now()))).await;
    }
}

/// [`connect_if_present`], cut at `deadline`.
async fn connect_before(
    socket: &Path,
    deadline: Instant,
    wait: Duration,
) -> Result<Client, IpcError> {
    tokio::time::timeout_at(deadline.into(), connect_if_present(socket))
        .await
        .unwrap_or(Err(IpcError::StartTimeout(wait)))
}

async fn connect_if_present(socket: &Path) -> Result<Client, IpcError> {
    match std::fs::symlink_metadata(socket) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(IpcError::NotRunning(socket.to_owned()))
        }
        _ => Client::connect(socket).await,
    }
}
