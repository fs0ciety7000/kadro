// SPDX-License-Identifier: GPL-3.0-or-later

//! What a backend found out about the system: the data behind `kropa doctor`.
//!
//! Everything is a stable code, never prose: the command line turns the codes into Fluent text, so
//! the report exists in French and English and the JSON stays the same in both.

/// Whether a feature works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// It works as asked.
    Available,
    /// It works through a fallback or with a restriction (see the fallback).
    Degraded,
    /// It does not work here.
    Unavailable,
}

impl Status {
    /// Name used in JSON output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One feature, its status, why, and what to do instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// Feature code (see [`ALL_FEATURES`]).
    pub feature: &'static str,
    /// Whether it works.
    pub status: Status,
    /// Why it does not fully work (see [`ALL_REASONS`]); `None` when it does.
    pub reason: Option<&'static str>,
    /// The fallback to use instead (see [`ALL_FALLBACKS`]).
    pub fallback: Option<&'static str>,
}

/// The features of the commands of this lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// `capture screen`.
    pub screen: Capability,
    /// `capture output`.
    pub output: Capability,
}

/// The value of a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactValue {
    /// A text (desktop name, error detail).
    Text(String),
    /// A number (interface version).
    Number(u32),
    /// A yes/no.
    Bool(bool),
    /// Looked for and not found.
    Absent,
    /// Not looked for: the probe is not written yet.
    NotProbed,
}

/// One observation about the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// Fact code (see [`ALL_FACT_KEYS`]).
    pub key: &'static str,
    /// What was found.
    pub value: FactValue,
}

impl Fact {
    /// A fact.
    #[must_use]
    pub fn new(key: &'static str, value: FactValue) -> Self {
        Self { key, value }
    }
}

/// The result of [`crate::CaptureBackend::diagnose`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// Platform (`linux`, `macos`).
    pub platform: &'static str,
    /// Backend in use (`portal-screenshot`).
    pub backend: &'static str,
    /// What `capture` can do.
    pub capabilities: Capabilities,
    /// Observations, in display order.
    pub facts: Vec<Fact>,
}

impl Diagnosis {
    /// Fallbacks currently in force, in display order, without duplicates.
    #[must_use]
    pub fn active_fallbacks(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for cap in [&self.capabilities.screen, &self.capabilities.output] {
            if let Some(f) = cap.fallback
                && !out.contains(&f)
            {
                out.push(f);
            }
        }
        out
    }
}

/// Feature codes of [`Capability::feature`].
pub const ALL_FEATURES: [&str; 2] = ["capture_screen", "capture_output"];

/// Reason codes of [`Capability::reason`].
pub const ALL_REASONS: [&str; 3] = [
    "no_session_bus",
    "portal_unavailable",
    "output_not_isolable",
];

/// Fallback codes of [`Capability::fallback`].
pub const ALL_FALLBACKS: [&str; 1] = ["capture_screen_instead"];

/// Fact codes the backends emit.
pub const ALL_FACT_KEYS: [&str; 17] = [
    "desktop",
    "session_type",
    "wayland_display",
    "x11_display",
    "session_bus",
    "session_bus_error",
    "portal_screenshot",
    "portal_screenshot_targets",
    "portal_screencast",
    "portal_global_shortcuts",
    "portal_secret",
    "portal_registry",
    "kwin_service",
    "app_id",
    "desktop_file",
    "layer_shell",
    "data_control",
];

#[cfg(test)]
mod tests {
    use super::{Capabilities, Capability, Diagnosis, Status};

    fn cap(feature: &'static str, fallback: Option<&'static str>) -> Capability {
        Capability {
            feature,
            status: Status::Degraded,
            reason: None,
            fallback,
        }
    }

    #[test]
    fn active_fallbacks_are_deduplicated() {
        let d = Diagnosis {
            platform: "linux",
            backend: "portal-screenshot",
            capabilities: Capabilities {
                screen: cap("capture_screen", Some("a")),
                output: cap("capture_output", Some("a")),
            },
            facts: Vec::new(),
        };
        assert_eq!(d.active_fallbacks(), ["a"]);
    }

    #[test]
    fn status_names() {
        assert_eq!(Status::Available.as_str(), "available");
        assert_eq!(Status::Degraded.as_str(), "degraded");
        assert_eq!(Status::Unavailable.as_str(), "unavailable");
    }
}
