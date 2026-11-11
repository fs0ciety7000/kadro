// SPDX-License-Identifier: GPL-3.0-or-later

//! The Screenshot portal as a [`CaptureBackend`].

use std::io::Read;
use std::path::{Path, PathBuf};

use ashpd::desktop::screenshot::Screenshot;
use ashpd::desktop::ResponseError;
use ashpd::zbus::{self, Connection, DBusError};
use ashpd::{AppID, Error as AshpdError, PortalError};
use kropa_core::{APP_ID, ImageFormat};

use super::probe;
use super::uri::{UriError, file_uri_to_path};
use crate::{
    BackendId, CaptureBackend, CaptureError, CaptureMode, CaptureRequest, CapturedImage, Denial,
    Diagnosis, Failure, Note, Unsupported, sanitize_png,
};

/// The portal never hands over more than this: a bigger file is not a screenshot.
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

/// Screenshots through `org.freedesktop.portal.Screenshot`.
///
/// The request is non interactive: the portal takes the picture itself, after asking the user for
/// the permission the first time. The portal returns the whole desktop as one image, so
/// [`CaptureMode::Output`] is refused (`output_not_isolable`) before anything is asked.
#[derive(Debug, Default, Clone)]
pub struct PortalScreenshot {
    connection: Option<Connection>,
}

impl PortalScreenshot {
    /// A backend that connects to the session bus when it first needs it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A backend on a given connection (tests, or a bus address chosen by the caller).
    #[must_use]
    pub fn with_connection(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
        }
    }

    async fn connect(&self) -> Result<Connection, CaptureError> {
        match &self.connection {
            Some(c) => Ok(c.clone()),
            None => Connection::session()
                .await
                .map_err(|e| CaptureError::failed(Failure::NoSessionBus, e.to_string())),
        }
    }

    async fn capture_screen(&self) -> Result<CapturedImage, CaptureError> {
        let connection = self.connect().await?;
        let mut notes = Vec::new();
        notes.extend(register_app_id(&connection).await);
        let request = Screenshot::request()
            .interactive(false)
            .connection(Some(connection))
            .send()
            .await
            .map_err(|e| map_failure(&classify(&e)))?;
        let shot = request.response().map_err(|e| map_failure(&classify(&e)))?;
        let path = file_uri_to_path(shot.uri().as_str()).map_err(map_uri_error)?;
        let bytes = read_image(&path)?;
        let png = sanitize_png(&bytes)
            .map_err(|e| CaptureError::failed(Failure::InvalidImage, e.to_string()))?;
        remove_if_temporary(&path);
        Ok(CapturedImage {
            data: png.data,
            format: ImageFormat::Png,
            width: png.width,
            height: png.height,
            backend: BackendId::PortalScreenshot,
            output: None,
            scale: None,
            notes,
        })
    }
}

impl CaptureBackend for PortalScreenshot {
    fn id(&self) -> BackendId {
        BackendId::PortalScreenshot
    }

    fn formats(&self) -> &'static [ImageFormat] {
        &[ImageFormat::Png]
    }

    fn diagnose(&self) -> Diagnosis {
        zbus::block_on(probe::diagnose(self.connection.as_ref()))
    }

    fn capture(&self, request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError> {
        match request.mode {
            CaptureMode::Output => Err(CaptureError::Unsupported(Unsupported::OutputNotIsolable)),
            CaptureMode::Screen => zbus::block_on(self.capture_screen()),
        }
    }
}

/// Tells the portal which application this process is (`org.freedesktop.host.portal.Registry`),
/// so that permissions are stored under the application identifier and not under an empty one.
/// Older portals do not have the interface: that is not an error, only a note.
async fn register_app_id(connection: &Connection) -> Option<Note> {
    if ashpd::is_sandboxed() {
        // A sandbox already gives the portal the application identifier.
        return None;
    }
    let registered = match AppID::try_from(APP_ID) {
        Ok(id) => ashpd::register_host_app_with_connection(connection.clone(), id)
            .await
            .is_ok(),
        Err(_) => false,
    };
    Some(if registered {
        "app_id_registered"
    } else {
        "app_id_registration_unavailable"
    })
}

fn read_image(path: &Path) -> Result<Vec<u8>, CaptureError> {
    let unreadable = |e: &std::io::Error| {
        CaptureError::failed(Failure::FileUnreadable, format!("{}: {e}", path.display()))
    };
    let file = std::fs::File::open(path).map_err(|e| unreadable(&e))?;
    let meta = file.metadata().map_err(|e| unreadable(&e))?;
    if !meta.is_file() {
        return Err(CaptureError::failed(
            Failure::FileUnreadable,
            format!("{}: not a regular file", path.display()),
        ));
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(CaptureError::failed(
            Failure::InvalidImage,
            format!("{}: larger than {MAX_IMAGE_BYTES} bytes", path.display()),
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    // `take` guards against a file that grows while it is read.
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| unreadable(&e))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(CaptureError::failed(
            Failure::InvalidImage,
            format!("{}: larger than {MAX_IMAGE_BYTES} bytes", path.display()),
        ));
    }
    Ok(bytes)
}

/// Removes the portal's own copy, when it is in a place meant for temporary files: a screenshot of
/// the whole desktop must not linger there. A file anywhere else (the Pictures folder, a document
/// portal mount) is the portal's or the user's business and is left alone. Best effort.
fn remove_if_temporary(path: &Path) {
    let temporary = temporary_roots().iter().any(|root| path.starts_with(root));
    let plain_file = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file());
    if temporary && plain_file {
        let _ = std::fs::remove_file(path);
    }
}

fn temporary_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    // `/run/user/<uid>/doc` is the document portal's mount: not ours to clean.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        roots.push(PathBuf::from(runtime).join("kropa"));
    }
    roots
}

fn map_uri_error(error: UriError) -> CaptureError {
    CaptureError::failed(Failure::InvalidResponse, error.to_string())
}

/// What went wrong with the portal call, stripped of the types of the D-Bus libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortalFailure {
    /// The request ended with response 1 (dismissed) or 2 (other).
    Response(Dismissal),
    /// A D-Bus error reply: its name and message.
    DbusError { name: String, message: String },
    /// The frontend does not implement the interface.
    InterfaceMissing,
    /// No `Response` signal came back.
    NoResponse,
    /// Anything else, as text.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dismissal {
    /// Response 1.
    Cancelled,
    /// Response 2.
    Other,
}

fn classify_zbus(error: &zbus::Error) -> PortalFailure {
    match error {
        zbus::Error::MethodError(name, description, _) => PortalFailure::DbusError {
            name: name.to_string(),
            message: description.clone().unwrap_or_default(),
        },
        zbus::Error::FDO(fdo) => PortalFailure::DbusError {
            name: fdo.name().to_string(),
            message: fdo.description().unwrap_or_default().to_owned(),
        },
        other => PortalFailure::Other(other.to_string()),
    }
}

pub(crate) fn classify(error: &AshpdError) -> PortalFailure {
    match error {
        AshpdError::Response(ResponseError::Cancelled) => {
            PortalFailure::Response(Dismissal::Cancelled)
        }
        AshpdError::Response(ResponseError::Other) => PortalFailure::Response(Dismissal::Other),
        AshpdError::Portal(PortalError::ZBus(inner)) | AshpdError::Zbus(inner) => {
            classify_zbus(inner)
        }
        AshpdError::Portal(portal) => PortalFailure::DbusError {
            name: portal.name().to_string(),
            message: portal.description().unwrap_or_default().to_owned(),
        },
        AshpdError::PortalNotFound(_) => PortalFailure::InterfaceMissing,
        AshpdError::NoResponse => PortalFailure::NoResponse,
        other => PortalFailure::Other(other.to_string()),
    }
}

/// D-Bus error names that mean "nobody to talk to" rather than "talked and failed".
fn is_service_missing(name: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
    ) || name.starts_with("org.freedesktop.DBus.Error.Spawn.")
}

pub(crate) fn map_failure(failure: &PortalFailure) -> CaptureError {
    match failure {
        PortalFailure::Response(Dismissal::Cancelled) => {
            CaptureError::PermissionDenied(Denial::PortalDismissed)
        }
        PortalFailure::Response(Dismissal::Other) => {
            CaptureError::PermissionDenied(Denial::PortalDeclined)
        }
        PortalFailure::DbusError { name, message } => {
            let detail = if message.is_empty() {
                name.clone()
            } else {
                format!("{name}: {message}")
            };
            match name.as_str() {
                "org.freedesktop.portal.Error.NotAllowed"
                | "org.freedesktop.DBus.Error.AccessDenied" => {
                    CaptureError::PermissionDenied(Denial::NotAllowed)
                }
                "org.freedesktop.portal.Error.Cancelled" => {
                    CaptureError::PermissionDenied(Denial::PortalDismissed)
                }
                "org.freedesktop.DBus.Error.NoReply"
                | "org.freedesktop.DBus.Error.Timeout"
                | "org.freedesktop.DBus.Error.TimedOut" => {
                    CaptureError::failed(Failure::NoResponse, detail)
                }
                n if is_service_missing(n) => {
                    CaptureError::failed(Failure::PortalUnavailable, detail)
                }
                _ => CaptureError::failed(Failure::PortalError, detail),
            }
        }
        PortalFailure::InterfaceMissing => CaptureError::failed(
            Failure::PortalUnavailable,
            "the portal frontend has no Screenshot interface",
        ),
        PortalFailure::NoResponse => {
            CaptureError::failed(Failure::NoResponse, "no Response signal received")
        }
        PortalFailure::Other(text) => CaptureError::failed(Failure::PortalError, text.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ashpd::zbus::fdo;
    use ashpd::{Error as AshpdError, PortalError};
    use kropa_core::ExitCode;

    use super::{Dismissal, PortalFailure, classify, map_failure, remove_if_temporary};
    use crate::{CaptureError, Denial, Failure};

    fn dbus(name: &str) -> PortalFailure {
        PortalFailure::DbusError {
            name: name.to_owned(),
            message: "m".to_owned(),
        }
    }

    #[test]
    fn refusals_are_permission_denied() {
        for (failure, reason) in [
            (
                PortalFailure::Response(Dismissal::Other),
                Denial::PortalDeclined,
            ),
            (
                PortalFailure::Response(Dismissal::Cancelled),
                Denial::PortalDismissed,
            ),
            (
                dbus("org.freedesktop.portal.Error.NotAllowed"),
                Denial::NotAllowed,
            ),
            (
                dbus("org.freedesktop.DBus.Error.AccessDenied"),
                Denial::NotAllowed,
            ),
            (
                dbus("org.freedesktop.portal.Error.Cancelled"),
                Denial::PortalDismissed,
            ),
        ] {
            let error = map_failure(&failure);
            assert_eq!(error, CaptureError::PermissionDenied(reason), "{failure:?}");
            assert_eq!(error.exit_code(), ExitCode::PermissionDenied);
        }
    }

    #[test]
    fn a_missing_portal_is_a_capture_failure() {
        for failure in [
            dbus("org.freedesktop.DBus.Error.ServiceUnknown"),
            dbus("org.freedesktop.DBus.Error.NameHasNoOwner"),
            dbus("org.freedesktop.DBus.Error.UnknownMethod"),
            dbus("org.freedesktop.DBus.Error.Spawn.ServiceNotFound"),
            PortalFailure::InterfaceMissing,
        ] {
            let error = map_failure(&failure);
            assert_eq!(error.reason(), "portal_unavailable", "{failure:?}");
            assert_eq!(error.exit_code(), ExitCode::CaptureFailed);
        }
    }

    #[test]
    fn other_failures_keep_their_detail() {
        let error = map_failure(&dbus("org.freedesktop.portal.Error.Failed"));
        assert_eq!(error.reason(), "portal_error");
        assert_eq!(
            error.detail(),
            Some("org.freedesktop.portal.Error.Failed: m")
        );
        assert_eq!(
            map_failure(&dbus("org.freedesktop.DBus.Error.NoReply")).reason(),
            "portal_no_response"
        );
        assert_eq!(
            map_failure(&PortalFailure::NoResponse).reason(),
            "portal_no_response"
        );
        assert_eq!(
            map_failure(&PortalFailure::Other("x".into())),
            CaptureError::failed(Failure::PortalError, "x")
        );
    }

    #[test]
    fn ashpd_errors_are_classified() {
        use ashpd::desktop::ResponseError;
        assert_eq!(
            classify(&AshpdError::Response(ResponseError::Cancelled)),
            PortalFailure::Response(Dismissal::Cancelled)
        );
        assert_eq!(
            classify(&AshpdError::Response(ResponseError::Other)),
            PortalFailure::Response(Dismissal::Other)
        );
        assert_eq!(
            classify(&AshpdError::Portal(PortalError::NotAllowed("no".into()))),
            dbus_named("org.freedesktop.portal.Error.NotAllowed", "no")
        );
        let fdo = ashpd::zbus::Error::FDO(Box::new(fdo::Error::ServiceUnknown("gone".into())));
        assert_eq!(
            classify(&AshpdError::Zbus(fdo)),
            dbus_named("org.freedesktop.DBus.Error.ServiceUnknown", "gone")
        );
        let io = ashpd::zbus::Error::InputOutput(Arc::new(std::io::Error::other("pipe")));
        assert!(matches!(
            classify(&AshpdError::Zbus(io)),
            PortalFailure::Other(_)
        ));
        assert_eq!(
            classify(&AshpdError::NoResponse),
            PortalFailure::NoResponse
        );
    }

    fn dbus_named(name: &str, message: &str) -> PortalFailure {
        PortalFailure::DbusError {
            name: name.to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn only_files_in_the_temporary_folder_are_removed() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let path = temporary.path().to_owned();
        assert!(path.starts_with(std::env::temp_dir()));
        remove_if_temporary(&path);
        assert!(!path.exists());
        // Not under the temporary folder: left alone.
        let outside = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        remove_if_temporary(&outside);
        assert!(outside.exists());
    }
}
