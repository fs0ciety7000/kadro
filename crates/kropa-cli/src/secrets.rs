// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa secrets status|set|remove|allow` (`docs/cli.md` section 2.4, lot 1.8).
//!
//! Nothing here prints, logs or returns the value of a secret: `status` shows counts, `set` reads a
//! value and hands it to the store, and the messages quote names of profiles and secrets, origins and
//! places, which are not secrets. A value is read only after every confirmation, so that a refused or
//! impossible command has not consumed stdin.
//!
//! The JSON schema of `status` is `kropa.secrets.status/1`. `set`, `remove` and `allow` have no
//! schema in `docs/cli.md`: with `--json` they print nothing on stdout, and the exit code and stderr
//! say how it went.

use std::path::Path;

use kropa_core::ExitCode;
use kropa_i18n::Localizer;
use kropa_upload::UploadError;
use kropa_upload::destination::{Compiled, Destination};
use kropa_upload::origin::canonical_origin;
use kropa_upload::secret::{
    Authorization, EntryInfo, Placement, SHORT_SECRET_LEN, SecretError, SecretValue, StoreState,
    StoreStatus,
};
use serde_json::json;

use crate::args::SecretSource;
use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;
use crate::secret_input::{InputError, InputFrom};
use crate::upload::{cli_error, cli_error_write, load_one};

/// JSON schema of `kropa secrets status`.
pub const STATUS_SCHEMA: &str = "kropa.secrets.status/1";

/// The error of a store call, with the exit code and `error.reason` of `docs/cli.md`.
fn store_error(error: SecretError, loc: &Localizer) -> CliError {
    cli_error(&UploadError::from(error), loc).at_step("secrets")
}

/// Like [`store_error`] for a call that changes the store.
fn store_write_error(error: SecretError, loc: &Localizer) -> CliError {
    cli_error_write(&UploadError::from(error), loc).at_step("secrets")
}

fn cancelled(loc: &Localizer) -> CliError {
    CliError::new(ExitCode::Cancelled, loc, "error-cancelled", &[])
}

fn list_text(items: &[String], loc: &Localizer) -> String {
    if items.is_empty() {
        loc.message("out-secrets-none")
    } else {
        items.join(", ")
    }
}

// ---------------------------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------------------------

fn status_json(status: &StoreStatus) -> serde_json::Value {
    let profiles: Vec<_> = status
        .profiles
        .iter()
        .map(|(profile, count)| {
            json!({"profile": profile, "entries": count.entries, "empty": count.empty})
        })
        .collect();
    // Nothing is switched by itself: the vault is only said to exist, and when.
    let fallback = (status.state == StoreState::Unavailable)
        .then(|| json!({"store": "vault", "lot": "2.8", "switched": false}));
    json!({
        "schema": STATUS_SCHEMA, "status": "ok", "exit_code": 0,
        "store": status.store,
        "state": status.state.as_str(),
        "detail": status.detail,
        "profiles": profiles,
        "fallback": fallback,
    })
}

/// `kropa secrets status`.
///
/// # Errors
/// [`CliError`] (exit 5) when this build has no store; any other state is a report, exit 0.
pub fn status(ctx: &Context<'_>, session: &mut Session<'_, '_>) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let status = ctx.secrets.status();
    if status.state == StoreState::NotDelivered {
        return Err(store_error(SecretError::NotDelivered, loc));
    }
    if session.json {
        session.document(&status_json(&status));
        return Ok(ExitCode::Ok);
    }
    let state = loc.message(&format!("out-secrets-state-{}", status.state.as_str()));
    session.result(&loc.message_args(
        "out-secrets-store",
        &[("store", status.store), ("state", &state)],
    ));
    if let Some(detail) = &status.detail {
        session.result(&loc.message_args("out-secrets-detail", &[("detail", detail)]));
    }
    for (profile, count) in &status.profiles {
        session.result(&loc.message_args(
            "out-secrets-profile",
            &[
                ("profile", profile),
                ("entries", &count.entries.to_string()),
                ("empty", &count.empty.to_string()),
            ],
        ));
    }
    if status.state == StoreState::Unavailable {
        session.result(&loc.message("out-secrets-fallback"));
    }
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// set
// ---------------------------------------------------------------------------------------------

/// The destination of a profile, which must declare the secret it is asked to store.
fn declared(
    config_dir: &Path,
    profile: &str,
    name: &str,
    loc: &Localizer,
) -> Result<(Destination, Compiled), CliError> {
    let destination = load_one(config_dir, profile, loc)?;
    let compiled = destination.compile().map_err(|e| cli_error(&e, loc))?;
    if !compiled.secrets.contains(name) {
        return Err(CliError::new(
            ExitCode::Config,
            loc,
            "error-secret-undeclared",
            &[("profile", profile), ("name", name)],
        )
        .with_reason("invalid_destination")
        .at_step("secrets"));
    }
    Ok((destination, compiled))
}

/// Origins and places a new entry gets: the address of the profile and the places where its
/// templates use the secret (`docs/security/secrets.md` section 4.3).
pub(crate) fn fresh_authorization(
    compiled: &Compiled,
    name: &str,
    loc: &Localizer,
) -> Result<Authorization, CliError> {
    let origin = compiled.origin().ok_or_else(|| {
        CliError::new(ExitCode::Config, loc, "error-secret-no-origin", &[])
            .with_reason("invalid_destination")
            .at_step("secrets")
    })?;
    let mut placements: Vec<Placement> = Vec::new();
    for slot in compiled
        .secret_slots()
        .into_iter()
        .filter(|s| s.name == name)
    {
        if !placements.contains(&slot.placement) {
            placements.push(slot.placement);
        }
    }
    Ok(Authorization {
        origins: vec![origin],
        placements,
    })
}

fn show_authorization(
    origins: &[String],
    placements: &[Placement],
    loc: &Localizer,
    session: &mut Session<'_, '_>,
) {
    let places: Vec<String> = placements.iter().map(ToString::to_string).collect();
    session.note(&loc.message_args(
        "out-secrets-origins",
        &[("origins", &list_text(origins, loc))],
    ));
    session.note(&loc.message_args(
        "out-secrets-placements",
        &[("placements", &list_text(&places, loc))],
    ));
    if placements.iter().any(|p| matches!(p, Placement::Query(_))) {
        session.note(&loc.message("out-secrets-query-warning"));
    }
}

fn read_error(error: InputError, loc: &Localizer) -> CliError {
    match error {
        InputError::TooLong => CliError::new(ExitCode::Usage, loc, "error-secret-too-long", &[]),
        InputError::NotText => CliError::new(ExitCode::Usage, loc, "error-secret-not-text", &[]),
        InputError::Unreadable => CliError::new(ExitCode::Io, loc, "error-secret-unreadable", &[]),
        InputError::NoTerminal => CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-secret-input-required",
            &[],
        ),
    }
}

/// Reads the value of `<profile>/<name>` from the source the user chose (`--secret-stdin`,
/// `--secret-fd`, or the hidden prompt on a terminal), and checks it. Called only once everything
/// else has been confirmed, so that a refused command has not consumed stdin.
///
/// # Errors
/// [`CliError`] without a terminal and without a source (12), for a value that cannot be read (2,
/// 15) or is shorter than 4 bytes (11).
pub(crate) fn read_value(
    profile: &str,
    name: &str,
    source: &SecretSource,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<SecretValue, CliError> {
    let loc = ctx.loc;
    let from = if source.secret_stdin {
        InputFrom::Stdin
    } else if let Some(fd) = source.secret_fd {
        InputFrom::Fd(fd)
    } else if session.interactive {
        InputFrom::Terminal
    } else {
        return Err(CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-secret-input-required",
            &[],
        ));
    };
    let prompt = loc.message_args(
        "prompt-secret-value",
        &[("profile", profile), ("name", name)],
    );
    let text = ctx
        .input
        .read(from, &prompt)
        .map_err(|e| read_error(e, loc))?;
    let value = SecretValue::new(text.as_str()).map_err(|_| {
        cli_error(
            &UploadError::Security {
                reason: "secret_too_short",
                detail: String::new(),
            },
            loc,
        )
        .at_step("secrets")
    })?;
    drop(text);
    if value.len() < SHORT_SECRET_LEN {
        session.note(&loc.message_args("warn-secret-short", &[("detail", name)]));
    }
    Ok(value)
}

/// `kropa secrets set <PROFILE> <NAME>`.
///
/// # Errors
/// [`CliError`] for an unknown profile or an undeclared secret (6), a locked or missing store (7),
/// a confirmation or a value that cannot be asked for (12), a value that cannot be read (2, 15) or
/// is shorter than 4 bytes (11), or a declined confirmation (3).
pub fn set(
    profile: &str,
    name: &str,
    source: &SecretSource,
    yes: bool,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let (_, compiled) = declared(config_dir, profile, name, loc)?;
    let info = ctx
        .secrets
        .inspect(profile, name)
        .map_err(|e| store_error(e, loc))?;
    // A new entry, or one an import left empty (N6): where it may be sent and put is shown and confirmed.
    let fresh = match &info {
        Some(EntryInfo {
            has_value: true, ..
        }) => None,
        Some(existing) => Some((existing.origins.clone(), existing.placements.clone())),
        None => {
            let a = fresh_authorization(&compiled, name, loc)?;
            Some((a.origins, a.placements))
        }
    };
    let mut authorization = Authorization::default();
    if let Some((origins, placements)) = fresh {
        session.note(&loc.message_args(
            "out-secrets-new-entry",
            &[("profile", profile), ("name", name)],
        ));
        show_authorization(&origins, &placements, loc, session);
        if !yes {
            if !session.interactive {
                return Err(CliError::new(
                    ExitCode::InteractionRequired,
                    loc,
                    "error-confirmation-required",
                    &[],
                ));
            }
            let prompt = loc.message_args(
                "prompt-secrets-set",
                &[("profile", profile), ("name", name)],
            );
            if !(ctx.confirm)(&prompt) {
                return Err(cancelled(loc));
            }
        }
        authorization = Authorization {
            origins,
            placements,
        };
    }
    let value = read_value(profile, name, source, ctx, session)?;
    ctx.secrets
        .set(profile, name, value, authorization)
        .map_err(|e| store_write_error(e, loc))?;
    session.note(&loc.message_args("note-secret-set", &[("profile", profile), ("name", name)]));
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------------------------

/// `kropa secrets remove <PROFILE> <NAME>`.
///
/// # Errors
/// [`CliError`] for a store that cannot be written (7) or a declined confirmation (3).
pub fn remove(
    profile: &str,
    name: &str,
    yes: bool,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    if !yes {
        let prompt = loc.message_args(
            "prompt-secrets-remove",
            &[("profile", profile), ("name", name)],
        );
        if !(ctx.confirm)(&prompt) {
            return Err(cancelled(loc));
        }
    }
    let removed = ctx
        .secrets
        .remove(profile, name)
        .map_err(|e| store_write_error(e, loc))?;
    let id = if removed {
        "note-secret-removed"
    } else {
        "note-secret-absent"
    };
    session.note(&loc.message_args(id, &[("profile", profile), ("name", name)]));
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// allow
// ---------------------------------------------------------------------------------------------

fn refused(loc: &Localizer, reason: &'static str, id: &str) -> CliError {
    CliError::new(ExitCode::SecurityRefused, loc, id, &[])
        .with_reason(reason)
        .at_step("secrets")
}

/// The place named by `--placement`: `header:`, `query:`, `form:` or `body`. A host, a path or
/// anything else is refused (`docs/security/secrets.md` section 4.3, decision U2).
fn parse_placement(
    text: &str,
    profile: &str,
    config_dir: &Path,
    loc: &Localizer,
) -> Result<Placement, CliError> {
    if text == "body" {
        // The body place is tied to the template of the profile.
        let destination = load_one(config_dir, profile, loc)?;
        let compiled = destination.compile().map_err(|e| cli_error(&e, loc))?;
        return compiled
            .body_template
            .as_ref()
            .map(|t| Placement::body(t.source()))
            .ok_or_else(|| {
                CliError::new(ExitCode::Config, loc, "error-secrets-no-body", &[])
                    .with_reason("invalid_destination")
                    .at_step("secrets")
            });
    }
    let well_formed = text.split_once(':').is_some_and(|(kind, rest)| {
        !rest.is_empty()
            && match kind {
                "header" | "query" | "form" => true,
                // Only the digest of a template (see `Placement::body`).
                "body" => rest.len() == 64 && rest.bytes().all(|b| b.is_ascii_hexdigit()),
                _ => false,
            }
    });
    match text.parse::<Placement>() {
        Ok(placement) if well_formed => Ok(placement),
        _ => Err(refused(
            loc,
            "placement_not_allowed",
            "error-secrets-bad-placement",
        )),
    }
}

/// `kropa secrets allow <PROFILE> <NAME> (--origin URL | --placement PLACE)`: always asked on a
/// terminal, never confirmed by `--yes` (`docs/cli.md` section 2.4).
///
/// # Errors
/// [`CliError`] without a terminal (12), for an entry that does not exist (7), an origin that is not
/// valid (6), a place that is not allowed (11), or a declined confirmation (3).
pub fn allow(
    profile: &str,
    name: &str,
    origin: Option<&str>,
    placement: Option<&str>,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    if !session.interactive {
        return Err(CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-allow-interactive",
            &[],
        ));
    }
    let new_origin = match origin {
        None => None,
        Some(text) => Some(canonical_origin(text).ok_or_else(|| {
            CliError::new(ExitCode::Config, loc, "error-secrets-bad-origin", &[])
                .with_reason("invalid_origin")
                .at_step("secrets")
        })?),
    };
    let new_placement = placement
        .map(|p| parse_placement(p, profile, config_dir, loc))
        .transpose()?;
    let info = ctx
        .secrets
        .inspect(profile, name)
        .map_err(|e| store_error(e, loc))?
        .ok_or_else(|| store_error(missing(name), loc))?;
    session.note(&loc.message_args(
        "out-secrets-current",
        &[("profile", profile), ("name", name)],
    ));
    show_authorization(&info.origins, &info.placements, loc, session);
    if let Some(origin) = &new_origin {
        session.note(&loc.message_args("out-secrets-add-origin", &[("origin", origin)]));
    }
    if let Some(placement) = &new_placement {
        session.note(&loc.message_args(
            "out-secrets-add-placement",
            &[("placement", &placement.to_string())],
        ));
        if matches!(placement, Placement::Query(_)) {
            session.note(&loc.message("out-secrets-query-warning"));
        }
    }
    let prompt = loc.message_args(
        "prompt-secrets-allow",
        &[("profile", profile), ("name", name)],
    );
    if !(ctx.confirm)(&prompt) {
        return Err(cancelled(loc));
    }
    ctx.secrets
        .authorize(profile, name, new_origin.as_deref(), new_placement)
        .map_err(|e| store_write_error(e, loc))?;
    session.note(&loc.message_args(
        "note-secret-allowed",
        &[("profile", profile), ("name", name)],
    ));
    Ok(ExitCode::Ok)
}

fn missing(name: &str) -> SecretError {
    SecretError::Store {
        reason: "secret_missing",
        detail: name.to_owned(),
    }
}
