// SPDX-License-Identifier: GPL-3.0-or-later

//! The graphical application as the command line sees it: which launcher starts it, and how an IPC
//! failure becomes an error with exit code 14 (`docs/cli.md` section 3).
//!
//! The commands that call the application (`capture area`, lot 2.5) build on this module.

use kropa_core::ExitCode;
use kropa_i18n::Localizer;
use kropa_ipc::{IpcError, Launcher};

use crate::errors::CliError;

/// Name of the application binary on this system, as messages must name it.
#[cfg(target_os = "macos")]
pub const APP_BINARY: &str = "kropa-app";
/// Name of the application binary on this system, as messages must name it.
#[cfg(not(target_os = "macos"))]
pub const APP_BINARY: &str = "kropa-gtk";

/// The launcher of this system: D-Bus activation on Linux (decision D1). On macOS the launch is the
/// detached `kropa-app` of the `.app` (decision D13, lot 2.1c), which does not exist yet: the
/// launcher then fails explicitly instead of pretending.
#[must_use]
pub fn launcher() -> Box<dyn Launcher> {
    #[cfg(target_os = "linux")]
    {
        Box::new(kropa_ipc::DbusActivation::new())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Box::new(Unavailable)
    }
}

#[cfg(not(target_os = "linux"))]
struct Unavailable;

#[cfg(not(target_os = "linux"))]
impl Launcher for Unavailable {
    fn launch(&self) -> Result<(), IpcError> {
        Err(IpcError::StartFailed(
            "launching kropa-app is not implemented yet (lot 2.1c)".to_owned(),
        ))
    }
}

/// Fluent id of the message for a reason: `error-ipc-` and the reason with dashes.
fn message_id(error: &IpcError) -> String {
    match error {
        IpcError::Rpc(_) => "error-ipc-rpc".to_owned(),
        other => format!(
            "error-ipc-{}",
            other.reason().unwrap_or("ipc_protocol_error").replace('_', "-")
        ),
    }
}

/// The exit code of an IPC error: 14, or the one the application chose.
fn exit_code(error: &IpcError) -> ExitCode {
    match error.exit_code() {
        1 => ExitCode::Internal,
        2 => ExitCode::Usage,
        3 => ExitCode::Cancelled,
        4 => ExitCode::PermissionDenied,
        5 => ExitCode::Unsupported,
        6 => ExitCode::Config,
        7 => ExitCode::SecretUnavailable,
        8 => ExitCode::Network,
        9 => ExitCode::UploadFailed,
        10 => ExitCode::CaptureFailed,
        11 => ExitCode::SecurityRefused,
        12 => ExitCode::InteractionRequired,
        15 => ExitCode::Io,
        // 14, and a code that this build does not know: the application is not behaving.
        _ => ExitCode::DaemonUnavailable,
    }
}

/// Turns an IPC failure into a command-line error, reason included.
#[must_use]
pub fn to_cli_error(error: &IpcError, loc: &Localizer) -> CliError {
    let detail = error.to_string();
    let mut cli = CliError::new(
        exit_code(error),
        loc,
        &message_id(error),
        &[("app", APP_BINARY), ("detail", &detail)],
    )
    .at_step("ipc");
    if let Some(reason) = error.reason() {
        cli = cli.with_reason(reason);
    }
    cli
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use kropa_i18n::{Lang, Localizer};
    use kropa_ipc::RpcError;

    use super::*;

    fn errors() -> Vec<IpcError> {
        vec![
            IpcError::NoRuntimeDir("unset".into()),
            IpcError::UnsafeRuntimeDir {
                path: PathBuf::from("/run/user/1/kropa"),
                why: "mode 0755".into(),
            },
            IpcError::NotRunning(PathBuf::from("/s")),
            IpcError::StartFailed("x".into()),
            IpcError::StartTimeout(Duration::from_secs(5)),
            IpcError::PeerUidMismatch { peer: 1, expected: 2 },
            IpcError::VersionMismatch { server: 2, client: 1 },
            IpcError::Protocol("bad".into()),
            IpcError::Rpc(RpcError::new(3, "cancelled", Some("overlay_escape"), "Escape")),
        ]
    }

    #[test]
    fn every_failure_has_a_message_in_both_languages_and_keeps_its_reason() {
        for lang in [Lang::En, Lang::Fr] {
            let loc = Localizer::new(lang).unwrap();
            for error in errors() {
                let cli = to_cli_error(&error, &loc);
                assert!(!cli.message.is_empty());
                assert!(
                    !cli.message.contains("error-ipc"),
                    "missing message for {error}: {}",
                    cli.message
                );
                assert_eq!(cli.reason.as_deref(), error.reason());
                assert_eq!(cli.code.code(), error.exit_code(), "{error}");
            }
        }
    }

    #[test]
    fn the_application_error_keeps_its_own_exit_code_and_text() {
        let loc = Localizer::new(Lang::En).unwrap();
        let cli = to_cli_error(&errors().pop().unwrap(), &loc);
        assert_eq!(cli.code, ExitCode::Cancelled);
        assert_eq!(cli.message, "Escape");
    }
}
