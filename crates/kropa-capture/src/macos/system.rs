// SPDX-License-Identifier: GPL-3.0-or-later

//! What the system says about itself: its version, the login session, the Screen Recording
//! permission and the `.app` Kropa runs from.

use std::path::PathBuf;

use objc2_core_graphics::{
    CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{NSBundle, NSProcessInfo};

use crate::apple::{self, OsVersion, Permission};

/// Version of the running macOS.
pub(super) fn os_version() -> OsVersion {
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    let part = |n: isize| u32::try_from(n).unwrap_or(0);
    OsVersion {
        major: part(v.majorVersion),
        minor: part(v.minorVersion),
        patch: part(v.patchVersion),
    }
}

/// Whether this process belongs to a login session with a window server.
///
/// Over ssh, or from a daemon started outside the user's session, macOS has no such session and
/// ScreenCaptureKit cannot work: the capture must fail with a clear message instead of a crash or
/// a hang.
pub(super) fn has_graphical_session() -> bool {
    CGSessionCopyCurrentDictionary().is_some()
}

/// Whether the Screen Recording permission is granted to this process. Shows nothing.
///
/// A `true` is not a guarantee (an entry left by an older signature reads as granted and is
/// refused later): the capture itself has the last word.
pub(super) fn screen_recording_granted() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Checks the permission and asks for it once when it is missing (this may show the system dialog).
pub(super) fn ensure_screen_recording() -> Permission {
    apple::ensure_permission(
        || CGPreflightScreenCaptureAccess(),
        || CGRequestScreenCaptureAccess(),
    )
}

/// The `.app` this executable runs from, if any.
pub(super) fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // `current_exe` is not canonical when started through a symbolic link; the bundle is where the
    // real binary lives.
    let real = exe.canonicalize().unwrap_or(exe);
    apple::bundle_of(&real)
}

/// `CFBundleIdentifier` of the running bundle; `None` for a bare binary.
pub(super) fn bundle_identifier() -> Option<String> {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|id| id.to_string())
}
