// SPDX-License-Identifier: GPL-3.0-or-later

//! Forgetting the screenshot permission the system stored for Kropa, so that it is asked again.
//!
//! On Linux the answer lives in the portals' permission store (table `screenshot`); on macOS it
//! belongs to the system (TCC) and Kropa has no right to change it. The outcome says what really
//! happened: an entry removed, no entry to remove, or why nothing could be done.

use kropa_core::ExitCode;

/// What a reset did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResetOutcome {
    /// The stored answer existed and was deleted: the next capture asks again.
    Removed,
    /// The store had no answer for Kropa: nothing was stored, nothing was changed.
    Absent,
}

impl ResetOutcome {
    /// Stable name (`status` in the JSON of the command).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Absent => "absent",
        }
    }
}

/// Why a reset could not be done.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResetError {
    /// This platform keeps the permission out of Kropa's reach (macOS: System Settings).
    #[error("permission reset unsupported on this platform")]
    Unsupported,
    /// Nobody to talk to: no session bus, or no permission store on it.
    #[error("permission store unreachable: {0}")]
    StoreUnreachable(String),
    /// The store answered with an error that is not "no such entry".
    #[error("permission store error: {0}")]
    StoreError(String),
}

/// Every reason a [`ResetError`] can carry, for tests of the translations.
pub const ALL_RESET_REASONS: [&str; 3] = [
    "permission_reset_unsupported",
    "permission_store_unreachable",
    "permission_store_error",
];

impl ResetError {
    /// Stable machine reason (`error.reason` in JSON).
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Unsupported => ALL_RESET_REASONS[0],
            Self::StoreUnreachable(_) => ALL_RESET_REASONS[1],
            Self::StoreError(_) => ALL_RESET_REASONS[2],
        }
    }

    /// Exit code of `docs/cli.md` section 3: the function is not available here (5), or the store
    /// failed in a way nobody planned for (1).
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Unsupported | Self::StoreUnreachable(_) => ExitCode::Unsupported,
            Self::StoreError(_) => ExitCode::Internal,
        }
    }

    /// Technical detail (English, never a secret), when there is one.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Unsupported => None,
            Self::StoreUnreachable(d) | Self::StoreError(d) => Some(d),
        }
    }
}

#[cfg(test)]
mod tests {
    use kropa_core::ExitCode;

    use super::{ALL_RESET_REASONS, ResetError, ResetOutcome};

    #[test]
    fn reasons_and_codes_follow_the_cli_spec() {
        let all = [
            ResetError::Unsupported,
            ResetError::StoreUnreachable("x".into()),
            ResetError::StoreError("x".into()),
        ];
        let reasons: Vec<_> = all.iter().map(ResetError::reason).collect();
        assert_eq!(reasons, ALL_RESET_REASONS);
        assert_eq!(all[0].exit_code(), ExitCode::Unsupported);
        assert_eq!(all[1].exit_code(), ExitCode::Unsupported);
        assert_eq!(all[2].exit_code(), ExitCode::Internal);
        assert_eq!(all[0].detail(), None);
        assert_eq!(all[1].detail(), Some("x"));
    }

    #[test]
    fn outcomes_have_stable_names() {
        assert_eq!(ResetOutcome::Removed.as_str(), "removed");
        assert_eq!(ResetOutcome::Absent.as_str(), "absent");
    }
}
