// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa dest list|show|test|import` (`docs/cli.md` section 2.3, lot 1.7).
//!
//! Nothing here prints the value of a secret. `import` reads the file as data, shows what it would
//! do, asks (or requires `--yes` and `--expect-host` without a terminal) and only then writes; the
//! secrets it finds go to the secret store (lot 1.8) as new entries, and are taken back out if the
//! profile cannot be written. Where the build has no store (macOS until lot 1.8m) an import that holds
//! a secret stops with a clear message and writes nothing.

use std::io::Read as _;
use std::path::Path;

use kropa_core::{Config, ExitCode};
use kropa_i18n::Localizer;
use kropa_upload::UploadError;
use kropa_upload::destination::{Destination, DestinationType};
use kropa_upload::engine::{UploadInput, Uploader};
use kropa_upload::http::block_on;
use kropa_upload::import::{ImportFormat, ImportPlan, MAX_IMPORT_SIZE, Slot, import};
use kropa_upload::links::{LinkFormat, format_link};
use kropa_upload::masked::SecretSet;
use kropa_upload::profiles::{append_destination, load_destinations};
use kropa_upload::secret::{NewSecret, SecretError};
use serde_json::{Value, json};

use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;
use crate::upload::{cli_error, cli_error_write, link_failure, load_one, show_log};

fn media_names(destination: &Destination) -> Vec<&'static str> {
    destination
        .kinds
        .iter()
        .map(|k| match k {
            kropa_upload::destination::MediaKind::Image => "image",
            kropa_upload::destination::MediaKind::File => "file",
        })
        .collect()
}

fn summary(destination: &Destination, default: Option<&str>) -> Value {
    json!({
        "id": destination.id,
        "type": destination.kind.as_str(),
        "label": destination.label,
        "address": destination.address(),
        "origin": destination.origin(),
        "kinds": media_names(destination),
        "default": default == Some(destination.id.as_str()),
    })
}

/// `kropa dest list`.
///
/// # Errors
/// [`CliError`] when `destinations.toml` cannot be read or is not valid.
pub fn list(
    config: &Config,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let destinations = load_destinations(config_dir).map_err(|e| cli_error(&e, loc))?;
    let default = config.default_destination.as_deref();
    if session.json {
        session.document(&json!({
            "schema": "kropa.dest.list/1", "status": "ok", "exit_code": 0,
            "default": default,
            "destinations": destinations.iter().map(|d| summary(d, default)).collect::<Vec<_>>(),
        }));
        return Ok(ExitCode::Ok);
    }
    if destinations.is_empty() {
        session.note(&loc.message("note-no-destinations"));
    }
    for d in &destinations {
        let marker = if default == Some(d.id.as_str()) {
            format!(" {}", loc.message("out-dest-default-marker"))
        } else {
            String::new()
        };
        session.result(&format!(
            "{}\t{}\t{}{marker}",
            d.id,
            d.kind.as_str(),
            d.origin().unwrap_or_default()
        ));
    }
    Ok(ExitCode::Ok)
}

/// The state of a declared secret, from the store: `set`, `missing`, `unknown` (no store yet) or
/// `unavailable`. Only presence is read; the value is dropped at once.
fn secret_state(ctx: &Context<'_>, profile: &str, name: &str) -> &'static str {
    match ctx.secrets.load(profile, name) {
        Ok(Some(_)) => "set",
        Ok(None) => "missing",
        Err(SecretError::NotDelivered) => "unknown",
        Err(_) => "unavailable",
    }
}

/// `kropa dest show <ID>`.
///
/// # Errors
/// [`CliError`] for an unknown profile.
#[allow(clippy::too_many_lines)]
pub fn show(
    id: &str,
    config: &Config,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let destination = load_one(config_dir, id, loc)?;
    let default = config.default_destination.as_deref();
    // An S3 profile has no request: it is shown by its bucket (nothing is read from the network).
    let s3 = (destination.kind == DestinationType::S3)
        .then(|| destination.s3_settings())
        .transpose()
        .map_err(|e| cli_error(&e, loc))?;
    let spec = if s3.is_some() {
        None
    } else {
        Some(destination.expand().map_err(|e| cli_error(&e, loc))?)
    };
    let secrets: Vec<(String, &'static str)> = destination
        .secrets
        .keys()
        .map(|name| (name.clone(), secret_state(ctx, id, name)))
        .collect();
    if session.json {
        let mut doc = summary(&destination, default);
        if let Some((request, response)) = &spec {
            doc["request"] = json!({
                "method": request.method, "url": request.url, "headers": request.headers,
                "query": request.query, "fields": request.fields, "body": format!("{:?}", request.body).to_lowercase(),
            });
            doc["response"] = json!({
                "url": response.url, "thumbnail_url": response.thumbnail_url,
                "deletion_url": response.deletion_url, "error": response.error,
                "success_status": response.success_status,
            });
        }
        if let Some(s3) = &s3 {
            doc["s3"] = json!({
                "endpoint": s3.endpoint.as_ref().map(ToString::to_string),
                "region": s3.region, "bucket": s3.bucket, "path_style": s3.path_style,
                "prefix": s3.prefix, "public_base_url": s3.public_base_url.as_ref().map(ToString::to_string),
                "presign_seconds": s3.presign.map(|d| d.as_secs()),
                "access_key_id": s3.access_key_id,
            });
        }
        doc["secrets"] = json!(
            secrets
                .iter()
                .map(|(n, s)| json!({"name": n, "state": s}))
                .collect::<Vec<_>>()
        );
        doc["vars"] = json!(destination.vars.keys().collect::<Vec<_>>());
        session.document(&json!({
            "schema": "kropa.dest.show/1", "status": "ok", "exit_code": 0, "destination": doc,
        }));
        return Ok(ExitCode::Ok);
    }
    let say = |session: &mut Session<'_, '_>, id: &str, args: &[(&str, &str)]| {
        session.result(&loc.message_args(id, args));
    };
    say(
        session,
        "out-show-title",
        &[("id", id), ("type", destination.kind.as_str())],
    );
    if let Some(label) = &destination.label {
        say(session, "out-show-label", &[("label", label)]);
    }
    say(
        session,
        "out-show-address",
        &[("address", destination.address().unwrap_or_default())],
    );
    say(
        session,
        "out-show-origin",
        &[("origin", &destination.origin().unwrap_or_default())],
    );
    say(
        session,
        "out-show-kinds",
        &[("kinds", &media_names(&destination).join(", "))],
    );
    if default == Some(id) {
        say(session, "out-show-default", &[]);
    }
    if let Some((request, _)) = &spec {
        say(
            session,
            "out-show-request",
            &[("method", &request.method), ("url", &request.url)],
        );
        for (name, value) in &request.headers {
            say(
                session,
                "out-show-header",
                &[("name", name), ("value", value)],
            );
        }
        for (name, value) in &request.query {
            say(
                session,
                "out-show-query",
                &[("name", name), ("value", value)],
            );
        }
        for (name, value) in &request.fields {
            say(
                session,
                "out-show-field",
                &[("name", name), ("value", value)],
            );
        }
    }
    if let Some(s3) = &s3 {
        say(
            session,
            "out-show-s3",
            &[
                ("bucket", &s3.bucket),
                ("region", &s3.region),
                ("key_id", &s3.access_key_id),
            ],
        );
        let link = s3.public_base_url.as_ref().map_or_else(
            || format!("presign {}s", s3.presign.map_or(0, |d| d.as_secs())),
            ToString::to_string,
        );
        say(session, "out-show-s3-link", &[("link", &link)]);
    }
    for name in destination.vars.keys() {
        say(session, "out-show-var", &[("name", name)]);
    }
    for (name, state) in &secrets {
        let text = loc.message(&format!("out-secret-{state}"));
        let text = if text.starts_with("out-secret-") {
            (*state).to_owned()
        } else {
            text
        };
        say(
            session,
            "out-show-secret",
            &[("name", name), ("state", &text)],
        );
    }
    Ok(ExitCode::Ok)
}

fn cancelled(loc: &Localizer) -> CliError {
    CliError::new(ExitCode::Cancelled, loc, "error-cancelled", &[])
}

/// `kropa dest test <ID>`: one 1x1 image, after a confirmation.
///
/// # Errors
/// [`CliError`] for an unknown profile, a refusal, or the failure of the upload.
pub fn test(
    id: &str,
    yes: bool,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let destination = load_one(config_dir, id, loc)?;
    destination.validate().map_err(|e| cli_error(&e, loc))?;
    if !yes {
        let prompt = loc.message_args(
            "prompt-dest-test",
            &[
                ("id", id),
                ("origin", &destination.origin().unwrap_or_default()),
            ],
        );
        if !(ctx.confirm)(&prompt) {
            return Err(cancelled(loc));
        }
    }
    let (result, log) = match Uploader::new(ctx.secrets) {
        Ok(uploader) => {
            let uploader = uploader.with_debug_log(session.verbose);
            let result = block_on(uploader.upload(&destination, &UploadInput::test_image(), &[]));
            (result, uploader.take_log())
        }
        Err(e) => (Err(e), Vec::new()),
    };
    show_log(&log, session);
    let outcome = result.map_err(|e| cli_error(&e, loc))?;
    let known = SecretSet::loaded();
    let link = format_link(LinkFormat::Url, &outcome.url, "kropa-test.png", &known)
        .map_err(|e| cli_error(&link_failure(e), loc))?;
    let mut warnings: Vec<Value> = outcome
        .warnings
        .iter()
        .map(|w| json!({"code": w.code, "message": loc.message_args(&format!("warn-{}", w.code.replace('_', "-")), &[("detail", &w.detail)])}))
        .collect();
    warnings.push(json!({"code": "metadata_not_stripped", "message": loc.message("warn-metadata-not-stripped")}));
    if session.json {
        session.document(&json!({
            "schema": "kropa.dest.test/1", "status": "ok", "exit_code": 0,
            "destination": id, "http_status": outcome.status,
            "headers": outcome.headers.iter().map(|h| json!({"name": h.name, "value": h.value.as_str()})).collect::<Vec<_>>(),
            "url": outcome.url.as_str(),
            "thumbnail_url": outcome.thumbnail_url.as_ref().map(kropa_upload::masked::CheckedOutput::as_str),
            "deletion_url": outcome.deletion_url.as_ref().map(kropa_upload::masked::CheckedOutput::as_str),
            "link": {"format": "url", "text": link.as_str()},
            "warnings": warnings,
            "error": null,
        }));
        return Ok(ExitCode::Ok);
    }
    for w in &warnings {
        if let Some(m) = w["message"].as_str() {
            session.note(m);
        }
    }
    session.result(&loc.message_args(
        "out-test-status",
        &[("status", &outcome.status.to_string())],
    ));
    for h in &outcome.headers {
        session.result(&loc.message_args(
            "out-test-header",
            &[("name", &h.name), ("value", h.value.as_str())],
        ));
    }
    session.result(&loc.message_args("out-test-url", &[("url", outcome.url.as_str())]));
    session.result(&loc.message_args(
        "out-test-link",
        &[("format", "url"), ("text", link.as_str())],
    ));
    Ok(ExitCode::Ok)
}

fn slot_name(slot: &Slot) -> &str {
    match slot {
        Slot::Header(n) | Slot::Query(n) | Slot::Form(n) => n,
        Slot::Body => "body",
    }
}

fn plan_summary(plan: &ImportPlan, loc: &Localizer, session: &mut Session<'_, '_>) {
    session.note(&loc.message_args(
        "out-import-summary",
        &[("id", &plan.destination.id), ("origin", &plan.origin)],
    ));
    if !plan.https {
        session.note(&loc.message("out-import-http-warning"));
    }
    for s in &plan.secrets {
        let places = std::iter::once((&s.slot, &s.placement))
            .chain(s.also_in.iter().map(|(slot, p)| (slot, p)));
        for (slot, placement) in places {
            session.note(
                &loc.message_args(
                    "out-import-secret",
                    &[
                        ("name", &s.name),
                        (
                            "placement",
                            &format!("{placement}")
                                .split(':')
                                .next()
                                .map_or_else(String::new, |k| format!("{k}:{}", slot_name(slot))),
                        ),
                        ("preview", &s.preview),
                    ],
                ),
            );
        }
    }
    for n in &plan.notes {
        let id = format!("out-import-note-{}", n.code.replace('_', "-"));
        let text = loc.message_args(&id, &[("detail", &n.detail)]);
        if text != id {
            session.note(&text);
        }
    }
}

fn read_import(path: &Path, loc: &Localizer) -> Result<Vec<u8>, CliError> {
    let io = |e: &std::io::Error| {
        CliError::new(
            ExitCode::Io,
            loc,
            "error-dest-no-file",
            &[("detail", &e.to_string())],
        )
    };
    let file = std::fs::File::open(path).map_err(|e| io(&e))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMPORT_SIZE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(&e))?;
    Ok(bytes)
}

/// `kropa dest import <FILE>`.
///
/// # Errors
/// [`CliError`] for a file that cannot be imported, a host that is not the expected one, a
/// declined confirmation, a secret that cannot be stored, or a profile that already exists.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub fn import_command(
    file: &Path,
    as_id: Option<&str>,
    yes: bool,
    expect_host: Option<&str>,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let format = match file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("sxcu") => ImportFormat::Sxcu,
        Some("toml") => ImportFormat::Toml,
        _ => {
            return Err(
                CliError::new(ExitCode::Config, loc, "error-import-file-type", &[])
                    .with_reason("import_unsupported")
                    .at_step("import"),
            );
        }
    };
    let bytes = read_import(file, loc)?;
    let plan = import(&bytes, format, as_id).map_err(|e| {
        let mut error = cli_error(&e, loc);
        error.step = Some("import");
        error
    })?;
    if let Some(expected) = expect_host
        && !plan.host_matches(expected)
    {
        let e = UploadError::Security {
            reason: "import_host_mismatch",
            detail: plan.host.clone(),
        };
        let mut error = cli_error(&e, loc);
        error.step = Some("import");
        return Err(error);
    }
    let id = plan.destination.id.clone();
    let taken = load_destinations(config_dir)
        .map_err(|e| cli_error(&e, loc))?
        .iter()
        .any(|d| d.id == id)
        || ctx
            .secrets
            .profile_exists(&id)
            .map_err(|e| cli_error(&UploadError::from(e), loc))?;
    if taken {
        return Err(
            CliError::new(ExitCode::Config, loc, "error-dest-exists", &[("id", &id)])
                .with_reason("invalid_destination")
                .at_step("import"),
        );
    }
    plan_summary(&plan, loc, session);
    if !yes {
        let prompt = loc.message_args(
            "prompt-dest-import",
            &[("id", &id), ("origin", &plan.origin)],
        );
        if !(ctx.confirm)(&prompt) {
            return Err(cancelled(loc));
        }
    }
    let stored: Vec<Value> = plan
        .secrets
        .iter()
        .map(|s| json!({"name": s.name, "placement": s.placement.to_string(), "origin": s.origin, "has_value": s.value.is_some()}))
        .collect();
    let notes: Vec<Value> = plan
        .notes
        .iter()
        .map(|n| json!({"code": n.code, "detail": n.detail}))
        .collect();
    let origin = plan.origin.clone();
    let destination = plan.destination;
    // The entries and the profile go together: when one step fails, what was stored is removed, so
    // that no orphan entry is left to block the next import of the same identifier (N2).
    let mut created: Vec<String> = Vec::new();
    // What cannot be removed is said, with the names (never a value): a silent rollback would leave
    // the entries that block the next import of this identifier without anyone knowing.
    let rollback = |created: &[String]| -> Option<String> {
        let left: Vec<String> = created
            .iter()
            .filter(|name| ctx.secrets.remove(&id, name).is_err())
            .cloned()
            .collect();
        (!left.is_empty()).then(|| {
            loc.message_args(
                "note-import-rollback-left",
                &[("id", &id), ("names", &left.join(", "))],
            )
        })
    };
    // These warnings are needed to act (what may be stored, what was left): `--quiet` keeps them and
    // `--json` carries them in `error.warnings`.
    let fail = |mut error: CliError, warnings: Vec<String>, session: &mut Session<'_, '_>| {
        for line in &warnings {
            session.warn(line);
        }
        error.step = Some("import");
        error.warnings = warnings;
        error
    };
    for secret in plan.secrets {
        let name = secret.name.clone();
        let entry = NewSecret {
            value: secret.value,
            origins: vec![secret.origin],
            placements: std::iter::once(secret.placement)
                .chain(secret.also_in.into_iter().map(|(_, p)| p))
                .collect(),
        };
        if let Err(e) = ctx.secrets.put(&id, &name, entry) {
            let mut warnings = Vec::new();
            // A keyring that did not answer in time may still have created the entry.
            if matches!(&e, SecretError::Store { reason, .. } if *reason == "keyring_timeout") {
                warnings.push(loc.message_args(
                    "note-import-outcome-unknown",
                    &[("id", &id), ("name", &name)],
                ));
            }
            warnings.extend(rollback(&created));
            return Err(fail(
                cli_error_write(&UploadError::from(e), loc),
                warnings,
                session,
            ));
        }
        created.push(name);
    }
    if let Err(e) = append_destination(config_dir, &destination) {
        let warnings: Vec<String> = rollback(&created).into_iter().collect();
        return Err(fail(cli_error(&e, loc), warnings, session));
    }
    session.note(&loc.message_args("note-imported", &[("id", &id), ("origin", &origin)]));
    if session.json {
        session.document(&json!({
            "schema": "kropa.dest.import/1", "status": "ok", "exit_code": 0,
            "destination": {"id": id, "type": destination.kind.as_str(), "origin": origin},
            "secrets": stored, "notes": notes, "error": null,
        }));
    } else {
        session.result(&id);
    }
    Ok(ExitCode::Ok)
}
