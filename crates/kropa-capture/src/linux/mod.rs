// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux backends. Today: the `org.freedesktop.portal.Screenshot` portal (GNOME, KDE, wlroots with
//! `xdg-desktop-portal-wlr`), which asks the compositor for the image and hands back a file.
//!
//! The async D-Bus code (zbus, through ashpd) stays in this module: the rest of Kropa calls the
//! blocking [`crate::CaptureBackend`] interface and never sees a future. The runtime is the one
//! zbus brings (`async-io`); there is no tokio in the tree.

mod permission;
mod portal;
mod probe;
mod uri;

pub use portal::PortalScreenshot;
pub use probe::targets_text;
pub use uri::{UriError, file_uri_to_path};
