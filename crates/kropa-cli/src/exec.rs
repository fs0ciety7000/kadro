// SPDX-License-Identifier: GPL-3.0-or-later

//! Execution of a parsed command line.
//!
//! What does not depend on another lot is done for real: `version`, argument checks (secrets never
//! in arguments), configuration loading, input files, name resolution and the refusal to prompt
//! without a terminal. A command whose lot is not delivered ends with `unsupported` (5) and says
//! which lot brings it; nothing is simulated.

use std::path::{Path, PathBuf};

use kropa_core::{CONFIG_FILE, Clock, Config, ExitCode, config_dir};
use kropa_i18n::Localizer;
use serde_json::json;

use crate::args::{
    CaptureCommand, Cli, Command, DestCommand, HistoryCommand, QueueCommand, SecretsCommand,
};
use crate::errors::CliError;
use crate::output::Session;
use crate::target::{Target, TargetSpec, resolve_target};

/// Version of the JSON schema of the errors that no command schema covers (usage errors).
pub const ERROR_SCHEMA: &str = "kropa.error/1";

/// JSON schemas this binary speaks (`docs/cli.md` section 4).
pub const SCHEMAS: [&str; 10] = [
    "kropa.capture/1",
    "kropa.upload/1",
    "kropa.dest.list/1",
    "kropa.queue.list/1",
    "kropa.history.list/1",
    "kropa.doctor/1",
    "kropa.secrets.status/1",
    "kropa.version/1",
    "kropa.error/1",
    "kropa.edit/1",
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

/// JSON schema name of a command.
#[must_use]
pub fn schema_of(cmd: &Command) -> String {
    format!("kropa.{}/1", path(cmd).join("."))
}

/// Lots that must be delivered for the command to work, per `docs/plan.md`.
fn lots_of(cmd: &Command) -> Vec<&'static str> {
    let platform_capture = if cfg!(target_os = "macos") { "1.5" } else { "1.4" };
    match cmd {
        Command::Capture { mode } => {
            let (base, opts) = match mode {
                CaptureCommand::Screen(o) | CaptureCommand::Output { opts: o, .. } => {
                    (platform_capture, o)
                }
                CaptureCommand::Area { opts, .. } => ("2.5", opts),
            };
            let mut lots = vec![base];
            if opts.send.upload.is_some() {
                lots.push("1.7");
            }
            if opts.edit {
                lots.push("3.7");
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
        Command::Doctor => vec![platform_capture],
        Command::Version(_) => vec!["5"],
    }
}

fn unsupported(cmd: &Command, loc: &Localizer) -> CliError {
    let lots = lots_of(cmd);
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
        // `version` needs neither the configuration nor the disk.
        if v.check {
            return Err(unsupported(&cli.command, loc));
        }
        print_version(ctx, session);
        return Ok(ExitCode::Ok);
    }
    refuse_secrets_in_arguments(&cli.command, loc)?;
    let config_dir_arg = cli.global.config.as_deref();
    let config = Config::load_from(config_dir_arg).map_err(|e| CliError::from_core(&e, loc))?;
    if let Some(file) = config_file(config_dir_arg) {
        session.debug(
            1,
            &loc.message_args("note-config", &[("path", &file.display().to_string())]),
        );
    }
    check_inputs(&cli.command, &config, ctx, session)?;
    check_interaction(&cli.command, session.interactive, loc)?;
    Err(unsupported(&cli.command, loc))
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

const SENSITIVE_WORDS: [&str; 10] = [
    "authorization",
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api-key",
    "api_key",
    "bearer",
    "credential",
];

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SENSITIVE_WORDS.iter().any(|w| key.contains(w))
}

fn looks_like_credential_value(value: &str) -> bool {
    let value = value.trim_start().to_ascii_lowercase();
    value.starts_with("bearer ") || value.starts_with("basic ")
}

/// A URL with `user:pass@` in it, or a query parameter named like a secret.
fn url_carries_secret(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (before_query, query) = match rest.split_once('?') {
        Some((a, q)) => (a, q.split('#').next().unwrap_or_default()),
        None => (rest.split('#').next().unwrap_or(rest), ""),
    };
    let authority = before_query.split('/').next().unwrap_or_default();
    authority.contains('@')
        || query
            .split('&')
            .any(|pair| is_sensitive_key(pair.split('=').next().unwrap_or_default()))
}

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
    let refuse = |option: &str| {
        Err(CliError::new(
            ExitCode::SecurityRefused,
            loc,
            "error-secret-in-argument",
            &[("option", option)],
        ))
    };
    for (name, url) in urls {
        if url_carries_secret(url) {
            return refuse(name);
        }
    }
    for (key, value) in sets {
        if is_sensitive_key(key) || looks_like_credential_value(value) {
            return refuse("--set");
        }
    }
    // A variable may legitimately be named `password` (it is answered on purpose), but a value that
    // is an `Authorization` header is never accepted.
    for (_, value) in vars {
        if looks_like_credential_value(value) {
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

fn check_inputs(
    cmd: &Command,
    config: &Config,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<(), CliError> {
    let loc = ctx.loc;
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
            let note = match target {
                Target::File(p) => {
                    loc.message_args("note-target", &[("path", &p.display().to_string())])
                }
                Target::Deferred(d) => {
                    loc.message_args("note-target-deferred", &[("dir", &d.display().to_string())])
                }
            };
            session.debug(1, &note);
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
    Ok(())
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
    use super::{is_sensitive_key, looks_like_credential_value, url_carries_secret};

    #[test]
    fn urls_with_credentials_are_detected() {
        for bad in [
            "https://user:pw@host/x",
            "https://user@host",
            "https://host/p?token=abc",
            "https://host/p?a=1&X-Api-Key=abc",
            "host:1@x",
        ] {
            assert!(url_carries_secret(bad), "{bad}");
        }
        for good in [
            "https://host/path@v2",
            "https://host/p?a=1&b=2#frag@x",
            "https://zipline.example.org",
        ] {
            assert!(!url_carries_secret(good), "{good}");
        }
    }

    #[test]
    fn sensitive_keys_and_values() {
        assert!(is_sensitive_key("headers.Authorization"));
        assert!(is_sensitive_key("API_KEY"));
        assert!(!is_sensitive_key("response.url"));
        assert!(looks_like_credential_value("Bearer abc"));
        assert!(looks_like_credential_value(" basic abc"));
        assert!(!looks_like_credential_value("bearers"));
    }
}
