// SPDX-License-Identifier: GPL-3.0-or-later

//! Run-time errors of the CLI and localisation of clap's usage errors.

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
        }
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

/// The message of a clap error in the language in use, or `None` for the kinds not covered (the
/// caller then falls back to clap's own English text).
fn message(err: &clap::Error, loc: &Localizer) -> Option<String> {
    let arg = ctx(err, ContextKind::InvalidArg);
    let text = match err.kind() {
        ErrorKind::UnknownArgument => loc.message_args("error-unknown-argument", &[("arg", &arg)]),
        ErrorKind::InvalidSubcommand => loc.message_args(
            "error-unknown-subcommand",
            &[("name", &ctx(err, ContextKind::InvalidSubcommand))],
        ),
        ErrorKind::InvalidValue => {
            let mut text = loc.message_args(
                "error-invalid-value",
                &[
                    ("value", &ctx(err, ContextKind::InvalidValue)),
                    ("arg", &arg),
                ],
            );
            let valid = ctx(err, ContextKind::ValidValue);
            if !valid.is_empty() {
                text.push_str("\n  ");
                text.push_str(&loc.message_args("error-possible-values", &[("values", &valid)]));
            }
            text
        }
        ErrorKind::ValueValidation => {
            let source = std::error::Error::source(err);
            let reason = match source.and_then(|s| s.downcast_ref::<ArgError>()) {
                Some(e) => loc.message_args(e.id, &[("detail", &e.detail)]),
                None => source.map(ToString::to_string).unwrap_or_default(),
            };
            loc.message_args(
                "error-value-validation",
                &[
                    ("value", &ctx(err, ContextKind::InvalidValue)),
                    ("arg", &arg),
                    ("reason", &reason),
                ],
            )
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
        ErrorKind::InvalidUtf8 => return None,
        ErrorKind::TooFewValues | ErrorKind::WrongNumberOfValues => {
            loc.message_args("error-missing-value", &[("arg", &arg)])
        }
        _ => return None,
    };
    Some(text)
}

/// Full text of a usage error: message, suggestion, usage and hint. `None` if clap's own text must
/// be used (kinds that are not translated).
#[must_use]
pub fn localize_clap_error(err: &clap::Error, loc: &Localizer) -> Option<String> {
    let mut out = format!("{} {}\n", loc.message("error-prefix"), message(err, loc)?);
    for kind in [ContextKind::SuggestedSubcommand, ContextKind::SuggestedArg] {
        let tip = ctx(err, kind);
        if !tip.is_empty() {
            out.push_str(&format!("\n  {} '{tip}'\n", loc.message("error-tip")));
        }
    }
    let usage = ctx(err, ContextKind::Usage);
    let usage = usage
        .trim()
        .strip_prefix("Usage:")
        .unwrap_or(usage.trim())
        .trim();
    if !usage.is_empty() {
        out.push_str(&format!("\n{} {usage}\n", loc.message("cli-usage-heading")));
    }
    out.push_str(&format!("\n{}\n", loc.message("error-hint")));
    Some(out)
}

/// One-line plain message of a clap error, for the JSON `error.message`.
#[must_use]
pub fn plain_clap_message(err: &clap::Error, loc: &Localizer) -> String {
    message(err, loc).unwrap_or_else(|| {
        let rendered = err.to_string();
        rendered
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_owned()
    })
}
