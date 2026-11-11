// SPDX-License-Identifier: GPL-3.0-or-later

//! What the session bus and the portals offer, for `kropa doctor`.
//!
//! Detection is by capability: which interfaces answer and with which version. The name of the
//! desktop is reported as a fact and decides nothing.

use std::path::PathBuf;

use ashpd::zbus::fdo::{DBusProxy, PropertiesProxy};
use ashpd::zbus::names::InterfaceName;
use ashpd::zbus::{self, Connection, DBusError};
use kropa_core::APP_ID;

use crate::{Capabilities, Capability, Diagnosis, Fact, FactValue, Status};

const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";
/// First version of the Screenshot interface that has the `AvailableTargets` property.
const TARGETS_VERSION: u32 = 3;

/// What a portal interface answered when asked for its `version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortalState {
    /// It answered: the interface is there, at this version.
    Version(u32),
    /// The frontend runs but does not implement the interface.
    Missing,
    /// Nobody answered: the frontend is not installed or cannot be started. Carries the detail.
    Unreachable(String),
}

/// Reads a D-Bus error name from the `version` request into a [`PortalState`].
pub(crate) fn state_of_error(name: &str, detail: &str) -> PortalState {
    match name {
        "org.freedesktop.DBus.Error.InvalidArgs"
        | "org.freedesktop.DBus.Error.UnknownInterface"
        | "org.freedesktop.DBus.Error.UnknownProperty"
        | "org.freedesktop.DBus.Error.UnknownObject" => PortalState::Missing,
        _ => PortalState::Unreachable(format!("{name}: {detail}")),
    }
}

async fn portal_state(properties: &PropertiesProxy<'_>, interface: &'static str) -> PortalState {
    let Ok(name) = InterfaceName::try_from(interface) else {
        return PortalState::Missing;
    };
    match properties.get(name, "version").await {
        Ok(value) => u32::try_from(&*value).map_or(PortalState::Missing, PortalState::Version),
        Err(zbus::fdo::Error::ZBus(zbus::Error::MethodError(name, detail, _))) => {
            state_of_error(&name, &detail.unwrap_or_default())
        }
        Err(e) => state_of_error(e.name().as_str(), e.description().unwrap_or_default()),
    }
}

fn state_fact(key: &'static str, state: &PortalState) -> Fact {
    Fact::new(
        key,
        match state {
            PortalState::Version(v) => FactValue::Number(*v),
            PortalState::Missing | PortalState::Unreachable(_) => FactValue::Absent,
        },
    )
}

/// Names of the targets in the bit set of the `AvailableTargets` property (screen, window, area,
/// active window), comma separated.
#[must_use]
pub fn targets_text(mask: u32) -> String {
    [(1, "screen"), (2, "window"), (4, "area"), (8, "active_window")]
        .iter()
        .filter(|(bit, _)| mask & bit != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether `<dir>/applications/<APP_ID>.desktop` exists in `XDG_DATA_HOME` or `XDG_DATA_DIRS`.
pub(crate) fn desktop_file_installed(data_home: Option<PathBuf>, data_dirs: &str) -> bool {
    let file = format!("applications/{APP_ID}.desktop");
    data_home
        .into_iter()
        .chain(data_dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .any(|dir| dir.join(&file).is_file())
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
}

fn env_text(name: &str) -> FactValue {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => FactValue::Text(v),
        _ => FactValue::Absent,
    }
}

fn env_present(name: &str) -> FactValue {
    FactValue::Bool(std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

fn capability(feature: &'static str, status: Status, reason: Option<&'static str>) -> Capability {
    Capability {
        feature,
        status,
        reason,
        fallback: None,
    }
}

/// `capture screen` and `capture output` from what the Screenshot portal answered.
/// `bus_ok` is whether the session bus could be reached at all.
pub(crate) fn capabilities(bus_ok: bool, screenshot: Option<&PortalState>) -> Capabilities {
    let screen = match (bus_ok, screenshot) {
        (false, _) | (true, None) => capability(
            "capture_screen",
            Status::Unavailable,
            Some("no_session_bus"),
        ),
        (true, Some(PortalState::Version(_))) => {
            capability("capture_screen", Status::Available, None)
        }
        (true, Some(_)) => capability(
            "capture_screen",
            Status::Unavailable,
            Some("portal_unavailable"),
        ),
    };
    let mut output = capability(
        "capture_output",
        Status::Unavailable,
        Some("output_not_isolable"),
    );
    if screen.status == Status::Available {
        output.fallback = Some("capture_screen_instead");
    }
    Capabilities { screen, output }
}

pub(crate) async fn diagnose(connection: Option<&Connection>) -> Diagnosis {
    let mut facts = vec![
        Fact::new("desktop", env_text("XDG_CURRENT_DESKTOP")),
        Fact::new("session_type", env_text("XDG_SESSION_TYPE")),
        Fact::new("wayland_display", env_present("WAYLAND_DISPLAY")),
        Fact::new("x11_display", env_present("DISPLAY")),
    ];
    let connected = match connection {
        Some(c) => Ok(c.clone()),
        None => Connection::session().await,
    };
    let portal_keys = [
        "portal_screenshot_targets",
        "portal_screencast",
        "portal_global_shortcuts",
        "portal_secret",
        "portal_registry",
        "kwin_service",
    ];
    let mut screenshot = None;
    let bus_ok = match &connected {
        Ok(connection) => {
            facts.push(Fact::new("session_bus", FactValue::Bool(true)));
            let (state, mut more) = probe_bus(connection).await;
            facts.push(state_fact("portal_screenshot", &state));
            facts.append(&mut more);
            screenshot = Some(state);
            true
        }
        Err(e) => {
            facts.push(Fact::new("session_bus", FactValue::Bool(false)));
            facts.push(Fact::new("session_bus_error", FactValue::Text(e.to_string())));
            facts.push(Fact::new("portal_screenshot", FactValue::NotProbed));
            for key in portal_keys {
                facts.push(Fact::new(key, FactValue::NotProbed));
            }
            false
        }
    };
    let installed = desktop_file_installed(
        data_home(),
        &std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned()),
    );
    facts.push(Fact::new("app_id", FactValue::Text(APP_ID.to_owned())));
    facts.push(Fact::new("desktop_file", FactValue::Bool(installed)));
    // Needs a Wayland connection to read the compositor's globals: not written yet.
    facts.push(Fact::new("layer_shell", FactValue::NotProbed));
    facts.push(Fact::new("data_control", FactValue::NotProbed));
    Diagnosis {
        platform: "linux",
        backend: "portal-screenshot",
        capabilities: capabilities(bus_ok, screenshot.as_ref()),
        facts,
    }
}

/// The Screenshot state, then the facts about the other portals, KWin and the targets.
async fn probe_bus(connection: &Connection) -> (PortalState, Vec<Fact>) {
    let mut facts = Vec::new();
    let properties = PropertiesProxy::builder(connection)
        .destination(PORTAL_NAME)
        .and_then(|b| b.path(PORTAL_PATH))
        .map(|b| b.build());
    let Ok(properties) = properties else {
        return (PortalState::Missing, facts);
    };
    let Ok(properties) = properties.await else {
        return (PortalState::Missing, facts);
    };
    let screenshot = portal_state(&properties, SCREENSHOT).await;
    let targets = match &screenshot {
        PortalState::Version(v) if *v >= TARGETS_VERSION => {
            let name = InterfaceName::from_static_str_unchecked(SCREENSHOT);
            match properties.get(name, "AvailableTargets").await {
                Ok(value) => u32::try_from(&*value)
                    .map_or(FactValue::Absent, |mask| FactValue::Text(targets_text(mask))),
                Err(_) => FactValue::Absent,
            }
        }
        _ => FactValue::Absent,
    };
    facts.push(Fact::new("portal_screenshot_targets", targets));
    for (key, interface) in [
        ("portal_screencast", "org.freedesktop.portal.ScreenCast"),
        (
            "portal_global_shortcuts",
            "org.freedesktop.portal.GlobalShortcuts",
        ),
        ("portal_secret", "org.freedesktop.portal.Secret"),
        ("portal_registry", "org.freedesktop.host.portal.Registry"),
    ] {
        facts.push(state_fact(key, &portal_state(&properties, interface).await));
    }
    // KWin owns `org.kde.KWin` whatever the policy: the fact only says it is there. Using its
    // `ScreenShot2` interface needs the `.desktop` declaration, which Kropa does not make yet.
    let kwin = match DBusProxy::new(connection).await {
        Ok(bus) => bus
            .name_has_owner("org.kde.KWin".try_into().unwrap_or_else(|_| unreachable!()))
            .await
            .map_or(FactValue::Absent, FactValue::Bool),
        Err(_) => FactValue::Absent,
    };
    facts.push(Fact::new("kwin_service", kwin));
    (screenshot, facts)
}

#[cfg(test)]
mod tests {
    use super::{PortalState, capabilities, desktop_file_installed, state_of_error, targets_text};
    use crate::Status;

    #[test]
    fn target_bits_have_names() {
        assert_eq!(targets_text(0), "");
        assert_eq!(targets_text(1 | 2 | 4), "screen,window,area");
        assert_eq!(targets_text(8), "active_window");
    }

    #[test]
    fn version_errors_are_read_by_name() {
        assert_eq!(
            state_of_error("org.freedesktop.DBus.Error.InvalidArgs", "x"),
            PortalState::Missing
        );
        assert!(matches!(
            state_of_error("org.freedesktop.DBus.Error.ServiceUnknown", "gone"),
            PortalState::Unreachable(d) if d.contains("gone")
        ));
    }

    #[test]
    fn screen_capability_follows_the_portal() {
        let ok = capabilities(true, Some(&PortalState::Version(2)));
        assert_eq!(ok.screen.status, Status::Available);
        assert_eq!(ok.output.status, Status::Unavailable);
        assert_eq!(ok.output.reason, Some("output_not_isolable"));
        assert_eq!(ok.output.fallback, Some("capture_screen_instead"));
        let missing = capabilities(true, Some(&PortalState::Missing));
        assert_eq!(missing.screen.reason, Some("portal_unavailable"));
        assert_eq!(missing.output.fallback, None);
        let nobus = capabilities(false, None);
        assert_eq!(nobus.screen.reason, Some("no_session_bus"));
    }

    #[test]
    fn desktop_file_is_looked_up_in_the_data_folders() {
        let home = tempfile::tempdir().unwrap();
        let sys = tempfile::tempdir().unwrap();
        let dirs = format!("/nonexistent::{}", sys.path().display());
        assert!(!desktop_file_installed(Some(home.path().to_owned()), &dirs));
        let file = format!("applications/{}.desktop", kropa_core::APP_ID);
        std::fs::create_dir_all(sys.path().join("applications")).unwrap();
        std::fs::write(sys.path().join(&file), "[Desktop Entry]\n").unwrap();
        assert!(desktop_file_installed(Some(home.path().to_owned()), &dirs));
        assert!(!desktop_file_installed(None, "/nonexistent"));
        std::fs::create_dir_all(home.path().join("applications")).unwrap();
        std::fs::write(home.path().join(&file), "[Desktop Entry]\n").unwrap();
        assert!(desktop_file_installed(Some(home.path().to_owned()), "/nonexistent"));
    }
}
