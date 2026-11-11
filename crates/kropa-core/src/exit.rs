// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable process exit codes (`docs/cli.md` section 3).

/// Exit code of the `kropa` command. The numeric values are a stable contract (SemVer from v1.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum ExitCode {
    /// Success.
    Ok = 0,
    /// Unexpected error (bug).
    Internal = 1,
    /// Invalid arguments, exclusive options, unknown subcommand.
    Usage = 2,
    /// Cancelled by the user.
    Cancelled = 3,
    /// Portal, TCC or keychain access refused.
    PermissionDenied = 4,
    /// Feature unavailable on this platform or desktop.
    Unsupported = 5,
    /// Invalid configuration or destination.
    Config = 6,
    /// Locked vault, wrong passphrase, missing secret.
    SecretUnavailable = 7,
    /// DNS, connection, timeout, TLS.
    Network = 8,
    /// The server answered but the upload failed.
    UploadFailed = 9,
    /// Capture failure other than a permission one.
    CaptureFailed = 10,
    /// Origin not allowed for a secret, HTTPS required, secret in an argument.
    SecurityRefused = 11,
    /// An answer is needed but input is not interactive.
    InteractionRequired = 12,
    /// `kropa-app` unreachable or incompatible IPC version.
    DaemonUnavailable = 14,
    /// File not found or unreadable, write impossible, existing file without `--force`.
    Io = 15,
    /// Interrupted (SIGINT).
    Interrupted = 130,
}

impl ExitCode {
    /// Numeric process exit status.
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Machine name used as `error.code` in JSON output; `None` for the interruption, which has none.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Ok => "ok",
            Self::Internal => "internal",
            Self::Usage => "usage",
            Self::Cancelled => "cancelled",
            Self::PermissionDenied => "permission_denied",
            Self::Unsupported => "unsupported",
            Self::Config => "config",
            Self::SecretUnavailable => "secret_unavailable",
            Self::Network => "network",
            Self::UploadFailed => "upload_failed",
            Self::CaptureFailed => "capture_failed",
            Self::SecurityRefused => "security_refused",
            Self::InteractionRequired => "interaction_required",
            Self::DaemonUnavailable => "daemon_unavailable",
            Self::Io => "io",
            Self::Interrupted => return None,
        })
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        Self::from(code.code())
    }
}

#[cfg(test)]
mod tests {
    use super::ExitCode;

    #[test]
    fn values_match_the_cli_spec() {
        let table = [
            (ExitCode::Ok, 0, Some("ok")),
            (ExitCode::Internal, 1, Some("internal")),
            (ExitCode::Usage, 2, Some("usage")),
            (ExitCode::Cancelled, 3, Some("cancelled")),
            (ExitCode::PermissionDenied, 4, Some("permission_denied")),
            (ExitCode::Unsupported, 5, Some("unsupported")),
            (ExitCode::Config, 6, Some("config")),
            (ExitCode::SecretUnavailable, 7, Some("secret_unavailable")),
            (ExitCode::Network, 8, Some("network")),
            (ExitCode::UploadFailed, 9, Some("upload_failed")),
            (ExitCode::CaptureFailed, 10, Some("capture_failed")),
            (ExitCode::SecurityRefused, 11, Some("security_refused")),
            (ExitCode::InteractionRequired, 12, Some("interaction_required")),
            (ExitCode::DaemonUnavailable, 14, Some("daemon_unavailable")),
            (ExitCode::Io, 15, Some("io")),
            (ExitCode::Interrupted, 130, None),
        ];
        for (exit, code, name) in table {
            assert_eq!(exit.code(), code);
            assert_eq!(exit.name(), name);
        }
    }
}
