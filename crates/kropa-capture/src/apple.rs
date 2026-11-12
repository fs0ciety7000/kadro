// SPDX-License-Identifier: GPL-3.0-or-later

//! Decisions of the macOS backend that need no system call: what a version number allows, which
//! `.app` a path belongs to, how a `ScreenCaptureKit` error is read, and the order of the permission
//! check.
//!
//! They live outside the `macos` module so that Linux CI runs their tests too; the `macos` module
//! only feeds them with what the system answered.

use std::path::{Path, PathBuf};

/// Oldest macOS Kropa supports (`docs/decisions.md`).
pub const MIN_MACOS_MAJOR: u32 = 15;

/// `SCStreamErrorUserDeclined`: `ScreenCaptureKit`'s answer when the Screen Recording permission is
/// missing or does not apply to this process.
pub const SC_USER_DECLINED: isize = -3801;

/// A macOS version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OsVersion {
    /// Major number (15 for Sequoia, 26 for Tahoe).
    pub major: u32,
    /// Minor number.
    pub minor: u32,
    /// Patch number.
    pub patch: u32,
}

impl OsVersion {
    /// Whether Kropa supports this version.
    #[must_use]
    pub fn supported(self) -> bool {
        self.major >= MIN_MACOS_MAJOR
    }
}

impl std::fmt::Display for OsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The `.app` bundle an executable runs from, when it is `X.app/Contents/MacOS/<exe>`.
///
/// Permissions of macOS (TCC) belong to a bundle, not to a bare binary: `kropa doctor` reports
/// whether this is one.
#[must_use]
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let mut parts = exe.ancestors();
    parts.next()?; // the executable itself
    let macos_dir = parts.next()?;
    let contents = parts.next()?;
    let app = parts.next()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = app
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (is(macos_dir, "MacOS") && is(contents, "Contents") && is_app).then(|| app.to_owned())
}

/// How a `ScreenCaptureKit` error is to be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SckError {
    /// The permission is missing, or macOS does not link it to this process.
    UserDeclined,
    /// Any other error.
    Other,
}

/// Classifies an `NSError` of `ScreenCaptureKit` from its domain and code.
#[must_use]
pub fn classify_sck_error(domain: &str, code: isize) -> SckError {
    if code == SC_USER_DECLINED && domain.contains("ScreenCaptureKit") {
        SckError::UserDeclined
    } else {
        SckError::Other
    }
}

/// Result of the permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// The permission is in force.
    Granted,
    /// The permission is missing; if it was just requested, the user has not granted it yet.
    Denied,
}

/// Checks the permission without asking, and asks (once) only when it is missing.
///
/// `preflight` reads the state and never shows anything; `request` may show the system dialog.
/// The permission cannot be granted while the process runs (macOS asks to reopen the application),
/// so after a request the answer is read again, and a `false` is final for this run: the caller
/// reports it with the way to the setting instead of waiting for it.
pub fn ensure_permission(
    preflight: impl Fn() -> bool,
    request: impl FnOnce() -> bool,
) -> Permission {
    if preflight() {
        return Permission::Granted;
    }
    let granted_now = request();
    if granted_now || preflight() {
        Permission::Granted
    } else {
        Permission::Denied
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::{
        OsVersion, Permission, SC_USER_DECLINED, SckError, bundle_of, classify_sck_error,
        ensure_permission,
    };

    fn v(major: u32, minor: u32) -> OsVersion {
        OsVersion {
            major,
            minor,
            patch: 0,
        }
    }

    #[test]
    fn macos_14_is_too_old_and_15_is_supported() {
        assert!(!v(14, 7).supported());
        assert!(v(15, 0).supported());
        assert!(v(26, 1).supported());
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(v(15, 10) > v(15, 9));
        assert_eq!(v(15, 1).to_string(), "15.1.0");
    }

    #[test]
    fn a_binary_in_an_app_bundle_is_recognised() {
        let exe = Path::new("/Applications/Kropa.app/Contents/MacOS/kropa");
        assert_eq!(
            bundle_of(exe).as_deref(),
            Some(Path::new("/Applications/Kropa.app"))
        );
    }

    #[test]
    fn the_extension_of_the_bundle_is_not_case_sensitive() {
        let exe = Path::new("/tmp/Kropa.APP/Contents/MacOS/kropa");
        assert!(bundle_of(exe).is_some());
    }

    #[test]
    fn bare_binaries_are_not_bundles() {
        for path in [
            "/usr/local/bin/kropa",
            "/Users/lea/kropa/target/release/kropa",
            "/Applications/Kropa.app/Contents/Resources/kropa",
            "/Applications/Kropa.app/kropa",
            "/Applications/Kropa/Contents/MacOS/kropa",
            "/Contents/MacOS/kropa",
            "kropa",
            "",
        ] {
            assert_eq!(bundle_of(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn only_the_declined_code_of_screencapturekit_is_a_permission_problem() {
        let domain = "com.apple.ScreenCaptureKit.SCStreamErrorDomain";
        assert_eq!(
            classify_sck_error(domain, SC_USER_DECLINED),
            SckError::UserDeclined
        );
        assert_eq!(classify_sck_error(domain, -3811), SckError::Other);
        assert_eq!(
            classify_sck_error("NSCocoaErrorDomain", SC_USER_DECLINED),
            SckError::Other
        );
    }

    #[test]
    fn a_granted_permission_is_never_requested() {
        let asked = Cell::new(false);
        let got = ensure_permission(
            || true,
            || {
                asked.set(true);
                true
            },
        );
        assert_eq!(got, Permission::Granted);
        assert!(!asked.get(), "no dialog when the permission is in force");
    }

    #[test]
    fn a_missing_permission_is_requested_once_and_stays_denied() {
        let asked = Cell::new(0);
        let reads = Cell::new(0);
        let got = ensure_permission(
            || {
                reads.set(reads.get() + 1);
                false
            },
            || {
                asked.set(asked.get() + 1);
                false
            },
        );
        assert_eq!(got, Permission::Denied);
        assert_eq!(asked.get(), 1);
        assert_eq!(reads.get(), 2, "read before and after the request");
    }

    #[test]
    fn a_permission_that_appears_after_the_request_is_granted() {
        let state = Cell::new(false);
        let got = ensure_permission(
            || state.get(),
            || {
                state.set(true);
                false
            },
        );
        assert_eq!(got, Permission::Granted);
    }
}
