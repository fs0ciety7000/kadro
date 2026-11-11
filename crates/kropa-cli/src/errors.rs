// SPDX-License-Identifier: GPL-3.0-or-later

//! Run-time errors of the CLI and localisation of clap's usage errors.

use std::fmt::Write as _;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use kropa_core::{CoreError, ExitCode};
use kropa_i18n::Localizer;

use crate::args::ArgError;

/// A failure with its exit code and its (already localised) message.
#[derive(Debug, Clone)]
pub struct CliError {
    /// Process exit code.
    pub code: ExitCode,
    /// Human message, in the language in use. Never contains a secret.
    pub message: String,
    /// Step that failed, for `error.step` in JSON output (`None` when the command did not start).
    pub step: Option<&'static str>,
    /// Lots that bring the missing feature (only for `unsupported`).
    pub lots: Vec<&'static str>,
    /// Machine sub-cause, `error.reason` in JSON (`docs/cli.md` section 4); `None` when there is none.
    pub reason: Option<String>,
    /// Warnings that go with the failure (already localised), `error.warnings` in JSON when not
    /// empty: what the command could not undo, for instance.
    pub warnings: Vec<String>,
}

impl CliError {
    /// An error with a Fluent message.
    #[must_use]
    pub fn new(code: ExitCode, loc: &Localizer, id: &str, args: &[(&str, &str)]) -> Self {
        Self {
            code,
            message: loc.message_args(id, args),
            step: None,
            lots: Vec::new(),
            reason: None,
            warnings: Vec::new(),
        }
    }

    /// The same error with a machine sub-cause.
    #[must_use]
    pub fn with_reason(mut self, reason: &str) -> Self {
        self.reason = Some(reason.to_owned());
        self
    }

    /// The same error attributed to a step of the chain (`capture`, `save`, ...).
    #[must_use]
    pub fn at_step(mut self, step: &'static str) -> Self {
        self.step = Some(step);
        self
    }

    /// Wraps an error of `kropa-core`; its own text is technical and stays in English.
    #[must_use]
    pub fn from_core(err: &CoreError, loc: &Localizer) -> Self {
        let id = match err.exit_code() {
            ExitCode::Config => "error-config",
            ExitCode::Usage => "error-name",
            _ => "error-io",
        };
        let text = err.to_string();
        // The configuration message of the core already says "invalid configuration: ".
        let detail = text
            .strip_prefix("invalid configuration: ")
            .unwrap_or(&text);
        Self::new(err.exit_code(), loc, id, &[("detail", detail)])
    }
}

fn context_text(value: &ContextValue) -> Option<String> {
    match value {
        ContextValue::String(s) => Some(s.clone()),
        ContextValue::Strings(v) => Some(v.join(", ")),
        ContextValue::StyledStr(s) => Some(s.to_string()),
        ContextValue::StyledStrs(v) => Some(
            v.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        ContextValue::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn ctx(err: &clap::Error, kind: ContextKind) -> String {
    err.get(kind).and_then(context_text).unwrap_or_default()
}

/// The name of an unknown option (`--bogus`, `-x`) out of what clap reports, and nothing of a value:
/// `--opt=VALUE` is cut at the `=`, and anything that is not shaped like an option name (a
/// positional value such as a secret given in the wrong place) gives `None`.
fn option_name(raw: &str) -> Option<String> {
    if let Some(long) = raw.strip_prefix("--") {
        let name = long.split('=').next().unwrap_or_default();
        let shaped = (1..=40).contains(&name.len())
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        return shaped.then(|| format!("--{name}"));
    }
    let short = raw.strip_prefix('-')?.chars().next()?;
    short.is_ascii_alphanumeric().then(|| format!("-{short}"))
}

/// The message of a clap error in the language in use.
///
/// It never repeats a value typed by the user, which may be a secret given in the wrong place: an
/// invalid or unexpected value is reported by the name of its option (or `<NAME>` for a positional),
/// an unexpected positional by "unexpected extra argument", and kinds that are not translated by a
/// generic line.
fn message(err: &clap::Error, loc: &Localizer) -> String {
    let arg = ctx(err, ContextKind::InvalidArg);
    match err.kind() {
        ErrorKind::UnknownArgument => match option_name(&arg) {
            Some(name) => loc.message_args("error-unknown-argument", &[("arg", &name)]),
            None => loc.message("error-extra-argument"),
        },
        ErrorKind::InvalidSubcommand => loc.message("error-unknown-subcommand"),
        ErrorKind::InvalidValue => {
            let mut text = loc.message_args("error-invalid-value", &[("arg", &arg)]);
            let valid = ctx(err, ContextKind::ValidValue);
            if !valid.is_empty() {
                text.push_str("\n  ");
                text.push_str(&loc.message_args("error-possible-values", &[("values", &valid)]));
            }
            text
        }
        ErrorKind::ValueValidation => {
            // Only the Fluent reason of our own parsers: a foreign source error may quote the value.
            let reason = std::error::Error::source(err)
                .and_then(|s| s.downcast_ref::<ArgError>())
                .map(|e| loc.message_args(e.id, &[("detail", &e.detail)]));
            match reason {
                Some(reason) => loc.message_args(
                    "error-value-validation",
                    &[("arg", &arg), ("reason", &reason)],
                ),
                None => loc.message_args("error-invalid-value", &[("arg", &arg)]),
            }
        }
        ErrorKind::ArgumentConflict => loc.message_args(
            "error-conflict",
            &[("arg", &arg), ("other", &ctx(err, ContextKind::PriorArg))],
        ),
        ErrorKind::MissingRequiredArgument => {
            loc.message_args("error-missing-required", &[("args", &arg)])
        }
        ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            loc.message("error-missing-subcommand")
        }
        ErrorKind::NoEquals => loc.message_args("error-no-equals", &[("arg", &arg)]),
        ErrorKind::TooFewValues | ErrorKind::WrongNumberOfValues => {
            loc.message_args("error-missing-value", &[("arg", &arg)])
        }
        ErrorKind::TooManyValues => loc.message_args("error-too-many-values", &[("arg", &arg)]),
        _ => loc.message("error-usage-generic"),
    }
}

/// Full text of a usage error: message, suggestion, usage and hint.
#[must_use]
pub fn localize_clap_error(err: &clap::Error, loc: &Localizer) -> String {
    let mut out = format!("{} {}\n", loc.message("error-prefix"), message(err, loc));
    for kind in [ContextKind::SuggestedSubcommand, ContextKind::SuggestedArg] {
        let tip = ctx(err, kind);
        if !tip.is_empty() {
            let _ = write!(out, "\n  {} '{tip}'\n", loc.message("error-tip"));
        }
    }
    let usage = ctx(err, ContextKind::Usage);
    let usage = usage
        .trim()
        .strip_prefix("Usage:")
        .unwrap_or(usage.trim())
        .trim();
    if !usage.is_empty() {
        let _ = write!(out, "\n{} {usage}\n", loc.message("cli-usage-heading"));
    }
    let _ = write!(out, "\n{}\n", loc.message("error-hint"));
    out
}

/// One-line plain message of a clap error, for the JSON `error.message`.
#[must_use]
pub fn plain_clap_message(err: &clap::Error, loc: &Localizer) -> String {
    message(err, loc)
}

#[cfg(test)]
mod tests {
    use super::option_name;

    #[test]
    fn only_option_names_are_echoed() {
        assert_eq!(option_name("--bogus").as_deref(), Some("--bogus"));
        assert_eq!(option_name("--bogus=sk-live-1").as_deref(), Some("--bogus"));
        assert_eq!(option_name("-x").as_deref(), Some("-x"));
        assert_eq!(option_name("-xvalue").as_deref(), Some("-x"));
        for value in ["sk-live-ABC", "", "--", "---x", "--a b", "-", "-é", "--é"] {
            assert_eq!(option_name(value), None, "{value:?}");
        }
        assert_eq!(option_name(&format!("--{}", "a".repeat(41))), None);
    }
}
