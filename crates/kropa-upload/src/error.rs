// SPDX-License-Identifier: GPL-3.0-or-later

//! Errors of `kropa-upload` and their mapping to the exit codes and `error.reason` values of
//! `docs/cli.md` sections 3 and 4.
//!
//! No error holds a secret, a URL query or a server text that has not been masked: texts that come
//! from a response are [`MaskedText`]; everything else is built from fixed strings and names of the
//! user's own configuration.

use std::path::PathBuf;

use kropa_core::ExitCode;

use crate::masked::MaskedText;

/// Every `error.reason` this crate can produce, for the tests that check that each one has a
/// Fluent message in both languages (`error-upload-<reason>`, dashes for underscores).
pub const ALL_REASONS: &[&str] = &[
    "invalid_destination",
    "unknown_destination",
    "import_invalid",
    "import_unsupported",
    "import_too_large",
    "missing_variable",
    "invalid_variable",
    "secret_missing",
    "secret_store_not_delivered",
    "secret_store_error",
    "timeout",
    "connect",
    "tls",
    "network",
    "http_status",
    "url_missing",
    "url_invalid",
    "response_too_large",
    "bad_response",
    "file_too_large",
    "origin_not_allowed",
    "placement_not_allowed",
    "secret_in_output",
    "https_required",
    "cross_origin_redirect",
    "secret_in_argument",
    "secret_too_short",
    "secret_in_path",
    "import_host_mismatch",
    "io",
    "tls_cpu_unsupported",
];

/// Whether a failed upload is worth retrying (queue, lot 4.5): `408`, `429`, `5xx` and network
/// errors are transient, other `4xx` are final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// Try again later.
    Transient,
    /// Do not try again.
    Final,
}

/// Error of the upload engine, of the destination files and of the import.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UploadError {
    /// A destination, an imported file or a template is not valid (exit 6).
    #[error("{detail}")]
    Config {
        /// `invalid_destination`, `unknown_destination`, `import_invalid`, `import_unsupported`,
        /// `import_too_large`.
        reason: &'static str,
        /// What is wrong; never a secret.
        detail: String,
    },
    /// A `--var` is missing or not one of the choices (exit 2).
    #[error("{detail}")]
    Variable {
        /// `missing_variable` or `invalid_variable`.
        reason: &'static str,
        /// The variable name.
        detail: String,
    },
    /// A secret the destination needs is not in the store (exit 7).
    #[error("secret {name:?} is not set")]
    SecretMissing {
        /// Name of the secret.
        name: String,
    },
    /// The store failed (exit 7).
    #[error("{0}")]
    SecretStore(String),
    /// There is no secret store yet: lot 1.8 (exit 5).
    #[error("the secret store is not available yet")]
    SecretStoreNotDelivered,
    /// DNS, connection, timeout or TLS (exit 8).
    #[error("{detail}")]
    Network {
        /// `timeout`, `connect`, `tls` or `network`.
        reason: &'static str,
        /// A fixed text, never the URL.
        detail: String,
    },
    /// The server answered with a status outside `success_status` (exit 9).
    #[error("the server answered with status {status}")]
    Server {
        /// HTTP status.
        status: u16,
        /// The `error` template of the destination, or the start of the body, masked.
        message: MaskedText,
        /// Whether to try again.
        retry: Retry,
    },
    /// The server answered but the upload cannot be used (exit 9).
    #[error("{detail}")]
    Failed {
        /// `url_missing`, `url_invalid`, `response_too_large`, `bad_response`, `file_too_large`.
        reason: &'static str,
        /// What is wrong; never a response text.
        detail: String,
    },
    /// A rule of `docs/security/secrets.md` refused the operation (exit 11).
    #[error("{detail}")]
    Security {
        /// `origin_not_allowed`, `placement_not_allowed`, `secret_in_output`, `https_required`,
        /// `cross_origin_redirect`, `secret_in_argument`, `secret_too_short`, `secret_in_path`,
        /// `import_host_mismatch`.
        reason: &'static str,
        /// What is wrong; never a secret.
        detail: String,
    },
    /// The system cannot run the TLS provider (exit 5).
    #[error("this processor lacks features the TLS provider needs")]
    TlsUnsupported,
    /// A local file could not be read or written (exit 15).
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
}

impl UploadError {
    pub(crate) fn config(detail: impl Into<String>) -> Self {
        Self::Config {
            reason: "invalid_destination",
            detail: detail.into(),
        }
    }

    pub(crate) fn import(reason: &'static str, detail: impl Into<String>) -> Self {
        Self::Config {
            reason,
            detail: detail.into(),
        }
    }

    pub(crate) fn security(reason: &'static str, detail: impl Into<String>) -> Self {
        Self::Security {
            reason,
            detail: detail.into(),
        }
    }

    pub(crate) fn failed(reason: &'static str, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason,
            detail: detail.into(),
        }
    }

    /// Exit code of `docs/cli.md` section 3.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Config { .. } => ExitCode::Config,
            Self::Variable { .. } => ExitCode::Usage,
            Self::SecretMissing { .. } | Self::SecretStore(_) => ExitCode::SecretUnavailable,
            Self::SecretStoreNotDelivered | Self::TlsUnsupported => ExitCode::Unsupported,
            Self::Network { .. } => ExitCode::Network,
            Self::Server { .. } | Self::Failed { .. } => ExitCode::UploadFailed,
            Self::Security { .. } => ExitCode::SecurityRefused,
            Self::Io { .. } => ExitCode::Io,
        }
    }

    /// The machine sub-cause, `error.reason` in the JSON output.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Config { reason, .. }
            | Self::Variable { reason, .. }
            | Self::Network { reason, .. }
            | Self::Failed { reason, .. }
            | Self::Security { reason, .. } => reason,
            Self::SecretMissing { .. } => "secret_missing",
            Self::SecretStore(_) => "secret_store_error",
            Self::SecretStoreNotDelivered => "secret_store_not_delivered",
            Self::Server { .. } => "http_status",
            Self::TlsUnsupported => "tls_cpu_unsupported",
            Self::Io { .. } => "io",
        }
    }

    /// Whether a later attempt may succeed (`Retry::Transient`), for the queue of lot 4.5.
    #[must_use]
    pub fn retry(&self) -> Retry {
        match self {
            Self::Network { .. } => Retry::Transient,
            Self::Server { retry, .. } => *retry,
            _ => Retry::Final,
        }
    }

    /// The reason is a leak of the secret into the answer of the server (exit 11): the file may
    /// have been sent, but its link is withheld (`uploaded_link_withheld`).
    #[must_use]
    pub fn is_secret_in_output(&self) -> bool {
        matches!(
            self,
            Self::Security {
                reason: "secret_in_output",
                ..
            }
        )
    }

    /// HTTP status of a server error.
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Server { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// The text of the server, masked, if the error carries one; and whether masking hid a secret.
    #[must_use]
    pub fn server_message(&self) -> Option<&MaskedText> {
        match self {
            Self::Server { message, .. } => Some(message),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use kropa_core::ExitCode;

    use super::{ALL_REASONS, Retry, UploadError};

    #[test]
    fn exit_codes_follow_the_spec() {
        let cases = [
            (UploadError::config("x"), ExitCode::Config),
            (
                UploadError::SecretMissing { name: "t".into() },
                ExitCode::SecretUnavailable,
            ),
            (
                UploadError::Network {
                    reason: "timeout",
                    detail: "x".into(),
                },
                ExitCode::Network,
            ),
            (
                UploadError::failed("url_missing", "x"),
                ExitCode::UploadFailed,
            ),
            (
                UploadError::security("https_required", "x"),
                ExitCode::SecurityRefused,
            ),
            (UploadError::SecretStoreNotDelivered, ExitCode::Unsupported),
        ];
        for (error, code) in cases {
            assert_eq!(error.exit_code(), code, "{error}");
            assert!(ALL_REASONS.contains(&error.reason()), "{}", error.reason());
        }
    }

    #[test]
    fn only_network_errors_are_transient_by_default() {
        let net = UploadError::Network {
            reason: "connect",
            detail: String::new(),
        };
        assert_eq!(net.retry(), Retry::Transient);
        assert_eq!(UploadError::config("x").retry(), Retry::Final);
    }
}
