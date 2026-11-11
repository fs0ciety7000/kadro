// SPDX-License-Identifier: GPL-3.0-or-later

//! Separation of stdout (the result) and stderr (messages), and the JSON envelope (`docs/cli.md`
//! sections 1 and 4).

use std::io::Write;

use kropa_core::ExitCode;
use serde_json::{Value, json};

use crate::errors::CliError;

/// Output streams and terminal facts, injectable for tests.
pub struct Io<'a> {
    /// Standard output: the result only.
    pub out: &'a mut dyn Write,
    /// Standard error: progress, warnings, errors.
    pub err: &'a mut dyn Write,
    /// Whether stdin is a terminal.
    pub stdin_is_tty: bool,
}

/// Output of one run, shaped by the global options.
pub struct Session<'a, 'b> {
    io: &'a mut Io<'b>,
    /// `--json`.
    pub json: bool,
    /// `--quiet`.
    pub quiet: bool,
    /// `-v` count.
    pub verbose: u8,
    /// Whether the command may prompt: a terminal on stdin and no `--no-input`.
    pub interactive: bool,
}

impl<'a, 'b> Session<'a, 'b> {
    /// A session writing to `io`.
    pub fn new(io: &'a mut Io<'b>, json: bool, quiet: bool, verbose: u8, no_input: bool) -> Self {
        let interactive = io.stdin_is_tty && !no_input;
        Self {
            io,
            json,
            quiet,
            verbose,
            interactive,
        }
    }

    // A closed pipe or terminal leaves nothing sensible to do: write errors are ignored.

    /// One line of result on stdout (human mode).
    pub fn result(&mut self, line: &str) {
        let _ = writeln!(self.io.out, "{line}");
    }

    /// The single JSON object on stdout.
    pub fn document(&mut self, value: &Value) {
        let _ = writeln!(self.io.out, "{value}");
    }

    /// A message on stderr, hidden by `--quiet`.
    pub fn note(&mut self, line: &str) {
        if !self.quiet {
            let _ = writeln!(self.io.err, "{line}");
        }
    }

    /// A message shown from `-v` (`level` 1) or `-vv` (`level` 2) on.
    pub fn debug(&mut self, level: u8, line: &str) {
        if self.verbose >= level {
            self.note(line);
        }
    }

    /// Raw text on stderr (clap's own rendering), always shown.
    pub fn raw_err(&mut self, text: &str) {
        let _ = write!(self.io.err, "{text}");
    }

    /// Reports a failure: message on stderr (errors survive `--quiet`), and with `--json` the error
    /// envelope on stdout. Returns the exit code.
    pub fn fail(&mut self, schema: &str, error: &CliError) -> ExitCode {
        let _ = writeln!(self.io.err, "kropa: {}", error.message);
        if self.json {
            let doc = error_document(schema, error);
            self.document(&doc);
        }
        error.code
    }
}

/// The error envelope: `schema`, `status`, `exit_code` and `error` (`docs/cli.md` section 4).
#[must_use]
pub fn error_document(schema: &str, error: &CliError) -> Value {
    let mut detail = json!({
        "code": error.code.name(),
        "reason": error.reason,
        "message": error.message,
        "step": error.step,
    });
    if !error.lots.is_empty() {
        detail["lots"] = json!(error.lots);
    }
    json!({
        "schema": schema,
        "status": "error",
        "exit_code": error.code.code(),
        "error": detail,
    })
}

#[cfg(test)]
mod tests {
    use kropa_core::ExitCode;

    use super::{Io, Session, error_document};
    use crate::errors::CliError;

    fn err(code: ExitCode) -> CliError {
        CliError {
            code,
            message: "boom".into(),
            step: None,
            lots: vec!["1.7"],
            reason: None,
        }
    }

    #[test]
    fn streams_are_separated_and_quiet_keeps_errors() {
        let (mut out, mut errb) = (Vec::new(), Vec::new());
        let mut io = Io {
            out: &mut out,
            err: &mut errb,
            stdin_is_tty: false,
        };
        let mut s = Session::new(&mut io, false, true, 0, false);
        s.result("value");
        s.note("hidden");
        let code = s.fail("kropa.x/1", &err(ExitCode::Config));
        assert_eq!(code, ExitCode::Config);
        assert_eq!(String::from_utf8(out).unwrap(), "value\n");
        assert_eq!(String::from_utf8(errb).unwrap(), "kropa: boom\n");
    }

    #[test]
    fn json_failure_is_one_object_on_stdout() {
        let (mut out, mut errb) = (Vec::new(), Vec::new());
        let mut io = Io {
            out: &mut out,
            err: &mut errb,
            stdin_is_tty: true,
        };
        let mut s = Session::new(&mut io, true, false, 0, true);
        assert!(!s.interactive, "--no-input wins over a terminal");
        s.fail("kropa.x/1", &err(ExitCode::Unsupported));
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["schema"], "kropa.x/1");
        assert_eq!(v["status"], "error");
        assert_eq!(v["exit_code"], 5);
        assert_eq!(v["error"]["code"], "unsupported");
        assert_eq!(v["error"]["step"], serde_json::Value::Null);
        assert_eq!(v["error"]["lots"][0], "1.7");
        assert_eq!(
            error_document("s", &err(ExitCode::Io))["error"]["code"],
            "io"
        );
    }

    #[test]
    fn verbosity_levels() {
        let (mut out, mut errb) = (Vec::new(), Vec::new());
        let mut io = Io {
            out: &mut out,
            err: &mut errb,
            stdin_is_tty: false,
        };
        let mut s = Session::new(&mut io, false, false, 1, false);
        s.debug(1, "one");
        s.debug(2, "two");
        assert_eq!(String::from_utf8(errb).unwrap(), "one\n");
    }
}
