// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(
    unsafe_code,
    reason = "Objective-C and CoreGraphics calls; each `unsafe` block says why it is sound"
)]

//! macOS backend: `SCScreenshotManager` of `ScreenCaptureKit` through the generated `objc2` bindings.
//!
//! The Rust side is safe: everything that needs `unsafe` is in the private submodules, behind
//! functions that take and return plain Rust values, and every block carries a `SAFETY` comment.
//!
//! What the user sees, in order:
//! 1. [`CaptureBackend::check`] refuses what can never work, before any delay or dialog: macOS
//!    older than 15, no login session, a screen name that matches nothing.
//! 2. [`CaptureBackend::authorize`] reads the Screen Recording permission (TCC) without showing
//!    anything; when it is missing it asks the system once and then reports a refusal that says
//!    where to switch it on. macOS only applies a new permission to applications started
//!    afterwards, so the command is never kept waiting for it.
//! 3. [`CaptureBackend::capture`] takes the picture of each display at its native resolution (no
//!    pointer) and encodes it as PNG; several displays are drawn on one sRGB canvas.
//!
//! The permission belongs to the application macOS holds responsible for the process. Started from
//! Terminal, that is Terminal, not Kropa: see `docs/testing/manual-macos.md`.

mod displays;
mod image;
mod sck;
mod system;

use kropa_core::ImageFormat;
use objc2_core_graphics::CGImage;

use crate::apple::Permission;
use crate::screens::{self, LayoutError, Screen, SelectError};
use crate::{
    BackendId, Capabilities, Capability, CaptureBackend, CaptureError, CaptureMode, CaptureRequest,
    CapturedImage, CleanPng, Denial, Diagnosis, Fact, FactValue, Failure, Status, Unsupported,
    sanitize_png,
};

/// Screenshots through `ScreenCaptureKit`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScreenCaptureKit;

impl ScreenCaptureKit {
    /// The backend. Nothing is queried until it is used.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn select_error(error: &SelectError) -> CaptureError {
    CaptureError::failed(Failure::OutputNotFound, error.to_string())
}

fn layout_error(error: LayoutError) -> CaptureError {
    match error {
        LayoutError::Empty => CaptureError::failed(Failure::OutputNotFound, error.to_string()),
        LayoutError::InvalidScreen | LayoutError::TooLarge => {
            CaptureError::failed(Failure::ImageEncoding, error.to_string())
        }
    }
}

/// The screens a request covers.
fn targets(request: &CaptureRequest<'_>) -> Result<Vec<Screen>, CaptureError> {
    let connected = displays::connected();
    match request.mode {
        CaptureMode::Screen => {
            if connected.is_empty() {
                Err(select_error(&SelectError::NoScreen))
            } else {
                Ok(connected)
            }
        }
        CaptureMode::Output => screens::select(&connected, request.output)
            .map(|s| vec![s.clone()])
            .map_err(|e| select_error(&e)),
    }
}

fn describe(screen: &Screen) -> String {
    format!(
        "{} (id {}) {}x{} pt @{}x",
        screen.label(),
        screen.id,
        screen.width,
        screen.height,
        screen.scale
    )
}

impl CaptureBackend for ScreenCaptureKit {
    fn id(&self) -> BackendId {
        BackendId::ScreenCaptureKit
    }

    fn formats(&self) -> &'static [ImageFormat] {
        &[ImageFormat::Png]
    }

    fn diagnose(&self) -> Diagnosis {
        let version = system::os_version();
        let session = system::has_graphical_session();
        let granted = system::screen_recording_granted();
        let reason = if !version.supported() {
            Some("macos_too_old")
        } else if !session {
            Some("no_graphical_session")
        } else if !granted {
            Some("screen_recording_denied")
        } else {
            None
        };
        let capability = |feature| Capability {
            feature,
            status: if reason.is_some() {
                Status::Unavailable
            } else {
                Status::Available
            },
            reason,
            fallback: None,
        };
        let mut facts = vec![
            Fact::new("macos_version", FactValue::Text(version.to_string())),
            Fact::new("graphical_session", FactValue::Bool(session)),
            Fact::new("screen_recording", FactValue::Bool(granted)),
            Fact::new(
                "app_bundle",
                system::app_bundle().map_or(FactValue::Absent, |p| {
                    FactValue::Text(p.display().to_string())
                }),
            ),
            Fact::new(
                "bundle_identifier",
                system::bundle_identifier().map_or(FactValue::Absent, FactValue::Text),
            ),
        ];
        if session {
            let mut connected = displays::connected();
            connected.sort_by_key(|s| !s.primary);
            for screen in &connected {
                let key = if screen.primary {
                    "display_primary"
                } else {
                    "display"
                };
                facts.push(Fact::new(key, FactValue::Text(describe(screen))));
            }
        }
        Diagnosis {
            platform: "macos",
            backend: self.id().as_str(),
            capabilities: Capabilities {
                screen: capability("capture_screen"),
                output: capability("capture_output"),
            },
            facts,
        }
    }

    fn check(&self, request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
        if !system::os_version().supported() {
            return Err(CaptureError::Unsupported(Unsupported::MacosTooOld));
        }
        if !system::has_graphical_session() {
            return Err(CaptureError::failed(
                Failure::NoGraphicalSession,
                "the process has no login session with a window server",
            ));
        }
        // A screen that does not exist is known without any permission: say so before asking.
        targets(request).map(|_| ())
    }

    fn authorize(&self, _request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
        match system::ensure_screen_recording() {
            Permission::Granted => Ok(()),
            Permission::Denied => Err(CaptureError::PermissionDenied(
                Denial::ScreenRecordingDenied,
            )),
        }
    }

    fn capture(&self, request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError> {
        self.check(request)?;
        self.authorize(request)?;
        let chosen = targets(request)?;
        let mut frames = Vec::with_capacity(chosen.len());
        for screen in &chosen {
            frames.push(sck::capture_display(screen.id)?);
        }
        let (picture, scale) = match <[_; 1]>::try_from(frames) {
            // One display: its own picture, with the color profile of the display.
            Ok([only]) => (only, chosen[0].scale),
            Err(frames) => {
                let layout = screens::layout(&chosen).map_err(layout_error)?;
                let placed: Vec<_> = layout.placements.iter().copied().zip(frames).collect();
                let canvas = image::compose(&layout, &placed)
                    .map_err(|d| CaptureError::failed(Failure::ImageEncoding, d))?;
                (canvas, layout.scale)
            }
        };
        let png = encode(&picture)?;
        Ok(CapturedImage {
            data: png.data,
            format: ImageFormat::Png,
            width: png.width,
            height: png.height,
            backend: self.id(),
            output: match request.mode {
                CaptureMode::Screen => None,
                CaptureMode::Output => chosen.first().map(Screen::label),
            },
            scale: Some(scale),
            notes: Vec::new(),
        })
    }
}

fn encode(picture: &CGImage) -> Result<CleanPng, CaptureError> {
    let bytes =
        image::encode_png(picture).map_err(|d| CaptureError::failed(Failure::ImageEncoding, d))?;
    sanitize_png(&bytes).map_err(|e| CaptureError::failed(Failure::InvalidImage, e.to_string()))
}

#[cfg(test)]
mod tests {
    //! These tests need a Mac but not the Screen Recording permission: the CI runner has none.

    use super::ScreenCaptureKit;
    use crate::{CaptureBackend, CaptureMode, CaptureRequest, FactValue, ImageFormat};

    #[test]
    fn the_backend_delivers_png_only() {
        assert_eq!(ScreenCaptureKit::new().formats(), [ImageFormat::Png]);
    }

    #[test]
    fn diagnosis_reports_the_system_without_asking_for_anything() {
        let d = ScreenCaptureKit::new().diagnose();
        assert_eq!(d.platform, "macos");
        assert_eq!(d.backend, "screencapturekit");
        let has = |k: &str| d.facts.iter().any(|f| f.key == k);
        assert!(has("macos_version"));
        assert!(has("screen_recording"));
        assert!(has("app_bundle"));
        // The test binary is not in an .app.
        let bundle = d.facts.iter().find(|f| f.key == "app_bundle").unwrap();
        assert_eq!(bundle.value, FactValue::Absent);
    }

    #[test]
    fn every_fact_and_reason_of_the_diagnosis_is_a_known_code() {
        let d = ScreenCaptureKit::new().diagnose();
        for fact in &d.facts {
            assert!(crate::ALL_FACT_KEYS.contains(&fact.key), "{}", fact.key);
        }
        for cap in [&d.capabilities.screen, &d.capabilities.output] {
            assert!(crate::ALL_FEATURES.contains(&cap.feature));
            if let Some(reason) = cap.reason {
                assert!(crate::ALL_REASONS.contains(&reason), "{reason}");
            }
        }
    }

    #[test]
    fn an_unknown_screen_is_refused_before_any_permission_is_asked() {
        let backend = ScreenCaptureKit::new();
        let request = CaptureRequest {
            mode: CaptureMode::Output,
            output: Some("no such display"),
        };
        // On a runner without a login session this is `no_graphical_session`; with one, it is
        // `output_not_found`. Both are failures (exit 10), never a permission question.
        let err = backend.check(&request).unwrap_err();
        assert_eq!(err.exit_code(), kropa_core::ExitCode::CaptureFailed);
        assert!(
            ["output_not_found", "no_graphical_session"].contains(&err.reason()),
            "{err}"
        );
    }
}
