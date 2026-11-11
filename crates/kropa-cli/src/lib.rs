// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line client of Kropa (`kropa`), free of any GTK dependency.
//!
//! Layout: [`args`] is the typed command line (clap derive), [`command`] gives it its Fluent texts,
//! [`errors`] localises clap's usage errors, [`output`] separates stdout and stderr and builds the
//! JSON envelope, [`target`] resolves file names, [`capture`] and [`doctor`] are the commands that
//! use the capture backend, [`secret_args`] spots secrets given in arguments, and [`exec`] runs the
//! commands.

pub mod args;
pub mod capture;
pub mod command;
pub mod dest;
pub mod doctor;
pub mod errors;
pub mod exec;
pub mod output;
pub mod secret_args;
pub mod secret_input;
pub mod secrets;
pub mod target;
pub mod upload;

use std::ffi::OsString;
use std::io::IsTerminal;
use std::time::Duration;

use clap::error::ErrorKind;
use clap::{ColorChoice, FromArgMatches, ValueEnum};
use kropa_capture::CaptureBackend;
use kropa_core::{Clock, Config, ExitCode, SystemClock};
use kropa_i18n::{I18nError, Lang, Localizer};
use kropa_upload::secret::SecretStore;

pub use output::Io;

use args::{Cli, ColorWhen};
use errors::{CliError, localize_clap_error, plain_clap_message};
use exec::{Context, ERROR_SCHEMA};
use output::{Session, error_document};
use secret_input::{SecretInput, SystemInput};

/// Finds the value of a long option in raw arguments, before the real parsing: the help text must
/// already be in the right language when clap renders it. Stops at `--`. Returns the last occurrence.
fn prescan(args: &[OsString], option: &str) -> Option<String> {
    let equals = format!("{option}=");
    let mut found = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else { continue };
        if arg == "--" {
            break;
        }
        if arg == option {
            if let Some(value) = iter.next().and_then(|v| v.to_str()) {
                found = Some(value.to_owned());
            }
        } else if let Some(value) = arg.strip_prefix(&equals) {
            found = Some(value.to_owned());
        }
    }
    found
}

/// Value of `--lang` in raw arguments (see [`prescan`]).
#[must_use]
pub fn prescan_lang(args: &[OsString]) -> Option<String> {
    prescan(args, "--lang")
}

/// Whether `--json` appears in raw arguments, so that even a usage error can be answered in JSON.
fn prescan_json(args: &[OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "--json")
}

/// Colour choice before the real parsing (the help is rendered by clap with it). The accepted
/// values are those of the `--color` enum, so that the list lives in one place; an invalid one is
/// reported by the real parsing, and the help stays on `auto` meanwhile.
fn prescan_color(args: &[OsString]) -> ColorChoice {
    prescan(args, "--color")
        .and_then(|v| ColorWhen::from_str(&v, true).ok())
        .map_or(ColorChoice::Auto, command::color_choice)
}

/// The language in use: `--lang`, then `language` of the configuration, then the locale variables,
/// then English (`docs/memory/decisions.md`). The configuration is read only when `--lang` does not
/// settle the matter, and leniently: a missing or invalid file is reported by the command itself,
/// with the language of the other sources.
fn negotiated_language(args: &[OsString], lookup: &dyn Fn(&str) -> Option<String>) -> Lang {
    let cli = prescan_lang(args);
    let settled = cli.as_deref().is_some_and(|v| Lang::parse(v).is_ok());
    let config = if settled {
        None
    } else {
        let dir = prescan(args, "--config").map(std::path::PathBuf::from);
        Config::load_from(dir.as_deref())
            .ok()
            .and_then(|c| c.language)
    };
    kropa_i18n::negotiate(cli.as_deref(), config.as_deref(), lookup)
}

/// Runs the command line on the real standard streams: `args` includes the program name, `lookup`
/// reads the standard locale variables. The result is the process exit code.
#[must_use]
pub fn run(args: &[OsString], lookup: &dyn Fn(&str) -> Option<String>) -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    let mut io = Io {
        out: &mut out,
        err: &mut err,
        stdin_is_tty: std::io::stdin().is_terminal(),
    };
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let host = (!host.is_empty()).then_some(host);
    run_with(args, lookup, &mut io, &SystemClock, host)
}

/// Like [`run`], with injected streams, clock and host name (tests). The capture backend is the one
/// of the platform.
#[must_use]
pub fn run_with(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
    io: &mut Io<'_>,
    clock: &dyn Clock,
    host: Option<String>,
) -> ExitCode {
    let backend = kropa_capture::default_backend();
    run_with_backend(
        args,
        lookup,
        io,
        clock,
        host,
        backend.as_deref(),
        &std::thread::sleep,
    )
}

/// What the commands need from outside the process, injectable for tests.
pub struct Services<'a> {
    /// Where secrets are read and written: the system keyring on Linux, nothing (exit 5) on macOS
    /// until the Keychain broker of lot 1.8m.
    pub secrets: &'a dyn SecretStore,
    /// Where the value of a secret is read from (terminal, stdin, descriptor).
    pub input: &'a dyn SecretInput,
    /// Answers a yes/no question put to the user on a terminal.
    pub confirm: &'a dyn Fn(&str) -> bool,
}

/// Asks on the terminal: the prompt on stderr, the answer on stdin (`y`, `yes`, `o`, `oui`).
fn ask_on_terminal(prompt: &str) -> bool {
    use std::io::{BufRead as _, Write as _};
    let _ = write!(std::io::stderr(), "{prompt} ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(
        line.trim().to_lowercase().as_str(),
        "y" | "yes" | "o" | "oui"
    )
}

/// Like [`run_with`], with an injected capture backend (`None`: the platform has none) and the
/// function that waits for `--delay`.
#[must_use]
pub fn run_with_backend(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
    io: &mut Io<'_>,
    clock: &dyn Clock,
    host: Option<String>,
    backend: Option<&dyn CaptureBackend>,
    sleep: &dyn Fn(Duration),
) -> ExitCode {
    // Opens nothing: the store connects when a command needs it.
    let store = kropa_store::open_system_store(std::sync::Arc::new(SystemClock));
    let services = Services {
        secrets: store.as_ref(),
        input: &SystemInput,
        confirm: &ask_on_terminal,
    };
    run_with_services(args, lookup, io, clock, host, backend, sleep, &services)
}

/// Like [`run_with_backend`], with the secret store and the confirmation injected.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn run_with_services(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
    io: &mut Io<'_>,
    clock: &dyn Clock,
    host: Option<String>,
    backend: Option<&dyn CaptureBackend>,
    sleep: &dyn Fn(Duration),
    services: &Services<'_>,
) -> ExitCode {
    match run_inner(args, lookup, io, clock, host, backend, sleep, services) {
        Ok(code) => code,
        Err(e) => {
            // The localizer itself is broken: the only message possible is an English one.
            let _ = std::io::Write::write_fmt(io.err, format_args!("kropa: {e}\n"));
            ExitCode::Internal
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
    io: &mut Io<'_>,
    clock: &dyn Clock,
    host: Option<String>,
    backend: Option<&dyn CaptureBackend>,
    sleep: &dyn Fn(Duration),
    services: &Services<'_>,
) -> Result<ExitCode, I18nError> {
    let lang = negotiated_language(args, lookup);
    let loc = Localizer::new(lang)?;
    let command = command::build_command(&loc, prescan_color(args));
    let parsed = command
        .try_get_matches_from(args)
        .and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(e) => return Ok(report_usage_error(&e, &loc, prescan_json(args), io)),
    };
    let g = &cli.global;
    let mut session = Session::new(io, g.json, g.quiet, g.verbose, g.no_input);
    let ctx = Context {
        loc: &loc,
        clock,
        host,
        backend,
        sleep,
        secrets: services.secrets,
        input: services.input,
        confirm: services.confirm,
    };
    // A locked keyring is only unlocked with a prompt when someone can answer it: a terminal, and a
    // graphical session to show the dialog on. Otherwise the call fails at once (exit 7).
    let display = lookup("WAYLAND_DISPLAY").is_some() || lookup("DISPLAY").is_some();
    ctx.secrets
        .set_interactive(session.interactive && (display || cfg!(target_os = "macos")));
    Ok(exec::execute(&cli, &ctx, &mut session))
}

/// Help and version go to stdout with code 0; every parse error is a usage error (2), localised.
/// clap's own rendering is used only for help: its error texts quote the values typed by the user.
fn report_usage_error(err: &clap::Error, loc: &Localizer, json: bool, io: &mut Io<'_>) -> ExitCode {
    let mut session = Session::new(io, json, false, 0, false);
    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // A closed terminal leaves nothing sensible to do.
            let _ = err.print();
            return ExitCode::Ok;
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            // Bare `kropa dest`: the help, on stderr, as a usage error.
            let _ = err.print();
        }
        _ => session.raw_err(&localize_clap_error(err, loc)),
    }
    if json {
        let error = CliError {
            code: ExitCode::Usage,
            message: plain_clap_message(err, loc),
            step: None,
            lots: Vec::new(),
            reason: None,
        };
        session.document(&error_document(ERROR_SCHEMA, &error));
    }
    ExitCode::Usage
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use kropa_core::{ExitCode, FixedClock};

    use super::{Io, prescan_json, prescan_lang, run_with};

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    struct Run {
        code: ExitCode,
        out: String,
        err: String,
    }

    fn run(list: &[&str], tty: bool) -> Run {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
        let code = {
            let mut io = Io {
                out: &mut out,
                err: &mut err,
                stdin_is_tty: tty,
            };
            run_with(
                &args(list),
                &|_| None,
                &mut io,
                &clock,
                Some("laptop".into()),
            )
        };
        Run {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    #[test]
    fn prescan_lang_and_json() {
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang", "fr"])).as_deref(),
            Some("fr")
        );
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang=en", "--help"])).as_deref(),
            Some("en")
        );
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang", "fr", "--lang=en"])).as_deref(),
            Some("en")
        );
        assert_eq!(prescan_lang(&args(&["kropa", "--", "--lang", "fr"])), None);
        assert_eq!(prescan_lang(&args(&["kropa", "--lang"])), None);
        assert!(prescan_json(&args(&["kropa", "x", "--json"])));
        assert!(!prescan_json(&args(&["kropa", "--", "--json"])));
    }

    #[test]
    fn version_json_is_one_object() {
        let r = run(&["kropa", "version", "--json"], false);
        assert_eq!(r.code, ExitCode::Ok);
        assert_eq!(r.err, "");
        let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
        assert_eq!(r.out.lines().count(), 1);
        assert_eq!(v["schema"], "kropa.version/1");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        assert!(v["ipc_protocol"].is_null());
    }

    #[test]
    fn usage_errors_are_localised_and_exit_2() {
        let r = run(
            &[
                "kropa", "--lang", "fr", "capture", "screen", "--delay", "99",
            ],
            false,
        );
        assert_eq!(r.code, ExitCode::Usage);
        assert_eq!(r.out, "");
        assert!(
            r.err.contains("valeur invalide pour '--delay <SEC>'"),
            "{}",
            r.err
        );
        assert!(!r.err.contains("99"), "{}", r.err);
        assert!(r.err.contains("--help"), "{}", r.err);
        let r = run(&["kropa", "--lang", "fr", "capture", "--bogus"], false);
        assert!(r.err.contains("argument inattendu '--bogus'"), "{}", r.err);
        assert!(r.err.contains("Utilisation : kropa capture"), "{}", r.err);
        let r = run(&["kropa", "--lang", "en", "capture", "--bogus"], false);
        assert_eq!(r.code, ExitCode::Usage);
        assert!(r.err.contains("unexpected argument '--bogus'"), "{}", r.err);
    }

    #[test]
    fn version_text_is_exact_where_it_does_not_depend_on_the_machine() {
        let r = run(&["kropa", "--lang", "en", "version"], false);
        assert_eq!(r.code, ExitCode::Ok);
        let lines: Vec<&str> = r.out.lines().collect();
        assert_eq!(lines[0], format!("kropa {}", env!("CARGO_PKG_VERSION")));
        assert!(lines[1].starts_with("commit: "), "{}", r.out);
        assert!(lines[2].starts_with("target: "), "{}", r.out);
        assert_eq!(lines[3], "IPC protocol: not available yet (lot 2.1)");
        assert!(
            lines[4].ends_with("kropa.version/1, kropa.error/1"),
            "{}",
            r.out
        );
    }

    #[test]
    fn usage_error_in_json_mode() {
        let r = run(&["kropa", "--json", "--lang", "en", "nope"], false);
        assert_eq!(r.code, ExitCode::Usage);
        let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
        assert_eq!(v["schema"], "kropa.error/1");
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["error"]["code"], "usage");
    }

    const SENTINEL: &str = "sk-live-SENTINEL123";

    /// No error, whatever its kind, text or JSON, may repeat what the user typed: it may be a secret.
    #[test]
    fn no_error_repeats_a_typed_value() {
        let cases: &[&[&str]] = &[
            &["secrets", "set", "zipline", "TOKEN", SENTINEL],
            &["dest", "add", "z", "--type", "zipline", "--url", SENTINEL],
            &["dest", "edit", "z", "--url", SENTINEL],
            &["dest", "edit", "z", "--set", SENTINEL],
            &["dest", "edit", "z", "--set", "token=sk-live-SENTINEL123"],
            &[
                "dest",
                "edit",
                "z",
                "--url",
                "https://h.example.org/?k=sk-live-SENTINEL123",
            ],
            &["capture", "screen", "--var", SENTINEL],
            &["capture", "screen", "--var", "h=sk-live-SENTINEL123"],
            &["capture", "screen", "--copy", SENTINEL],
            &["capture", "screen", "--delay", SENTINEL],
            &["capture", "screen", "--name", "{sk-live-SENTINEL123}"],
            &["capture", "screen", "--name", "x:sk-live-SENTINEL123"],
            &["capture", "screen", "--bogus=sk-live-SENTINEL123"],
            &["capture", "screen", "-zsk-live-SENTINEL123"],
            &["capture", "output", "a", SENTINEL],
            &["capture", SENTINEL],
            &["upload", "--to", SENTINEL, "f.png"],
            &["history", "list", "--limit", SENTINEL],
            &["history", "list", "--since", SENTINEL],
            &["dest", "add", "z", "--type", SENTINEL],
            &["dest", "add", "z", "--type", "s3", "--presign", SENTINEL],
            &["dest", "add", "z", "--type", "s3", "--port", SENTINEL],
            &["queue", "retry", SENTINEL, "--failed"],
            &["--color", SENTINEL, "version"],
            &["--lang", SENTINEL, "version"],
            &["--config", "", SENTINEL],
            &["--password-fd", SENTINEL, "version"],
            &["version", SENTINEL],
            &[SENTINEL],
        ];
        for lang in ["en", "fr"] {
            for json in [false, true] {
                for case in cases {
                    let mut argv = vec!["kropa", "--lang", lang];
                    if json {
                        argv.push("--json");
                    }
                    argv.extend_from_slice(case);
                    let r = run(&argv, false);
                    assert_ne!(r.code, ExitCode::Ok, "{argv:?}");
                    assert!(!r.out.contains("SENTINEL"), "{argv:?}\n{}", r.out);
                    assert!(!r.err.contains("SENTINEL"), "{argv:?}\n{}", r.err);
                    if json {
                        let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
                        assert_eq!(v["status"], "error", "{argv:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn language_comes_from_option_then_configuration_then_environment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "language = \"fr\"\n").unwrap();
        let config = dir.path().to_str().unwrap();
        let fr_env = |name: &str| (name == "LANG").then(|| "fr_FR.UTF-8".to_owned());
        let en_env = |name: &str| (name == "LANG").then(|| "en_US.UTF-8".to_owned());
        let error_of = |list: &[&str], lookup: &dyn Fn(&str) -> Option<String>| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
            let mut io = Io {
                out: &mut out,
                err: &mut err,
                stdin_is_tty: false,
            };
            let _ = run_with(&args(list), lookup, &mut io, &clock, None);
            String::from_utf8(err).unwrap()
        };
        // The configuration wins over the environment.
        let from_config = error_of(&["kropa", "--config", config, "nope"], &en_env);
        assert!(
            from_config.contains("sous-commande inconnue"),
            "{from_config}"
        );
        // `--lang` wins over the configuration.
        let from_option = error_of(
            &["kropa", "--config", config, "--lang", "en", "nope"],
            &fr_env,
        );
        assert!(
            from_option.contains("unrecognized subcommand"),
            "{from_option}"
        );
        // No configuration: the environment.
        let empty = tempfile::tempdir().unwrap();
        let from_env = error_of(
            &["kropa", "--config", empty.path().to_str().unwrap(), "nope"],
            &fr_env,
        );
        assert!(from_env.contains("sous-commande inconnue"), "{from_env}");
        // A configuration that does not parse does not decide the language.
        std::fs::write(dir.path().join("config.toml"), "language = [").unwrap();
        let broken = error_of(&["kropa", "--config", config, "nope"], &en_env);
        assert!(broken.contains("unrecognized subcommand"), "{broken}");
    }

    #[test]
    fn unsupported_commands_say_so() {
        let r = run(&["kropa", "--lang", "en", "queue", "list"], false);
        assert_eq!(r.code, ExitCode::Unsupported);
        assert_eq!(r.out, "");
        assert!(r.err.contains("lot 4.5"), "{}", r.err);
    }

    #[test]
    fn interaction_is_refused_without_a_terminal() {
        let args = ["kropa", "--lang", "en", "queue", "clear"];
        assert_eq!(run(&args, false).code, ExitCode::InteractionRequired);
        // On a terminal the confirmation would be asked: the feature is not there yet.
        assert_eq!(run(&args, true).code, ExitCode::Unsupported);
        // `--no-input` wins over the terminal.
        let mut with_flag = args.to_vec();
        with_flag.push("--no-input");
        assert_eq!(run(&with_flag, true).code, ExitCode::InteractionRequired);
        // `--yes` confirms.
        let mut yes = args.to_vec();
        yes.push("--yes");
        assert_eq!(run(&yes, false).code, ExitCode::Unsupported);
    }
}
