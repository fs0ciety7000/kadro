// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen capture abstraction of Kropa, with an explicit status and fallback per platform.
//!
//! [`CaptureBackend`] is the contract every platform fills: the Linux portal backend lives in
//! `linux` (compiled on Linux only), the macOS one arrives with lot 1.5. A backend never reports a
//! success it did not get: what it cannot do is a [`CaptureError`] with a stable reason, and what
//! is degraded shows in its [`Diagnosis`]. Nothing here depends on a toolkit; the async runtime
//! needed by D-Bus stays inside the Linux backend.
//!
//! The backend is chosen from what the system offers (portals present, interface versions), never
//! from the name of the desktop: `XDG_CURRENT_DESKTOP` is only reported for information.

mod diagnosis;
mod error;
mod png;

#[cfg(target_os = "linux")]
pub mod linux;

pub use diagnosis::{
    ALL_FACT_KEYS, ALL_FALLBACKS, ALL_FEATURES, ALL_REASONS, Capabilities, Capability, Diagnosis,
    Fact, FactValue, Status,
};
pub use error::{ALL_ERROR_REASONS, CaptureError, Denial, Failure, Unsupported};
pub use kropa_core::ImageFormat;
pub use png::{CleanPng, PngError, sanitize as sanitize_png};

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
}

impl BackendId {
    /// Name used in JSON output and in `kropa doctor`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PortalScreenshot => "portal-screenshot",
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

    /// Takes the screenshot. May wait for the user (permission dialog).
    ///
    /// # Errors
    /// [`CaptureError`]: refusal (exit code 4), unsupported request (5) or failure (10).
    fn capture(&self, request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError>;
}

/// The backend this system offers, or `None` when the platform has none yet (macOS before lot 1.5).
#[must_use]
pub fn default_backend() -> Option<Box<dyn CaptureBackend>> {
    #[cfg(target_os = "linux")]
    {
        Some(Box::new(linux::PortalScreenshot::new()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
