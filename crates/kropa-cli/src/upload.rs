// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa upload` and the upload step of `kropa capture --upload`.
//!
//! The file is read, sent by the engine of `kropa-upload` and reported in the JSON schema
//! `kropa.upload/1` (and the `upload`, `link`, `clipboard` and `warnings` members of
//! `kropa.capture/1`). Every text that comes from the server goes out through `CheckedOutput` or
//! `MaskedText`. The clipboard is lot 1.10: `clipboard.status` says so instead of pretending.

use std::path::{Path, PathBuf};

use kropa_core::{Config, ExitCode, config_dir};
use kropa_i18n::Localizer;
use kropa_upload::UploadError;
use kropa_upload::destination::Destination;
use kropa_upload::engine::{UploadInput, UploadOutcome, Uploader};
use kropa_upload::http::block_on;
use kropa_upload::links::{LinkFormat, format_link};
use kropa_upload::masked::{CheckedOutput, MaskedText, SecretSet};
use kropa_upload::profiles::{find, load_destinations};
use serde_json::{Value, json};

use crate::args::{CopyTarget, LinkCopy, UploadArgs};
use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;

/// JSON schema of `kropa upload`.
pub const SCHEMA: &str = "kropa.upload/1";

/// What `--copy` asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyChoice {
    /// The URL.
    Url,
    /// A Markdown link.
    Markdown,
    /// An HTML snippet.
    Html,
    /// Nothing.
    None,
}

impl CopyChoice {
    /// Name in the JSON output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::None => "none",
        }
    }

    fn format(self) -> Option<LinkFormat> {
        match self {
            Self::Url => Some(LinkFormat::Url),
            Self::Markdown => Some(LinkFormat::Markdown),
            Self::Html => Some(LinkFormat::Html),
            Self::None => None,
        }
    }
}

impl From<LinkCopy> for CopyChoice {
    fn from(copy: LinkCopy) -> Self {
        match copy {
            LinkCopy::Url => Self::Url,
            LinkCopy::Markdown => Self::Markdown,
            LinkCopy::Html => Self::Html,
            LinkCopy::None => Self::None,
        }
    }
}

impl CopyChoice {
    /// `--copy image` is the clipboard of lot 1.10 and never gets here; it counts as nothing.
    #[must_use]
    pub fn from_target(copy: CopyTarget) -> Self {
        match copy {
            CopyTarget::Url => Self::Url,
            CopyTarget::Markdown => Self::Markdown,
            CopyTarget::Html => Self::Html,
            CopyTarget::Image | CopyTarget::None => Self::None,
        }
    }
}

/// The configuration folder: `--config`, else the standard one.
///
/// # Errors
/// [`CliError`] (`config`) when there is no home directory.
pub fn config_dir_of(arg: Option<&Path>, loc: &Localizer) -> Result<PathBuf, CliError> {
    match arg {
        Some(dir) => Ok(dir.to_owned()),
        None => config_dir().map_err(|e| CliError::from_core(&e, loc)),
    }
}

/// The command-line error for an upload failure, with its exit code and `error.reason`. Texts from
/// the server arrive here only as `MaskedText`.
#[must_use]
pub fn cli_error(err: &UploadError, loc: &Localizer) -> CliError {
    let reason = err.reason();
    let mut id = format!("error-upload-{}", reason.replace('_', "-"));
    let detail = match err {
        UploadError::SecretMissing { name } => name.clone(),
        UploadError::Variable { detail, .. } => detail.clone(),
        UploadError::Server { message, .. } => {
            if message.as_str().is_empty() {
                id.push_str("-bare");
            }
            message.to_string()
        }
        other => other.to_string(),
    };
    let status = err.status().map(|s| s.to_string()).unwrap_or_default();
    let mut error = CliError::new(
        err.exit_code(),
        loc,
        &id,
        &[("detail", &detail), ("status", &status)],
    )
    .with_reason(reason)
    .at_step("upload");
    if matches!(err, UploadError::SecretStoreNotDelivered) {
        error.lots = vec!["1.8m"];
    }
    error
}

/// Like [`cli_error`] for a command that changes the secret store: a keyring that did not answer
/// may have done the change, which the message says (a read that times out changes nothing).
#[must_use]
pub fn cli_error_write(err: &UploadError, loc: &Localizer) -> CliError {
    let mut error = cli_error(err, loc);
    if error.reason.as_deref() == Some("keyring_timeout") {
        let detail = err.to_string();
        error.message =
            loc.message_args("error-upload-keyring-timeout-write", &[("detail", &detail)]);
    }
    error
}

/// An upload that was tried: the profile, the name of the file, and what came of it.
pub struct Delivery {
    /// Profile name.
    pub profile: String,
    /// Name of the file, used in links.
    pub file_name: String,
    /// The outcome.
    pub result: Result<UploadOutcome, UploadError>,
}

fn send(
    ctx: &Context<'_>,
    config_dir: &Path,
    profile: &str,
    file: &Path,
    vars: &[(String, String)],
) -> Result<UploadOutcome, UploadError> {
    let destinations = load_destinations(config_dir)?;
    let destination = find(&destinations, profile)?;
    let compiled = destination.compile()?;
    let input = UploadInput::from_file(file, compiled.max_size)?;
    let uploader = Uploader::new(ctx.secrets)?;
    block_on(uploader.upload(destination, &input, vars))
}

/// Sends `file` to `profile`.
#[must_use]
pub fn deliver(
    ctx: &Context<'_>,
    config_dir: &Path,
    profile: &str,
    file: &Path,
    vars: &[(String, String)],
) -> Delivery {
    Delivery {
        profile: profile.to_owned(),
        file_name: file
            .file_name()
            .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned()),
        result: send(ctx, config_dir, profile, file, vars),
    }
}

/// What a delivery adds to the JSON documents and to stdout.
pub struct Report {
    /// The `upload` member.
    pub upload: Value,
    /// The `link` member.
    pub link: Value,
    /// The `clipboard` member.
    pub clipboard: Value,
    /// The `warnings` member, as `{code, message}` objects.
    pub warnings: Vec<Value>,
    /// The failure, if the upload did not end well.
    pub error: Option<CliError>,
    /// The URL to print on stdout.
    pub url: Option<CheckedOutput>,
}

fn warning(code: &str, loc: &Localizer, detail: &str) -> Value {
    json!({
        "code": code,
        "message": loc.message_args(&format!("warn-{}", code.replace('_', "-")), &[("detail", detail)]),
    })
}

/// Builds the report of a delivery and writes its messages on stderr. `copy` is what `--copy` asked
/// for (`None`: not given).
pub fn report(
    delivery: &Delivery,
    copy: Option<CopyChoice>,
    loc: &Localizer,
    session: &mut Session<'_, '_>,
) -> Report {
    let requested = copy.unwrap_or(CopyChoice::None);
    let refused;
    let mut link: Option<CheckedOutput> = None;
    let (outcome, error): (Option<&UploadOutcome>, Option<&UploadError>) = match &delivery.result {
        Err(e) => (None, Some(e)),
        Ok(outcome) => {
            let format = requested.format().unwrap_or(LinkFormat::Url);
            if let Ok(text) = format_link(
                format,
                &outcome.url,
                &delivery.file_name,
                &SecretSet::loaded(),
            ) {
                link = Some(text);
                (Some(outcome), None)
            } else {
                refused = UploadError::Security {
                    reason: "secret_in_output",
                    detail: "the link holds a secret".to_owned(),
                };
                (None, Some(&refused))
            }
        }
    };
    let mut warnings = Vec::new();
    if let Some(e) = error {
        if e.server_message().is_some_and(MaskedText::was_masked) {
            warnings.push(warning("secret_echoed", loc, ""));
        }
        if e.is_secret_in_output() {
            warnings.push(warning("uploaded_link_withheld", loc, ""));
        }
    }
    if let Some(outcome) = outcome {
        for w in &outcome.warnings {
            warnings.push(warning(w.code, loc, &w.detail));
        }
        warnings.push(warning("metadata_not_stripped", loc, ""));
    }
    let wants_copy = outcome.is_some() && requested != CopyChoice::None;
    if wants_copy {
        warnings.push(warning("clipboard_unavailable", loc, ""));
    }
    let clipboard = json!({
        "requested": requested.as_str(),
        "explicit": copy.is_some(),
        "status": if wants_copy { "unavailable" } else { "skipped" },
        "reason": if wants_copy {
            Some("clipboard_not_delivered")
        } else if requested != CopyChoice::None {
            Some("nothing_to_copy")
        } else {
            None
        },
    });
    for w in &warnings {
        if let Some(message) = w["message"].as_str() {
            session.note(message);
        }
    }
    let (upload, link_json) = if let (Some(outcome), Some(text)) = (outcome, &link) {
        session.note(&loc.message_args("note-uploaded", &[("profile", &delivery.profile)]));
        if matches!(requested, CopyChoice::Markdown | CopyChoice::Html) {
            session.note(&loc.message_args(
                "note-link",
                &[("format", requested.as_str()), ("text", text.as_str())],
            ));
        }
        (
            json!({
                "status": "done", "destination": delivery.profile, "job_id": null, "error": null,
                "url": outcome.url.as_str(),
                "thumbnail_url": outcome.thumbnail_url.as_ref().map(CheckedOutput::as_str),
                "deletion_url": outcome.deletion_url.as_ref().map(CheckedOutput::as_str),
            }),
            json!({ "format": requested.format().unwrap_or(LinkFormat::Url).as_str(), "text": text.as_str() }),
        )
    } else {
        (
            json!({
                "status": "failed", "destination": delivery.profile, "job_id": null,
                "error": error.map(|e| cli_error(e, loc).message),
                "url": null, "thumbnail_url": null, "deletion_url": null,
            }),
            Value::Null,
        )
    };
    Report {
        upload,
        link: link_json,
        clipboard,
        warnings,
        error: error.map(|e| cli_error(e, loc)),
        url: outcome.map(|o| o.url.clone()),
    }
}

/// The destination to use: the one named, else the default of the configuration.
fn profile_of(named: Option<&str>, config: &Config) -> Option<String> {
    named
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(|| config.default_destination.clone())
}

/// `kropa upload <FILE>`.
///
/// # Errors
/// A [`CliError`] when the command cannot start (no destination at all); an upload failure is
/// reported here, with the JSON document of the command, and gives its exit code.
pub fn run(
    args: &UploadArgs,
    config: &Config,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let profile = profile_of(args.to.as_deref(), config)
        .ok_or_else(|| CliError::new(ExitCode::Config, loc, "error-no-default-destination", &[]))?;
    let delivery = deliver(ctx, config_dir, &profile, &args.file, &args.vars);
    let copy = args.copy.map(CopyChoice::from);
    let report = report(&delivery, copy, loc, session);
    let file = file_member(&args.file);
    if let Some(error) = report.error {
        // The file was not changed. When the answer held the secret, stdout gets the local path.
        let withheld = error.reason.as_deref() == Some("secret_in_output");
        if withheld && !session.json {
            session.result(&args.file.display().to_string());
        }
        let extra = json!({
            "file": file,
            "upload": report.upload,
            "link": report.link,
            "clipboard": report.clipboard,
            "warnings": report.warnings,
        });
        return Ok(session.fail_with(SCHEMA, &error, &extra));
    }
    if session.json {
        session.document(&json!({
            "schema": SCHEMA, "status": "ok", "exit_code": 0,
            "file": file,
            "upload": report.upload,
            "link": report.link,
            "clipboard": report.clipboard,
            "warnings": report.warnings,
            "error": null,
        }));
    } else if let Some(url) = &report.url {
        session.result_checked(url);
    }
    Ok(ExitCode::Ok)
}

/// The `file` member of the upload documents.
fn file_member(path: &Path) -> Value {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let size = std::fs::metadata(path).map(|m| m.len()).ok();
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    json!({
        "path": absolute.display().to_string(),
        "format": format,
        "width": null,
        "height": null,
        "size": size,
    })
}

/// Looks the destination up for the commands that show it.
///
/// # Errors
/// [`CliError`] for an unreadable file or an unknown profile.
pub fn load_one(config_dir: &Path, id: &str, loc: &Localizer) -> Result<Destination, CliError> {
    let destinations = load_destinations(config_dir).map_err(|e| cli_error(&e, loc))?;
    find(&destinations, id)
        .cloned()
        .map_err(|e| cli_error(&e, loc))
}

#[cfg(test)]
mod tests {
    use kropa_i18n::{Lang, Localizer};
    use kropa_upload::UploadError;
    use kropa_upload::error::ALL_REASONS;

    use super::cli_error;

    #[test]
    fn every_reason_has_a_message_in_both_languages() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for reason in ALL_REASONS {
                let id = format!("error-upload-{}", reason.replace('_', "-"));
                assert_ne!(
                    loc.message_args(&id, &[("detail", "d"), ("status", "500")]),
                    id,
                    "{lang:?}: {id}"
                );
            }
            for id in ["error-upload-http-status", "error-upload-http-status-bare"] {
                assert_ne!(
                    loc.message_args(id, &[("detail", "d"), ("status", "500")]),
                    id,
                    "{lang:?}: {id}"
                );
            }
        }
    }

    #[test]
    fn errors_carry_their_code_reason_and_step() {
        let loc = Localizer::new(Lang::En).unwrap();
        let e = cli_error(&UploadError::SecretStoreNotDelivered, &loc);
        assert_eq!(e.code.code(), 5);
        assert_eq!(e.reason.as_deref(), Some("secret_store_not_delivered"));
        assert_eq!(e.step, Some("upload"));
        assert_eq!(e.lots, ["1.8m"]);
        assert!(e.message.contains("lot 1.8m"), "{}", e.message);
    }
}
