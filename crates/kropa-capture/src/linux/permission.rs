// SPDX-License-Identifier: GPL-3.0-or-later

//! Deletes Kropa's entry from the portals' permission store (`org.freedesktop.impl.portal.PermissionStore`).
//!
//! The store is the session service that remembers, per application, what the user answered to a
//! portal dialog. It is what `flatpak permission-reset` edits; outside Flatpak nothing else offers
//! the operation. The call is `DeletePermission(table, id, app)` with table and id `screenshot`.

use ashpd::zbus::{self, Connection};
use kropa_core::APP_ID;

use super::portal::{PortalFailure, classify_zbus, is_service_missing};
use crate::{ResetError, ResetOutcome};

const DESTINATION: &str = "org.freedesktop.impl.portal.PermissionStore";
const PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const TABLE: &str = "screenshot";
const NOT_FOUND: &str = "org.freedesktop.portal.Error.NotFound";

/// Asks the store to forget the screenshot answer given to [`APP_ID`].
pub(super) async fn reset_screenshot(
    connection: Option<&Connection>,
) -> Result<ResetOutcome, ResetError> {
    let connection = match connection {
        Some(c) => c.clone(),
        None => Connection::session()
            .await
            .map_err(|e| ResetError::StoreUnreachable(format!("no session bus: {e}")))?,
    };
    let reply = connection
        .call_method(
            Some(DESTINATION),
            PATH,
            Some(DESTINATION),
            "DeletePermission",
            &(TABLE, TABLE, APP_ID),
        )
        .await;
    match reply {
        Ok(_) => Ok(ResetOutcome::Removed),
        Err(error) => interpret(&error),
    }
}

/// An answer "no such entry" is an outcome, not a failure; anything else is classified.
fn interpret(error: &zbus::Error) -> Result<ResetOutcome, ResetError> {
    match classify_zbus(error) {
        PortalFailure::DbusError { name, .. } if name == NOT_FOUND => Ok(ResetOutcome::Absent),
        PortalFailure::DbusError { name, message } => {
            let detail = if message.is_empty() {
                name.clone()
            } else {
                format!("{name}: {message}")
            };
            if is_service_missing(&name) || is_unreachable(&name) {
                Err(ResetError::StoreUnreachable(detail))
            } else {
                Err(ResetError::StoreError(detail))
            }
        }
        // The connection itself failed (socket closed, I/O): nobody to talk to.
        PortalFailure::Other(text) => Err(ResetError::StoreUnreachable(text)),
        other => Err(ResetError::StoreError(format!("{other:?}"))),
    }
}

/// Errors of the bus that mean the store could not be reached or may not be talked to (a sandbox
/// without access to the store answers `AccessDenied`).
fn is_unreachable(name: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.AccessDenied"
            | "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout"
            | "org.freedesktop.DBus.Error.TimedOut"
    )
}
