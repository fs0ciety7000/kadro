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

/// How long [`ensure_running`] waits for the socket after the launch.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Pause between two attempts to connect.
const POLL: Duration = Duration::from_millis(50);

/// Something that starts the application. It returns when the launch is done, not when the socket
/// answers: [`ensure_running`] waits for that.
pub trait Launcher {
    /// Starts the application.
    ///
    /// # Errors
    /// `daemon_start_failed`.
    fn launch(&self) -> Result<(), IpcError>;
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
    fn launch(&self) -> Result<(), IpcError> {
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
                .method_timeout(self.timeout)
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
    fn launch(&self) -> Result<(), IpcError> {
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

/// Connects to the application of this user, launching it first if nobody listens, and waiting
/// [`START_TIMEOUT`]-ish at most.
///
/// # Errors
/// Everything [`Client::connect`] can refuse (nothing is launched for those), `daemon_start_failed`
/// when the launch fails, `daemon_start_timeout` when the socket stays silent.
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
    let socket = socket_path(dir);
    if crate::dir::dir_exists(dir) {
        // An unsafe directory is refused before anything is launched into it.
        crate::dir::verify_dir(dir, crate::dir::current_uid())?;
    }
    // The directory may not exist yet: a missing socket is "not running", not a refusal.
    match connect_if_present(&socket).await {
        Err(IpcError::NotRunning(_)) => {}
        other => return other,
    }
    launcher.launch()?;
    let deadline = Instant::now() + wait;
    loop {
        match connect_if_present(&socket).await {
            Err(IpcError::NotRunning(_)) => {}
            other => return other,
        }
        if Instant::now() >= deadline {
            return Err(IpcError::StartTimeout(wait));
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn connect_if_present(socket: &Path) -> Result<Client, IpcError> {
    match std::fs::symlink_metadata(socket) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(IpcError::NotRunning(socket.to_owned()))
        }
        _ => Client::connect(socket).await,
    }
}
