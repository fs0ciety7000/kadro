// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a capture did not happen, with the exit code and a stable machine reason for each case.

use kropa_core::ExitCode;

/// A refusal by the user or by the platform (maps to `permission_denied`, code 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Denial {
    /// The portal answered "other" (response 2): GNOME and KDE answer so when the permission was
    /// denied, and the portal cannot be told apart from a backend failure.
    PortalDeclined,
    /// The request was dismissed (response 1), e.g. the permission dialog was closed.
    PortalDismissed,
    /// The portal replied with the D-Bus error `NotAllowed`.
    NotAllowed,
}

/// A feature this platform or backend cannot offer (maps to `unsupported`, code 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unsupported {
    /// The backend only returns all screens as one image: one screen cannot be isolated.
    OutputNotIsolable,
    /// The backend cannot deliver the configured image format.
    ImageFormat,
}

/// A capture that failed for a reason other than a refusal (maps to `capture_failed`, code 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// No session bus: there is no graphical session to ask.
    NoSessionBus,
    /// The portal frontend (or its Screenshot interface) is not installed or cannot be started.
    PortalUnavailable,
    /// The portal answered with an error that is not a refusal.
    PortalError,
    /// The portal never answered.
    NoResponse,
    /// The portal answered with something that is not a usable image location.
    InvalidResponse,
    /// The image the portal produced could not be read.
    FileUnreadable,
    /// The image is not a valid PNG.
    InvalidImage,
}

/// Why a capture did not produce an image.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// The user or the portal refused (exit code 4).
    #[error("capture refused: {}", .0.reason())]
    PermissionDenied(Denial),
    /// The platform cannot do this (exit code 5).
    #[error("capture unsupported: {}", .0.reason())]
    Unsupported(Unsupported),
    /// The capture failed (exit code 10). `detail` is technical English text, for `-v`.
    #[error("capture failed: {}: {detail}", .kind.reason())]
    Failed {
        /// What failed.
        kind: Failure,
        /// Technical detail (never a secret).
        detail: String,
    },
}

impl Denial {
    /// Stable machine reason (`error.reason` in JSON).
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::PortalDeclined => "portal_declined",
            Self::PortalDismissed => "portal_dismissed",
            Self::NotAllowed => "portal_not_allowed",
        }
    }
}

impl Unsupported {
    /// Stable machine reason (`error.reason` in JSON).
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::OutputNotIsolable => "output_not_isolable",
            Self::ImageFormat => "image_format_unsupported",
        }
    }
}

impl Failure {
    /// Stable machine reason (`error.reason` in JSON).
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoSessionBus => "no_session_bus",
            Self::PortalUnavailable => "portal_unavailable",
            Self::PortalError => "portal_error",
            Self::NoResponse => "portal_no_response",
            Self::InvalidResponse => "portal_invalid_response",
            Self::FileUnreadable => "portal_file_unreadable",
            Self::InvalidImage => "invalid_image",
        }
    }
}

/// Every reason a [`CaptureError`] can carry, for tests of the translations.
pub const ALL_ERROR_REASONS: [&str; 12] = [
    "portal_declined",
    "portal_dismissed",
    "portal_not_allowed",
    "output_not_isolable",
    "image_format_unsupported",
    "no_session_bus",
    "portal_unavailable",
    "portal_error",
    "portal_no_response",
    "portal_invalid_response",
    "portal_file_unreadable",
    "invalid_image",
];

impl CaptureError {
    /// A failure with its technical detail.
    #[must_use]
    pub fn failed(kind: Failure, detail: impl Into<String>) -> Self {
        Self::Failed {
            kind,
            detail: detail.into(),
        }
    }

    /// Exit code of `docs/cli.md` section 3.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::PermissionDenied(_) => ExitCode::PermissionDenied,
            Self::Unsupported(_) => ExitCode::Unsupported,
            Self::Failed { .. } => ExitCode::CaptureFailed,
        }
    }

    /// Stable machine reason (`error.reason` in JSON).
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::PermissionDenied(d) => d.reason(),
            Self::Unsupported(u) => u.reason(),
            Self::Failed { kind, .. } => kind.reason(),
        }
    }

    /// Technical detail, when there is one.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Failed { detail, .. } => Some(detail),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use kropa_core::ExitCode;

    use super::{ALL_ERROR_REASONS, CaptureError, Denial, Failure, Unsupported};

    #[test]
    fn exit_codes_follow_the_cli_spec() {
        assert_eq!(
            CaptureError::PermissionDenied(Denial::PortalDeclined).exit_code(),
            ExitCode::PermissionDenied
        );
        assert_eq!(
            CaptureError::Unsupported(Unsupported::OutputNotIsolable).exit_code(),
            ExitCode::Unsupported
        );
        assert_eq!(
            CaptureError::failed(Failure::NoSessionBus, "x").exit_code(),
            ExitCode::CaptureFailed
        );
    }

    #[test]
    fn the_list_of_reasons_is_complete_and_unique() {
        let all = [
            Denial::PortalDeclined.reason(),
            Denial::PortalDismissed.reason(),
            Denial::NotAllowed.reason(),
            Unsupported::OutputNotIsolable.reason(),
            Unsupported::ImageFormat.reason(),
            Failure::NoSessionBus.reason(),
            Failure::PortalUnavailable.reason(),
            Failure::PortalError.reason(),
            Failure::NoResponse.reason(),
            Failure::InvalidResponse.reason(),
            Failure::FileUnreadable.reason(),
            Failure::InvalidImage.reason(),
        ];
        assert_eq!(all, ALL_ERROR_REASONS);
        let mut sorted = all.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
    }

    #[test]
    fn display_carries_the_reason_and_the_detail() {
        let e = CaptureError::failed(Failure::PortalError, "boom");
        assert_eq!(e.to_string(), "capture failed: portal_error: boom");
        assert_eq!(e.detail(), Some("boom"));
        assert_eq!(e.reason(), "portal_error");
    }
}
