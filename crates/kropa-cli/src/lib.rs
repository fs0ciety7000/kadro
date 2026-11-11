// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line client of Kropa (`kropa`), free of any GTK dependency.
//!
//! Layout: [`args`] is the typed command line (clap derive), [`command`] gives it its Fluent texts,
//! [`errors`] localises clap's usage errors, [`output`] separates stdout and stderr and builds the
//! JSON envelope, [`target`] resolves file names, [`capture`] and [`doctor`] are the commands that
//! use the capture backend, and [`exec`] runs the commands.

pub mod args;
pub mod capture;
pub mod command;
pub mod doctor;
pub mod errors;
pub mod exec;
pub mod output;
pub mod target;

use std::ffi::OsString;
use std::io::IsTerminal;
use std::time::Duration;

use clap::error::ErrorKind;
use clap::{ColorChoice, FromArgMatches};
use kropa_capture::CaptureBackend;
use kropa_core::{Clock, ExitCode, SystemClock};
use kropa_i18n::{I18nError, Lang, Localizer};

pub use output::Io;

use args::Cli;
use errors::{CliError, localize_clap_error, plain_clap_message};
use exec::{Context, ERROR_SCHEMA};
use output::{Session, error_document};

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

fn prescan_color(args: &[OsString]) -> ColorChoice {
    match prescan(args, "--color").as_deref() {
        Some("always") => ColorChoice::Always,
        Some("never") => ColorChoice::Never,
        _ => ColorChoice::Auto,
    }
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
    match run_inner(args, lookup, io, clock, host, backend, sleep) {
        Ok(code) => code,
        Err(e) => {
            // The localizer itself is broken: the only message possible is an English one.
            let _ = std::io::Write::write_fmt(io.err, format_args!("kropa: {e}\n"));
            ExitCode::Internal
        }
    }
}

fn run_inner(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
    io: &mut Io<'_>,
    clock: &dyn Clock,
    host: Option<String>,
    backend: Option<&dyn CaptureBackend>,
    sleep: &dyn Fn(Duration),
) -> Result<ExitCode, I18nError> {
    let lang: Lang = kropa_i18n::negotiate(prescan_lang(args).as_deref(), lookup);
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
    };
    Ok(exec::execute(&cli, &ctx, &mut session))
}

/// Help and version go to stdout with code 0; every parse error is a usage error (2), localised
/// when the kind is covered, with clap's own English text otherwise.
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
        _ => match localize_clap_error(err, loc) {
            Some(text) => session.raw_err(&text),
            None => {
                let _ = err.print();
            }
        },
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
        assert!(r.err.contains("valeur invalide '99'"), "{}", r.err);
        assert!(r.err.contains("--help"), "{}", r.err);
        let r = run(&["kropa", "--lang", "fr", "capture", "--bogus"], false);
        assert!(r.err.contains("argument inattendu '--bogus'"), "{}", r.err);
        assert!(r.err.contains("Utilisation : kropa capture"), "{}", r.err);
        let r = run(&["kropa", "--lang", "en", "capture", "--bogus"], false);
        assert_eq!(r.code, ExitCode::Usage);
        assert!(r.err.contains("unexpected argument '--bogus'"), "{}", r.err);
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
