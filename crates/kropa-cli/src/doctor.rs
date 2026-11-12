// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa doctor`: what this system lets Kropa do, and the fallbacks in force (`docs/cli.md`
//! section 2.8). The backend detects; this module only presents. No network request is made.
//!
//! It is a report: capabilities that are missing do not change the exit code, and a broken
//! configuration file is a finding, not a failure.

use std::path::{Path, PathBuf};

use kropa_capture::{CaptureBackend, Diagnosis, Fact, FactValue, ResetError};
use kropa_core::{CONFIG_FILE, Config, ExitCode, config_dir};
use kropa_i18n::Localizer;
use kropa_ipc::Probe;
use kropa_upload::profiles::load_destinations;
use kropa_upload::secret::{StoreState, StoreStatus};
use serde_json::{Value, json};

use crate::args::DoctorArgs;
use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;

/// JSON schema of the report.
pub const SCHEMA: &str = "kropa.doctor/1";

/// JSON schema of `doctor --reset-screenshot-permission`.
pub const RESET_SCHEMA: &str = "kropa.doctor.reset/1";

/// What `doctor` cannot report yet, the lot that adds it, and its Fluent message.
const PENDING: [(&str, &str, &str); 1] = [("outputs", "2.5", "doctor-pending-outputs")];

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

/// The secret store as the report shows it: its state and the profiles it holds, by name and count.
/// Never a value, never the name of a secret.
struct Secrets {
    status: StoreStatus,
    /// Profiles of the store that no destination of the configuration declares (N2: leftovers of a
    /// removed profile, which would block the next import of the same identifier). `None` when the
    /// destinations file could not be read, so that nothing can be said.
    orphans: Option<Vec<String>>,
}

impl Secrets {
    fn duplicates(&self) -> bool {
        self.status.state == StoreState::Ambiguous
    }
}

/// Reads the state of the store (it never prompts) and compares its profiles with the destinations.
fn secrets(config_dir_arg: Option<&Path>, ctx: &Context<'_>) -> Secrets {
    secrets_of(ctx.secrets.status(), config_dir_arg)
}

fn secrets_of(status: StoreStatus, config_dir_arg: Option<&Path>) -> Secrets {
    let dir = match config_dir_arg {
        Some(d) => Some(d.to_owned()),
        None => config_dir().ok(),
    };
    let declared = dir.and_then(|d| load_destinations(&d).ok());
    let orphans = declared.map(|destinations| {
        status
            .profiles
            .keys()
            .filter(|profile| !destinations.iter().any(|d| &d.id == *profile))
            .cloned()
            .collect()
    });
    Secrets { status, orphans }
}

fn secrets_json(secrets: &Secrets) -> Value {
    let profiles: Vec<Value> = secrets
        .status
        .profiles
        .iter()
        .map(|(profile, count)| json!({"profile": profile, "entries": count.entries}))
        .collect();
    json!({
        "store": secrets.status.store,
        "state": secrets.status.state.as_str(),
        "detail": secrets.status.detail,
        "profiles": profiles,
        "duplicates": secrets.duplicates(),
        "orphans": secrets.orphans,
    })
}

/// The graphical application as the socket shows it, and the D-Bus service file that starts it.
struct Ipc {
    probe: Probe,
    /// `None` where no service file is used (macOS).
    service_file: Option<bool>,
}

impl Ipc {
    /// Looks at the socket (nothing is launched) and at the service directories of the bus.
    fn gather() -> Self {
        Self {
            probe: kropa_ipc::probe(),
            service_file: kropa_ipc::service_file_present(&|name| std::env::var(name).ok()),
        }
    }
}

fn ipc_json(ipc: &Ipc) -> Value {
    let server = ipc.probe.server.as_ref().map(|s| {
        json!({"name": s.server, "version": s.version, "protocol": s.protocol, "features": s.features})
    });
    json!({
        "socket": ipc.probe.socket.as_ref().map(|p| p.display().to_string()),
        "state": ipc.probe.state.as_str(),
        "reason": ipc.probe.reason,
        "protocol": kropa_ipc::PROTOCOL,
        "server": server,
        "service_file": ipc.service_file,
    })
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

fn document(diagnosis: &Diagnosis, paths: &Paths, secrets: &Secrets, ipc: &Ipc) -> Value {
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
        "secrets": secrets_json(secrets),
        "ipc": ipc_json(ipc),
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

fn render(
    diagnosis: &Diagnosis,
    paths: &Paths,
    secrets: &Secrets,
    ipc: &Ipc,
    loc: &Localizer,
    session: &mut Session<'_, '_>,
) {
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

    section(session, "doctor-section-secrets");
    let state = loc.message(&format!(
        "out-secrets-state-{}",
        secrets.status.state.as_str()
    ));
    let head = loc.message_args(
        "doctor-secrets-store",
        &[("store", secrets.status.store), ("state", &state)],
    );
    session.result(&format!("  {head}"));
    if let Some(detail) = &secrets.status.detail {
        // The detail comes from the store backend and stays in English: say so.
        let line = loc.message_args("doctor-secrets-detail", &[("detail", detail)]);
        session.result(&format!("    {line}"));
    }
    let count = secrets.status.profiles.len().to_string();
    let profiles = loc.message_args("doctor-secrets-profiles", &[("count", &count)]);
    session.result(&format!("  {profiles}"));
    if secrets.duplicates() {
        session.result(&format!("  {}", loc.message("doctor-secrets-duplicates")));
    }
    match &secrets.orphans {
        Some(orphans) if !orphans.is_empty() => {
            let names = orphans.join(", ");
            let text = loc.message_args("doctor-secrets-orphans", &[("names", &names)]);
            session.result(&format!("  {text}"));
        }
        Some(_) => {}
        None => session.result(&format!(
            "  {}",
            loc.message("doctor-secrets-orphans-unknown")
        )),
    }

    render_ipc(ipc, loc, session);

    section(session, "doctor-section-pending");
    for (_, _, id) in pending(diagnosis) {
        session.result(&format!("  {}", loc.message(id)));
    }
}

fn render_ipc(ipc: &Ipc, loc: &Localizer, session: &mut Session<'_, '_>) {
    session.result("");
    session.result(&loc.message("doctor-section-ipc"));
    let probe = &ipc.probe;
    if let Some(socket) = &probe.socket {
        let path = socket.display().to_string();
        session.result(&format!(
            "  {}",
            loc.message_args("doctor-ipc-socket", &[("path", &path)])
        ));
    }
    let state = loc.message(&format!("doctor-ipc-state-{}", probe.state.as_str()));
    session.result(&format!(
        "  {}",
        loc.message_args("doctor-ipc-state", &[("state", &state)])
    ));
    if let Some(reason) = &probe.reason {
        let text = loc.message(&fluent_id("doctor-ipc-reason", reason));
        session.result(&format!(
            "    {}",
            loc.message_args("doctor-ipc-reason", &[("reason", &text)])
        ));
    }
    if let Some(server) = &probe.server {
        let protocol = server.protocol.to_string();
        let line = loc.message_args(
            "doctor-ipc-server",
            &[
                ("server", &server.server),
                ("version", &server.version),
                ("protocol", &protocol),
            ],
        );
        session.result(&format!("  {line}"));
    }
    match ipc.service_file {
        Some(present) => {
            let value = loc.message(if present {
                "doctor-value-yes"
            } else {
                "doctor-value-no"
            });
            session.result(&format!(
                "  {}",
                loc.message_args("doctor-ipc-service-file", &[("value", &value)])
            ));
        }
        None => session.result(&format!(
            "  {}",
            loc.message("doctor-ipc-service-file-unused")
        )),
    }
}

/// `kropa doctor`: the report, or with `--reset-screenshot-permission` the reset of the stored
/// permission.
///
/// # Errors
/// Only the reset can fail, see [`reset_screenshot_permission`].
pub fn command(
    args: &DoctorArgs,
    backend: &dyn CaptureBackend,
    config_dir_arg: Option<&Path>,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    if args.reset_screenshot_permission {
        return reset_screenshot_permission(backend, ctx, session);
    }
    // A report: a broken configuration is shown in it, not a reason to fail.
    Ok(run(backend, config_dir_arg, ctx, session))
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
    let secrets = secrets(config_dir_arg, ctx);
    let ipc = Ipc::gather();
    if session.json {
        session.document(&document(&diagnosis, &paths, &secrets, &ipc));
    } else {
        render(&diagnosis, &paths, &secrets, &ipc, ctx.loc, session);
    }
    ExitCode::Ok
}

/// The error of a reset that could not be done, with its Fluent message and its machine reason.
fn reset_error(error: &ResetError, loc: &Localizer) -> CliError {
    let id = match error {
        ResetError::Unsupported => "error-reset-unsupported",
        ResetError::StoreUnreachable(_) => "error-reset-store-unreachable",
        // `StoreError`, and any case added later: the generic message carries the detail.
        _ => "error-reset-store-error",
    };
    let detail = error.detail().unwrap_or_default();
    CliError::new(error.exit_code(), loc, id, &[("detail", detail)])
        .with_reason(error.reason())
        .at_step("reset")
}

/// `doctor --reset-screenshot-permission`: makes the system forget the stored answer to the
/// screenshot request, and says what really happened (entry removed, none stored, or why nothing
/// could be done). Nothing is captured and no dialog is shown; the next capture asks again.
///
/// # Errors
/// A [`CliError`]: code 5 when the platform keeps the permission out of reach (macOS) or the
/// permission store cannot be reached, code 1 for an answer nobody planned for.
fn reset_screenshot_permission(
    backend: &dyn CaptureBackend,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let outcome = backend
        .reset_permission()
        .map_err(|e| reset_error(&e, ctx.loc))?;
    if session.json {
        session.document(&json!({
            "schema": RESET_SCHEMA,
            "status": "ok",
            "exit_code": 0,
            "permission": "screenshot",
            "result": outcome.as_str(),
        }));
    } else {
        session.result(
            &ctx.loc
                .message(&format!("doctor-reset-{}", outcome.as_str())),
        );
    }
    Ok(ExitCode::Ok)
}

#[cfg(test)]
mod tests {
    use kropa_capture::{
        ALL_FACT_KEYS, ALL_FALLBACKS, ALL_FEATURES, ALL_REASONS, Capabilities, Capability,
        Diagnosis, Fact, FactValue, Status,
    };
    use kropa_i18n::{Lang, Localizer};

    use std::collections::BTreeMap;

    use kropa_upload::secret::{ProfileCount, StoreState, StoreStatus};

    use kropa_capture::ResetError;

    use super::{
        Ipc, PENDING, Paths, Secrets, document, fluent_id, pending, render, reset_error, secrets_of,
    };
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

    fn secrets(state: StoreState, orphans: Option<&[&str]>) -> Secrets {
        let count = ProfileCount {
            entries: 2,
            empty: 0,
        };
        Secrets {
            status: StoreStatus {
                store: "keyring",
                state,
                detail: None,
                profiles: BTreeMap::from([
                    ("zipline".to_owned(), count),
                    ("old".to_owned(), count),
                ]),
            },
            orphans: orphans.map(|o| o.iter().map(|n| (*n).to_owned()).collect()),
        }
    }

    fn ipc() -> Ipc {
        Ipc {
            probe: kropa_ipc::Probe {
                socket: Some("/run/user/1000/kropa/app.sock".into()),
                state: kropa_ipc::ProbeState::Listening,
                reason: None,
                server: Some(kropa_ipc::HelloReply {
                    protocol: 1,
                    server: "kropa-gtk".into(),
                    version: "9.9.9".into(),
                    methods: vec![],
                    features: vec![],
                }),
            },
            service_file: Some(false),
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
        let doc = document(
            &diagnosis(),
            &paths(),
            &secrets(StoreState::Ambiguous, Some(&["old"])),
            &ipc(),
        );
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
        assert_eq!(doc["secrets"]["state"], "ambiguous");
        assert_eq!(doc["secrets"]["duplicates"], true);
        assert_eq!(doc["secrets"]["orphans"][0], "old");
        assert_eq!(doc["secrets"]["profiles"][0]["entries"], 2);
        assert_eq!(doc["ipc"]["state"], "listening");
        assert_eq!(doc["ipc"]["server"]["name"], "kropa-gtk");
        assert_eq!(doc["ipc"]["protocol"], 1);
        assert_eq!(doc["ipc"]["service_file"], false);
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
                render(
                    &diagnosis(),
                    &paths(),
                    &secrets(StoreState::Locked, Some(&["old"])),
                    &ipc(),
                    &loc,
                    &mut session,
                );
            }
            let text = String::from_utf8(out).unwrap();
            assert!(text.starts_with(title), "{text}");
            assert!(text.contains(available), "{text}");
            assert!(text.contains("KDE"), "{text}");
            assert!(text.contains("/c/config.toml"), "{text}");
            assert!(text.contains("bad"), "{text}");
            assert!(text.contains("keyring"), "{text}");
            assert!(text.contains("old"), "{text}");
            assert!(text.contains("kropa-gtk 9.9.9"), "{text}");
            assert!(text.contains("app.sock"), "{text}");
            assert_eq!(err.len(), 0);
            // A message id on screen would mean a missing translation.
            assert!(!text.contains("doctor-"), "{text}");
        }
    }

    #[test]
    fn orphans_are_store_profiles_no_destination_declares() {
        let dir = tempfile::tempdir().unwrap();
        let status = || secrets(StoreState::Available, None).status;
        // No destinations file: every profile of the store is an orphan.
        let none = secrets_of(status(), Some(dir.path()));
        assert_eq!(none.orphans.unwrap(), ["old", "zipline"]);
        std::fs::write(
            dir.path().join("destinations.toml"),
            "[[destination]]\nid = \"zipline\"\ntype = \"zipline\"\nurl = \"https://z.example.org/\"\n[destination.secrets]\ntoken = {}\n",
        )
        .unwrap();
        let some = secrets_of(status(), Some(dir.path()));
        assert_eq!(some.orphans.unwrap(), ["old"]);
        // A destinations file that does not parse: nothing is said, and it is said so.
        std::fs::write(dir.path().join("destinations.toml"), "not [ toml").unwrap();
        assert!(secrets_of(status(), Some(dir.path())).orphans.is_none());
    }

    #[test]
    fn the_secret_block_says_duplicates_and_unknown_orphans_and_nothing_else() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            let report = |secrets: &Secrets| {
                let (mut out, mut err) = (Vec::new(), Vec::new());
                {
                    let mut io = Io {
                        out: &mut out,
                        err: &mut err,
                        stdin_is_tty: false,
                    };
                    let mut session = Session::new(&mut io, false, false, 0, false);
                    render(&diagnosis(), &paths(), secrets, &ipc(), &loc, &mut session);
                }
                String::from_utf8(out).unwrap()
            };
            let clean = report(&secrets(StoreState::Available, Some(&[])));
            assert!(!clean.contains("doctor-"), "{clean}");
            assert!(!clean.contains(&loc.message("doctor-secrets-duplicates")));
            assert!(
                !clean.contains("old,") && !clean.contains(": old"),
                "{clean}"
            );
            let dup = report(&secrets(StoreState::Ambiguous, None));
            assert!(
                dup.contains(&loc.message("doctor-secrets-duplicates")),
                "{dup}"
            );
            assert!(
                dup.contains(&loc.message("doctor-secrets-orphans-unknown")),
                "{dup}"
            );
        }
    }

    #[test]
    fn a_failed_reset_has_its_code_reason_and_text_in_both_languages() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for (error, code, reason, mentions) in [
                (
                    ResetError::Unsupported,
                    5,
                    "permission_reset_unsupported",
                    None,
                ),
                (
                    ResetError::StoreUnreachable("no owner".into()),
                    5,
                    "permission_store_unreachable",
                    Some("no owner"),
                ),
                (
                    ResetError::StoreError("boom".into()),
                    1,
                    "permission_store_error",
                    Some("boom"),
                ),
            ] {
                let e = reset_error(&error, &loc);
                assert_eq!(e.code.code(), code);
                assert_eq!(e.reason.as_deref(), Some(reason));
                assert_eq!(e.step, Some("reset"));
                assert!(!e.message.starts_with("error-reset"), "{}", e.message);
                if let Some(detail) = mentions {
                    assert!(e.message.contains(detail), "{}", e.message);
                }
            }
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
            .chain(
                ["absent", "listening", "orphan", "unsafe", "incompatible"]
                    .iter()
                    .map(|k| format!("doctor-ipc-state-{k}")),
            )
            .chain(
                [
                    "no_runtime_dir",
                    "unsafe_runtime_dir",
                    "peer_uid_mismatch",
                    "ipc_version_mismatch",
                    "ipc_protocol_error",
                ]
                .iter()
                .map(|k| fluent_id("doctor-ipc-reason", k)),
            )
            .chain(
                [
                    "doctor-section-secrets",
                    "doctor-secrets-duplicates",
                    "doctor-secrets-orphans-unknown",
                    "doctor-section-ipc",
                    "doctor-ipc-service-file-unused",
                    "doctor-reset-removed",
                    "doctor-reset-absent",
                    "error-reset-unsupported",
                ]
                .map(str::to_owned),
            )
            .collect();
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for id in &ids {
                assert_ne!(&loc.message(id), id, "{lang:?}: {id}");
            }
        }
    }
}
