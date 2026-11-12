// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen capture abstraction of Kropa, with an explicit status and fallback per platform.
//!
//! [`CaptureBackend`] is the contract every platform fills: the Linux portal backend lives in
//! `linux` (compiled on Linux only), the macOS `ScreenCaptureKit` one in `macos` (macOS only). A
//! backend never reports a success it did not get: what it cannot do is a [`CaptureError`] with a
//! stable reason, and what is degraded shows in its [`Diagnosis`]. Nothing here depends on a
//! toolkit; the async runtime needed by D-Bus stays inside the Linux backend, and the Objective-C
//! calls (the only `unsafe` code of this crate) inside the macOS one.
//!
//! The [`clipboard`] module is the other half: [`Clipboard`] with the same rule (a status that is the
//! real effect, never a simulated success).
//!
//! The backend is chosen from what the system offers (portals present, interface versions), never
//! from the name of the desktop: `XDG_CURRENT_DESKTOP` is only reported for information.
//!
//! [`screens`] and [`apple`] hold the logic of the macOS backend that needs no system call (which
//! screen a name designates, how several screens share one canvas, how the permission is checked),
//! so that it is tested on every platform.

pub mod apple;
pub mod clipboard;
mod diagnosis;
mod error;
pub mod hud;
pub mod last_area;
mod permission;
mod png;
pub mod region;
pub mod screens;
pub mod selection;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

pub use clipboard::{
    ALL_CLIPBOARD_REASONS, Clipboard, ClipboardOutcome, ClipboardReason, FakeClipboard, FakeCopy,
    TextKind, UnavailableClipboard, default_clipboard,
};
pub use diagnosis::{
    ALL_FACT_KEYS, ALL_FALLBACKS, ALL_FEATURES, ALL_REASONS, Capabilities, Capability, Diagnosis,
    Fact, FactValue, Status,
};
pub use error::{ALL_ERROR_REASONS, CaptureError, Denial, Failure, Unsupported};
pub use hud::{HelpLayout, HudLayout, hud_layout};
pub use kropa_core::ImageFormat;
pub use last_area::{LastAreaError, StoredArea, default_last_area_path, last_area_path};
pub use permission::{ALL_RESET_REASONS, ResetError, ResetOutcome};
pub use png::{CleanPng, PngError, sanitize as sanitize_png};
pub use region::{Loupe, Region, RgbaView, crop_png, magnifier};
pub use selection::{
    Input, Key, MapError, Monitor, MonitorGeom, Outcome, PxRect, RectF, Selection, map_monitors,
};

#[cfg(feature = "test-support")]
pub use png::{build_png, sample_png};

/// What a capture covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    /// All screens as one image.
    Screen,
    /// One screen.
    Output,
}

impl CaptureMode {
    /// Name used in file names (`{mode}`) and in JSON (`capture.mode`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Output => "output",
        }
    }
}

/// What to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRequest<'a> {
    /// All screens or one.
    pub mode: CaptureMode,
    /// For [`CaptureMode::Output`]: the screen name; `None` is the primary screen.
    pub output: Option<&'a str>,
}

/// The mechanism that produced an image (`capture.backend` in JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BackendId {
    /// The `org.freedesktop.portal.Screenshot` portal.
    PortalScreenshot,
    /// `SCScreenshotManager` of `ScreenCaptureKit` (macOS).
    ScreenCaptureKit,
}

impl BackendId {
    /// Name used in JSON output and in `kropa doctor`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PortalScreenshot => "portal-screenshot",
            Self::ScreenCaptureKit => "screencapturekit",
        }
    }
}

/// Something worth saying about how an image was obtained, shown with `-v` (stable codes).
pub type Note = &'static str;

/// Notes a backend may attach to a capture.
pub const ALL_NOTES: [Note; 2] = ["app_id_registered", "app_id_registration_unavailable"];

/// An image delivered by a backend, already in its final encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedImage {
    /// Encoded file content, without metadata.
    pub data: Vec<u8>,
    /// Encoding of `data`.
    pub format: ImageFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Mechanism that produced the image.
    pub backend: BackendId,
    /// Screen the image shows (`None` for all screens).
    pub output: Option<String>,
    /// Scale factor of the screen when the backend knows it.
    pub scale: Option<f64>,
    /// How the image was obtained.
    pub notes: Vec<Note>,
}

/// A way to take screenshots on this system.
pub trait CaptureBackend {
    /// Which mechanism this is.
    fn id(&self) -> BackendId;

    /// Image formats this backend can deliver without conversion.
    fn formats(&self) -> &'static [ImageFormat];

    /// Detects, right now, what works on this system. May query the system (D-Bus), never opens a
    /// dialog and never captures anything.
    fn diagnose(&self) -> Diagnosis;

    /// Refuses, without side effect, a request this backend can never satisfy, so that the caller
    /// does not wait for a delay or a dialog first.
    ///
    /// # Errors
    /// [`CaptureError::Unsupported`] with the reason.
    fn check(&self, _request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
        Ok(())
    }

    /// Asks for the permission the capture needs, so that a refusal comes before any `--delay`.
    /// May show a system dialog; never captures anything. The default asks for nothing: the Linux
    /// portal asks for its permission itself, when the screenshot is requested.
    ///
    /// # Errors
    /// [`CaptureError::PermissionDenied`] when the permission is not granted.
    fn authorize(&self, _request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
        Ok(())
    }

    /// Makes the system forget the screenshot permission it stored for Kropa, so that the next
    /// capture asks again. The default does nothing and says so: a platform whose permission is
    /// out of Kropa's reach (macOS) answers [`ResetError::Unsupported`].
    ///
    /// # Errors
    /// [`ResetError`]: unsupported here, store unreachable, or an unexpected answer.
    fn reset_permission(&self) -> Result<ResetOutcome, ResetError> {
        Err(ResetError::Unsupported)
    }

    /// Takes the screenshot. May wait for the user (permission dialog). Fails like
    /// [`CaptureBackend::check`] on a request that cannot be satisfied.
    ///
    /// # Errors
    /// [`CaptureError`]: refusal (exit code 4), unsupported request (5) or failure (10).
    fn capture(&self, request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError>;
}

/// The backend this system offers, or `None` when the platform has none.
#[must_use]
pub fn default_backend() -> Option<Box<dyn CaptureBackend>> {
    #[cfg(target_os = "linux")]
    {
        Some(Box::new(linux::PortalScreenshot::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(macos::ScreenCaptureKit::new()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}
