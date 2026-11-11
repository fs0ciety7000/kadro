// SPDX-License-Identifier: GPL-3.0-or-later

//! Execution of a parsed command line.
//!
//! What does not depend on another lot is done for real: `version`, `doctor` and `capture
//! screen|output` where a capture backend exists (Linux), argument checks (secrets never in
//! arguments), configuration loading, input files, name resolution and the refusal to prompt without
//! a terminal. A command whose lot is not delivered ends with `unsupported` (5) and says which lot
//! brings it; nothing is simulated.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kropa_capture::{CaptureBackend, CaptureMode};
use kropa_core::{CONFIG_FILE, Clock, Config, ExitCode, config_dir};
use kropa_i18n::Localizer;
use serde_json::json;

use crate::args::{
    CaptureCommand, Cli, Command, CopyTarget, DestCommand, HistoryCommand, QueueCommand,
    SecretsCommand,
};
use crate::capture::{self, Job};
use crate::doctor;
use crate::errors::CliError;
use crate::output::Session;
use crate::secret_args::{set_carries_secret, url_carries_secret, var_carries_secret};
use crate::target::{Target, TargetSpec, resolve_target};

/// Version of the JSON schema of the errors that no command schema covers (usage errors).
pub const ERROR_SCHEMA: &str = "kropa.error/1";

/// JSON schemas this binary speaks (`docs/cli.md` section 4).
pub const SCHEMAS: [&str; 9] = [
    "kropa.capture/1",
    "kropa.upload/1",
    "kropa.dest.list/1",
    "kropa.queue.list/1",
    "kropa.history.list/1",
    "kropa.doctor/1",
    "kropa.secrets.status/1",
    "kropa.version/1",
    "kropa.error/1",
];

const COMMIT: Option<&str> = option_env!("KROPA_BUILD_COMMIT");
const TARGET: &str = env!("KROPA_BUILD_TARGET");

/// What the execution needs from the outside, injectable for tests.
pub struct Context<'a> {
    /// Language in use.
    pub loc: &'a Localizer,
    /// Time source for name templates.
    pub clock: &'a dyn Clock,
    /// Host name for `{host}`.
    pub host: Option<String>,
    /// How this system takes screenshots; `None` on a platform without one.
    pub backend: Option<&'a dyn CaptureBackend>,
    /// Waits for `--delay`; replaced by a recorder in tests.
    pub sleep: &'a dyn Fn(Duration),
}

/// Command path used for the schema name: `capture`, `dest.list`, `secrets.status`...
fn path(cmd: &Command) -> Vec<&'static str> {
    match cmd {
        Command::Capture { .. } => vec!["capture"],
        Command::Upload(_) => vec!["upload"],
        Command::Dest { action } => vec![
            "dest",
            match action {
                DestCommand::List => "list",
                DestCommand::Show { .. } => "show",
                DestCommand::Add(_) => "add",
                DestCommand::Edit { .. } => "edit",
                DestCommand::Test { .. } => "test",
                DestCommand::Remove { .. } => "remove",
                DestCommand::SetDefault { .. } => "set-default",
                DestCommand::Import { .. } => "import",
                DestCommand::Export { .. } => "export",
            },
        ],
        Command::Secrets { action } => vec![
            "secrets",
            match action {
                SecretsCommand::Status => "status",
                SecretsCommand::Set { .. } => "set",
                SecretsCommand::Remove { .. } => "remove",
                SecretsCommand::Unlock => "unlock",
                SecretsCommand::Lock => "lock",
            },
        ],
        Command::Queue { action } => vec![
            "queue",
            match action {
                QueueCommand::List { .. } => "list",
                QueueCommand::Retry { .. } => "retry",
                QueueCommand::Cancel { .. } => "cancel",
                QueueCommand::Clear { .. } => "clear",
            },
        ],
        Command::History { action } => match action {
            HistoryCommand::List { .. } => vec!["history", "list"],
        },
        Command::Edit(_) => vec!["edit"],
        Command::Doctor => vec!["doctor"],
        Command::Version(_) => vec!["version"],
    }
}

/// JSON schema of a command's output, and of its error envelope: the documented schema of the
/// command (`SCHEMAS`) when it has one, `kropa.error/1` for the others, so that a script never meets
/// a schema name that is not listed by `kropa version --json`.
#[must_use]
pub fn schema_of(cmd: &Command) -> String {
    let own = format!("kropa.{}/1", path(cmd).join("."));
    if SCHEMAS.contains(&own.as_str()) {
        own
    } else {
        ERROR_SCHEMA.to_owned()
    }
}

/// The lot that brings the capture backend of this platform, `None` when it is delivered.
fn platform_capture_lot() -> Option<&'static str> {
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        None
    } else {
        Some("1.5")
    }
}

/// Lots that must still be delivered for the command to work, per `docs/plan.md`.
fn lots_of(cmd: &Command) -> Vec<&'static str> {
    match cmd {
        Command::Capture { mode } => {
            let (base, opts) = match mode {
                CaptureCommand::Screen(o) | CaptureCommand::Output { opts: o, .. } => {
                    (platform_capture_lot(), o)
                }
                CaptureCommand::Area { opts, .. } => (Some("2.5"), opts),
            };
            let mut lots: Vec<&'static str> = base.into_iter().collect();
            if opts.send.upload.is_some() {
                lots.push("1.7");
            }
            if opts.edit {
                lots.push("3.7");
            }
            if opts.send.copy == Some(CopyTarget::Image) {
                lots.push("1.10");
            }
            lots
        }
        Command::Upload(_) => vec!["1.7"],
        Command::Dest { action } => match action {
            DestCommand::List
            | DestCommand::Show { .. }
            | DestCommand::Test { .. }
            | DestCommand::Import { .. } => vec!["1.7"],
            _ => vec!["2.6"],
        },
        Command::Secrets { action } => match action {
            SecretsCommand::Status | SecretsCommand::Set { .. } | SecretsCommand::Remove { .. } => {
                vec!["1.8"]
            }
            SecretsCommand::Unlock | SecretsCommand::Lock => vec!["2.8"],
        },
        Command::Queue { .. } => vec!["4.5"],
        Command::History { .. } => vec!["4.6"],
        Command::Edit(_) => vec!["3.7"],
        Command::Doctor => platform_capture_lot().into_iter().collect(),
        Command::Version(_) => vec!["5"],
    }
}

fn unsupported(cmd: &Command, loc: &Localizer) -> CliError {
    let mut lots = lots_of(cmd);
    lots.sort_unstable();
    let id = if lots.len() == 1 {
        "error-unsupported-one"
    } else {
        "error-unsupported-many"
    };
    let mut err = CliError::new(
        ExitCode::Unsupported,
        loc,
        id,
        &[("lots", &lots.join(", "))],
    );
    err.lots = lots;
    err
}

/// Runs the command and reports on `session`. Returns the exit code.
pub fn execute(cli: &Cli, ctx: &Context<'_>, session: &mut Session<'_, '_>) -> ExitCode {
    let schema = schema_of(&cli.command);
    match run_command(cli, ctx, session) {
        Ok(code) => code,
        Err(error) => session.fail(&schema, &error),
    }
}

fn run_command(
    cli: &Cli,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    if let Command::Version(v) = &cli.command {
        // `version` needs no configuration (the language alone is read from it, leniently, before).
        if v.check {
            return Err(unsupported(&cli.command, loc));
        }
        print_version(ctx, session);
        return Ok(ExitCode::Ok);
    }
    let config_dir_arg = cli.global.config.as_deref();
    if let (Command::Doctor, Some(backend)) = (&cli.command, ctx.backend) {
        // A report: a broken configuration is shown in it, not a reason to fail.
        return Ok(doctor::run(backend, config_dir_arg, ctx, session));
    }
    refuse_secrets_in_arguments(&cli.command, loc)?;
    let config = Config::load_from(config_dir_arg).map_err(|e| CliError::from_core(&e, loc))?;
    if let Some(file) = config_file(config_dir_arg) {
        session.debug(
            1,
            &loc.message_args("note-config", &[("path", &file.display().to_string())]),
        );
    }
    let target = check_inputs(&cli.command, &config, ctx, session)?;
    check_interaction(&cli.command, session.interactive, loc)?;
    if lots_of(&cli.command).is_empty()
        && let Command::Capture { mode } = &cli.command
        && let (Some(backend), Some(target), Some(job)) = (ctx.backend, &target, job_of(mode))
    {
        return capture::run(&job, backend, &config, target, ctx, session);
    }
    Err(unsupported(&cli.command, loc))
}

/// The capture job for the modes that have a backend; `area` needs the overlay (lot 2.5).
fn job_of(mode: &CaptureCommand) -> Option<Job<'_>> {
    match mode {
        CaptureCommand::Screen(opts) => Some(Job {
            mode: CaptureMode::Screen,
            connector: None,
            opts,
        }),
        CaptureCommand::Output { connector, opts } => Some(Job {
            mode: CaptureMode::Output,
            connector: connector.as_deref(),
            opts,
        }),
        CaptureCommand::Area { .. } => None,
    }
}

fn config_file(dir: Option<&Path>) -> Option<PathBuf> {
    let dir = match dir {
        Some(d) => d.to_owned(),
        None => config_dir().ok()?,
    };
    Some(dir.join(CONFIG_FILE))
}

fn print_version(ctx: &Context<'_>, session: &mut Session<'_, '_>) {
    let version = env!("CARGO_PKG_VERSION");
    if session.json {
        session.document(&json!({
            "schema": "kropa.version/1",
            "status": "ok",
            "exit_code": 0,
            "version": version,
            "commit": COMMIT,
            "target": TARGET,
            "ipc_protocol": null,
            "schemas": SCHEMAS,
        }));
        return;
    }
    let loc = ctx.loc;
    let commit = COMMIT.map_or_else(|| loc.message("out-unknown"), str::to_owned);
    session.result(&loc.message_args("out-version", &[("version", version)]));
    session.result(&loc.message_args("out-version-commit", &[("commit", &commit)]));
    session.result(&loc.message_args("out-version-target", &[("target", TARGET)]));
    session.result(&loc.message("out-version-ipc"));
    session.result(&loc.message_args("out-version-schemas", &[("schemas", &SCHEMAS.join(", "))]));
}

// ---------------------------------------------------------------------------------------------
// Secrets never go in arguments (docs/cli.md section 1, principle 5)
// ---------------------------------------------------------------------------------------------

fn refuse_secrets_in_arguments(cmd: &Command, loc: &Localizer) -> Result<(), CliError> {
    let mut urls: Vec<(&str, &str)> = Vec::new();
    let mut vars: &[(String, String)] = &[];
    let mut sets: &[(String, String)] = &[];
    match cmd {
        Command::Capture { mode } => {
            vars = match mode {
                CaptureCommand::Screen(o)
                | CaptureCommand::Output { opts: o, .. }
                | CaptureCommand::Area { opts: o, .. } => &o.vars,
            };
        }
        Command::Upload(a) => vars = &a.vars,
        Command::Dest { action } => match action {
            DestCommand::Add(a) => {
                let t = &a.type_opts;
                for (name, value) in [
                    ("--url", &a.url),
                    ("--endpoint", &t.endpoint),
                    ("--public-base-url", &t.public_base_url),
                    ("--public-url", &t.public_url),
                ] {
                    if let Some(v) = value {
                        urls.push((name, v));
                    }
                }
            }
            DestCommand::Edit { url, sets: s, .. } => {
                if let Some(v) = url {
                    urls.push(("--url", v));
                }
                sets = s;
            }
            _ => {}
        },
        _ => {}
    }
    // Only the name of the option goes in the message, never the value.
    let refuse = |option: &str| {
        Err(CliError::new(
            ExitCode::SecurityRefused,
            loc,
            "error-secret-in-argument",
            &[("option", option)],
        )
        .with_reason("secret_in_argument"))
    };
    for (name, url) in urls {
        if url_carries_secret(url) {
            return refuse(name);
        }
    }
    for (key, value) in sets {
        if set_carries_secret(key, value) {
            return refuse("--set");
        }
    }
    // A variable may legitimately be named `password` (it is answered on purpose), but a value that
    // looks like a credential is never accepted.
    for (_, value) in vars {
        if var_carries_secret(value) {
            return refuse("--var");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Inputs that can be checked now
// ---------------------------------------------------------------------------------------------

fn require_file(path: &Path, loc: &Localizer) -> Result<(), CliError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(CliError::new(
            ExitCode::Io,
            loc,
            "error-file-missing",
            &[("path", &path.display().to_string())],
        ))
    }
}

fn require_destination(
    explicit: Option<&Option<String>>,
    config: &Config,
    loc: &Localizer,
) -> Result<(), CliError> {
    // `--upload=<profile>` names one; bare `--upload` and `upload` without `--to` need the default.
    let named = explicit.is_some_and(|p| p.as_deref().is_some_and(|s| !s.is_empty()));
    if named || config.default_destination.is_some() {
        Ok(())
    } else {
        Err(CliError::new(
            ExitCode::Config,
            loc,
            "error-no-default-destination",
            &[],
        ))
    }
}

/// Checks what can be checked before anything is done; for a capture, returns where the file goes.
fn check_inputs(
    cmd: &Command,
    config: &Config,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<Option<Target>, CliError> {
    let loc = ctx.loc;
    let mut resolved = None;
    match cmd {
        Command::Capture { mode } => {
            let (name, connector, opts) = match mode {
                CaptureCommand::Screen(o) => ("screen", None, o),
                CaptureCommand::Output { connector, opts } => {
                    ("output", connector.as_deref(), opts)
                }
                CaptureCommand::Area { opts, .. } => ("area", None, opts),
            };
            if let Some(upload) = &opts.send.upload {
                require_destination(Some(upload), config, loc)?;
            }
            let spec = TargetSpec {
                mode: name,
                connector,
                output: opts.output.as_deref(),
                name: opts.name.as_deref(),
                force: opts.force,
            };
            let target = resolve_target(&spec, config, ctx.clock, ctx.host.as_deref(), loc)?;
            let note = match &target {
                Target::File(p) => {
                    loc.message_args("note-target", &[("path", &p.display().to_string())])
                }
                Target::Deferred(d) => {
                    loc.message_args("note-target-deferred", &[("dir", &d.display().to_string())])
                }
            };
            session.debug(1, &note);
            resolved = Some(target);
        }
        Command::Upload(a) => {
            require_file(&a.file, loc)?;
            if a.to.is_none() {
                require_destination(None, config, loc)?;
            }
        }
        Command::Edit(a) => {
            require_file(&a.file, loc)?;
            if let Some(upload) = &a.send.upload {
                require_destination(Some(upload), config, loc)?;
            }
        }
        Command::Dest {
            action: DestCommand::Import { file, .. },
        } => require_file(file, loc)?,
        _ => {}
    }
    Ok(resolved)
}

/// Without a terminal (or with `--no-input`) nothing may wait for an answer: code 12.
fn check_interaction(cmd: &Command, interactive: bool, loc: &Localizer) -> Result<(), CliError> {
    if interactive {
        return Ok(());
    }
    let needs_confirmation = match cmd {
        Command::Dest {
            action:
                DestCommand::Test { yes, .. }
                | DestCommand::Remove { yes, .. }
                | DestCommand::Import { yes, .. },
        }
        | Command::Secrets {
            action: SecretsCommand::Remove { yes, .. },
        }
        | Command::Queue {
            action: QueueCommand::Clear { yes },
        } => !yes,
        _ => false,
    };
    if needs_confirmation {
        return Err(CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-confirmation-required",
            &[],
        ));
    }
    let source = match cmd {
        Command::Dest {
            action: DestCommand::Add(a),
        } => Some(&a.secret),
        Command::Secrets {
            action: SecretsCommand::Set { source, .. },
        } => Some(source),
        _ => None,
    };
    if source.is_some_and(|s| !s.secret_stdin && s.secret_fd.is_none()) {
        return Err(CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-secret-input-required",
            &[],
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{COMMIT, SCHEMAS};

    /// The commit of a build is absent (no git checkout) or 12 hexadecimal digits, with `-dirty`
    /// when the working tree had changes.
    fn is_commit(text: &str) -> bool {
        let hex = text.strip_suffix("-dirty").unwrap_or(text);
        hex.len() == 12 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    #[test]
    fn build_commit_is_absent_or_twelve_hexadecimal_digits() {
        assert!(COMMIT.is_none_or(is_commit), "{COMMIT:?}");
        assert!(is_commit("0123456789ab"));
        assert!(is_commit("0123456789ab-dirty"));
        for bad in [
            "",
            "0123456789a",
            "0123456789abc",
            "0123456789AB",
            "0123456789ag",
            "0123456789ab-dirt",
        ] {
            assert!(!is_commit(bad), "{bad}");
        }
    }

    #[test]
    fn schemas_are_the_documented_ones() {
        // docs/cli.md section 4: capture and upload, then one schema per listing command, and the
        // errors of every command without a schema of its own.
        assert_eq!(
            SCHEMAS,
            [
                "kropa.capture/1",
                "kropa.upload/1",
                "kropa.dest.list/1",
                "kropa.queue.list/1",
                "kropa.history.list/1",
                "kropa.doctor/1",
                "kropa.secrets.status/1",
                "kropa.version/1",
                "kropa.error/1",
            ]
        );
        for schema in SCHEMAS {
            assert!(
                schema.starts_with("kropa.") && schema.ends_with("/1"),
                "{schema}"
            );
        }
    }
}
