// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa doctor`: what this system lets Kropa do, and the fallbacks in force (`docs/cli.md`
//! section 2.8). The backend detects; this module only presents. No network request is made.
//!
//! It is a report: capabilities that are missing do not change the exit code, and a broken
//! configuration file is a finding, not a failure.

use std::path::{Path, PathBuf};

use kropa_capture::{CaptureBackend, Diagnosis, Fact, FactValue};
use kropa_core::{CONFIG_FILE, Config, ExitCode, config_dir};
use kropa_i18n::Localizer;
use serde_json::{Value, json};

use crate::exec::Context;
use crate::output::Session;

/// JSON schema of the report.
pub const SCHEMA: &str = "kropa.doctor/1";

/// What `doctor` cannot report yet, the lot that adds it, and its Fluent message.
const PENDING: [(&str, &str, &str); 3] = [
    ("kropa_app", "2.1", "doctor-pending-app"),
    ("clipboard", "1.10", "doctor-pending-clipboard"),
    ("outputs", "2.5", "doctor-pending-outputs"),
];

/// What is still missing from the report: the screens are no longer pending once the backend lists
/// them (macOS).
fn pending(
    diagnosis: &Diagnosis,
) -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    let lists_screens = diagnosis
        .facts
        .iter()
        .any(|fact| fact.key == "display" || fact.key == "display_primary");
    PENDING
        .into_iter()
        .filter(move |(item, _, _)| !(lists_screens && *item == "outputs"))
}

/// Where things are, and whether the configuration reads.
struct Paths {
    config_file: Option<PathBuf>,
    config_error: Option<String>,
    captures_dir: Option<PathBuf>,
}

fn paths(config_dir_arg: Option<&Path>) -> Paths {
    let dir = match config_dir_arg {
        Some(d) => Some(d.to_owned()),
        None => config_dir().ok(),
    };
    let config_file = dir.map(|d| d.join(CONFIG_FILE));
    let loaded = match &config_file {
        Some(file) => Config::load(file),
        None => Ok(Config::default()),
    };
    match loaded {
        Ok(config) => Paths {
            config_file,
            config_error: None,
            captures_dir: config.resolved_output_dir().ok(),
        },
        Err(e) => Paths {
            config_file,
            config_error: Some(e.to_string()),
            captures_dir: None,
        },
    }
}

/// Fluent id of a code: the prefix and the code with dashes.
fn fluent_id(prefix: &str, code: &str) -> String {
    format!("{prefix}-{}", code.replace('_', "-"))
}

fn fact_json(fact: &Fact) -> Value {
    let (state, value) = match &fact.value {
        FactValue::Text(t) => ("present", json!(t)),
        FactValue::Number(n) => ("present", json!(n)),
        FactValue::Bool(b) => ("present", json!(b)),
        FactValue::Absent => ("absent", Value::Null),
        FactValue::Unreachable => ("unreachable", Value::Null),
        FactValue::NotProbed => ("not_probed", Value::Null),
    };
    json!({ "key": fact.key, "state": state, "value": value })
}

fn document(diagnosis: &Diagnosis, paths: &Paths) -> Value {
    let capabilities: Vec<Value> = [
        &diagnosis.capabilities.screen,
        &diagnosis.capabilities.output,
    ]
    .iter()
    .map(|c| {
        json!({
            "feature": c.feature,
            "status": c.status.as_str(),
            "reason": c.reason,
            "fallback": c.fallback,
        })
    })
    .collect();
    let pending: Vec<Value> = pending(diagnosis)
        .map(|(item, lot, _)| json!({ "item": item, "lot": lot }))
        .collect();
    json!({
        "schema": SCHEMA,
        "status": "ok",
        "exit_code": 0,
        "system": {
            "platform": diagnosis.platform,
            "arch": std::env::consts::ARCH,
            "kropa_version": env!("CARGO_PKG_VERSION"),
        },
        "backend": diagnosis.backend,
        "capabilities": capabilities,
        "fallbacks": diagnosis.active_fallbacks(),
        "facts": diagnosis.facts.iter().map(fact_json).collect::<Vec<_>>(),
        "paths": {
            "config_file": paths.config_file.as_ref().map(|p| p.display().to_string()),
            "config_error": paths.config_error,
            "captures_dir": paths.captures_dir.as_ref().map(|p| p.display().to_string()),
        },
        "not_available_yet": pending,
    })
}

fn fact_value_text(value: &FactValue, loc: &Localizer) -> String {
    match value {
        FactValue::Text(t) => t.clone(),
        FactValue::Number(n) => n.to_string(),
        FactValue::Bool(true) => loc.message("doctor-value-yes"),
        FactValue::Bool(false) => loc.message("doctor-value-no"),
        FactValue::Absent => loc.message("doctor-value-absent"),
        FactValue::Unreachable => loc.message("doctor-value-unreachable"),
        FactValue::NotProbed => loc.message("doctor-value-not-probed"),
    }
}

/// One `label: value` line, indented.
fn labelled(loc: &Localizer, label: &str, value: &str) -> String {
    format!(
        "  {}",
        loc.message_args("doctor-fact", &[("label", label), ("value", value)])
    )
}

fn render(diagnosis: &Diagnosis, paths: &Paths, loc: &Localizer, session: &mut Session<'_, '_>) {
    let section = |session: &mut Session<'_, '_>, id: &str| {
        session.result("");
        session.result(&loc.message(id));
    };
    session.result(&loc.message("doctor-title"));
    section(session, "doctor-section-system");
    let system = loc.message_args(
        "doctor-system",
        &[
            ("platform", diagnosis.platform),
            ("arch", std::env::consts::ARCH),
            ("version", env!("CARGO_PKG_VERSION")),
        ],
    );
    session.result(&format!("  {system}"));

    section(session, "doctor-section-capabilities");
    for cap in [
        &diagnosis.capabilities.screen,
        &diagnosis.capabilities.output,
    ] {
        let feature = loc.message(&fluent_id("doctor-feature", cap.feature));
        let status = loc.message(&format!("doctor-status-{}", cap.status.as_str()));
        let head = loc.message_args(
            "doctor-capability",
            &[("feature", &feature), ("status", &status)],
        );
        session.result(&format!("  {head}"));
        if let Some(reason) = cap.reason {
            let text = loc.message(&fluent_id("doctor-reason", reason));
            let why = loc.message_args("doctor-why", &[("reason", &text)]);
            session.result(&format!("    {why}"));
        }
        if let Some(fallback) = cap.fallback {
            let text = loc.message(&fluent_id("doctor-fallback", fallback));
            let instead = loc.message_args("doctor-instead", &[("fallback", &text)]);
            session.result(&format!("    {instead}"));
        }
    }

    section(session, "doctor-section-details");
    for fact in &diagnosis.facts {
        let label = loc.message(&fluent_id("doctor-fact", fact.key));
        session.result(&labelled(loc, &label, &fact_value_text(&fact.value, loc)));
    }

    section(session, "doctor-section-paths");
    let shown = |p: Option<&PathBuf>| {
        p.map_or_else(
            || loc.message("doctor-value-absent"),
            |p| p.display().to_string(),
        )
    };
    let config = loc.message("doctor-path-config");
    session.result(&labelled(loc, &config, &shown(paths.config_file.as_ref())));
    if let Some(error) = &paths.config_error {
        let label = loc.message("doctor-path-config-error");
        session.result(&labelled(loc, &label, error));
    }
    let captures = loc.message("doctor-path-captures");
    session.result(&labelled(
        loc,
        &captures,
        &shown(paths.captures_dir.as_ref()),
    ));

    section(session, "doctor-section-pending");
    for (_, _, id) in pending(diagnosis) {
        session.result(&format!("  {}", loc.message(id)));
    }
}

/// Prints the report. Always succeeds: what is missing is part of the report.
pub fn run(
    backend: &dyn CaptureBackend,
    config_dir_arg: Option<&Path>,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> ExitCode {
    let diagnosis = backend.diagnose();
    let paths = paths(config_dir_arg);
    if session.json {
        session.document(&document(&diagnosis, &paths));
    } else {
        render(&diagnosis, &paths, ctx.loc, session);
    }
    ExitCode::Ok
}

#[cfg(test)]
mod tests {
    use kropa_capture::{
        ALL_FACT_KEYS, ALL_FALLBACKS, ALL_FEATURES, ALL_REASONS, Capabilities, Capability,
        Diagnosis, Fact, FactValue, Status,
    };
    use kropa_i18n::{Lang, Localizer};

    use super::{PENDING, Paths, document, fluent_id, pending, render};
    use crate::output::{Io, Session};

    fn diagnosis() -> Diagnosis {
        Diagnosis {
            platform: "linux",
            backend: "portal-screenshot",
            capabilities: Capabilities {
                screen: Capability {
                    feature: "capture_screen",
                    status: Status::Available,
                    reason: None,
                    fallback: None,
                },
                output: Capability {
                    feature: "capture_output",
                    status: Status::Unavailable,
                    reason: Some("output_not_isolable"),
                    fallback: Some("capture_screen_instead"),
                },
            },
            facts: vec![
                Fact::new("desktop", FactValue::Text("KDE".into())),
                Fact::new("portal_screenshot", FactValue::Number(2)),
                Fact::new("wayland_display", FactValue::Bool(true)),
                Fact::new("portal_secret", FactValue::Absent),
                Fact::new("kwin_service", FactValue::Unreachable),
                Fact::new("layer_shell", FactValue::NotProbed),
            ],
        }
    }

    fn paths() -> Paths {
        Paths {
            config_file: Some("/c/config.toml".into()),
            config_error: Some("bad".into()),
            captures_dir: None,
        }
    }

    #[test]
    fn json_keeps_the_state_of_each_fact_apart() {
        let doc = document(&diagnosis(), &paths());
        let states: Vec<_> = doc["facts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["state"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            states,
            [
                "present",
                "present",
                "present",
                "absent",
                "unreachable",
                "not_probed"
            ]
        );
        assert_eq!(doc["facts"][1]["value"], 2);
        assert_eq!(doc["fallbacks"][0], "capture_screen_instead");
        assert_eq!(doc["paths"]["config_error"], "bad");
        assert!(doc["paths"]["captures_dir"].is_null());
        assert_eq!(
            doc["not_available_yet"].as_array().unwrap().len(),
            PENDING.len()
        );
    }

    #[test]
    fn screens_are_not_pending_once_the_backend_lists_them() {
        let mut d = diagnosis();
        assert!(pending(&d).any(|(item, _, _)| item == "outputs"));
        d.facts
            .push(Fact::new("display", FactValue::Text("DELL".into())));
        assert!(pending(&d).all(|(item, _, _)| item != "outputs"));
        assert_eq!(pending(&d).count(), PENDING.len() - 1);
    }

    #[test]
    fn human_report_has_every_section_in_both_languages() {
        for (lang, title, available) in [
            (Lang::En, "Kropa diagnostic", "available"),
            (Lang::Fr, "Diagnostic de Kropa", "disponible"),
        ] {
            let loc = Localizer::new(lang).unwrap();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            {
                let mut io = Io {
                    out: &mut out,
                    err: &mut err,
                    stdin_is_tty: false,
                };
                let mut session = Session::new(&mut io, false, false, 0, false);
                render(&diagnosis(), &paths(), &loc, &mut session);
            }
            let text = String::from_utf8(out).unwrap();
            assert!(text.starts_with(title), "{text}");
            assert!(text.contains(available), "{text}");
            assert!(text.contains("KDE"), "{text}");
            assert!(text.contains("/c/config.toml"), "{text}");
            assert!(text.contains("bad"), "{text}");
            assert_eq!(err.len(), 0);
            // A message id on screen would mean a missing translation.
            assert!(!text.contains("doctor-"), "{text}");
        }
    }

    #[test]
    fn every_code_the_backend_can_emit_has_a_message_in_both_languages() {
        let ids: Vec<String> = ALL_FACT_KEYS
            .iter()
            .map(|k| fluent_id("doctor-fact", k))
            .chain(ALL_FEATURES.iter().map(|k| fluent_id("doctor-feature", k)))
            .chain(ALL_REASONS.iter().map(|k| fluent_id("doctor-reason", k)))
            .chain(
                ALL_FALLBACKS
                    .iter()
                    .map(|k| fluent_id("doctor-fallback", k)),
            )
            .chain(
                ["available", "degraded", "unavailable"]
                    .iter()
                    .map(|k| format!("doctor-status-{k}")),
            )
            .chain(PENDING.iter().map(|(_, _, id)| (*id).to_owned()))
            .collect();
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for id in &ids {
                assert_ne!(&loc.message(id), id, "{lang:?}: {id}");
            }
        }
    }
}
