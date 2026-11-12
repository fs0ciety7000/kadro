// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa dest add|edit|remove|set-default|export` (`docs/cli.md` section 2.3, lot 2.6).
//!
//! Nothing here prints, logs or returns the value of a secret. `add` reads the token only after
//! everything else was checked, stores it bound to the origin and the place of the profile, and takes
//! it back out if the profile cannot be written. `edit` never lets a secret follow a new address or
//! place without a confirmation that only a terminal can give (`--yes` does nothing). `remove` deletes
//! the profile and every entry `<ID>/*`. `export` writes references (`{secret:NAME}`) only, and
//! refuses a file where someone left a value.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use kropa_core::{Config, ExitCode, set_default_destination};
use kropa_i18n::Localizer;
use kropa_upload::UploadError;
use kropa_upload::destination::{Destination, DestinationFile, DestinationType, MediaKind};
use kropa_upload::import::{ImportFormat, MAX_IMPORT_SIZE, import};
use kropa_upload::literals::presumed_values;
use kropa_upload::profiles::{
    append_destination, destination_toml, load_destinations, remove_destination,
    replace_destination, replace_destination_text,
};
use kropa_upload::secret::{Placement, SecretError};
use serde_json::json;

use crate::args::{DestAddArgs, DestKind};
use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;
use crate::secrets::{fresh_authorization, read_value};
use crate::upload::{cli_error, cli_error_write, load_one};

/// JSON schema of `dest add`, `edit`, `remove` and `set-default`.
pub const CHANGE_SCHEMA: &str = "kropa.dest.change/1";
/// JSON schema of `dest export`.
pub const EXPORT_SCHEMA: &str = "kropa.dest.export/1";

/// Keys that `dest edit --set` accepts. S3 keys (`prefix`, `public_base_url`, `presign`) come with
/// the S3 profile (lots 2.7a and 2.7b); a profile has no link-format key yet.
const SETTABLE: [&str; 2] = ["label", "kinds"];

fn dest_error(error: &UploadError, loc: &Localizer) -> CliError {
    cli_error(error, loc).at_step("dest")
}

fn dest_write_error(error: &UploadError, loc: &Localizer) -> CliError {
    cli_error_write(error, loc).at_step("dest")
}

fn store_error(error: SecretError, loc: &Localizer) -> CliError {
    dest_error(&UploadError::from(error), loc)
}

fn core_error(error: &kropa_core::CoreError, loc: &Localizer) -> CliError {
    CliError::from_core(error, loc).at_step("dest")
}

fn usage(loc: &Localizer, id: &str, args: &[(&str, &str)]) -> CliError {
    CliError::new(ExitCode::Usage, loc, id, args).at_step("dest")
}

fn cancelled(loc: &Localizer) -> CliError {
    CliError::new(ExitCode::Cancelled, loc, "error-cancelled", &[]).at_step("dest")
}

/// The JSON report of a change: what was done to which profile, the default afterwards, and what
/// happened to each secret (names only).
fn report(
    session: &mut Session<'_, '_>,
    action: &str,
    id: Option<&str>,
    default: Option<&str>,
    secrets: &[(String, &'static str)],
) {
    if session.json {
        session.document(&json!({
            "schema": CHANGE_SCHEMA, "status": "ok", "exit_code": 0,
            "action": action, "id": id, "default": default,
            "secrets": secrets.iter().map(|(n, a)| json!({"name": n, "action": a})).collect::<Vec<_>>(),
            "error": null,
        }));
    } else if let Some(id) = id
        && matches!(action, "add" | "edit")
    {
        session.result(id);
    }
}

// ---------------------------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------------------------

/// `kropa dest add <ID> --type zipline --url URL`.
///
/// # Errors
/// [`CliError`] for a type `add` does not create (2, 5), a missing or misplaced option (2), an
/// address that is not valid (6) or not https (11), an `id` already taken by a profile or an entry of
/// the store (6), a token that cannot be asked for (12), read (2, 15) or is too short (11), or a store
/// or file that cannot be written (7, 15).
#[allow(clippy::too_many_lines)]
pub fn add(
    args: &DestAddArgs,
    config: &Config,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    match args.kind {
        DestKind::Zipline => {}
        DestKind::Http => return Err(usage(loc, "error-dest-add-http", &[])),
        DestKind::S3 | DestKind::Sftp => {
            let lot = if args.kind == DestKind::S3 {
                "2.7"
            } else {
                "4.9"
            };
            let mut error = CliError::new(
                ExitCode::Unsupported,
                loc,
                "error-unsupported-one",
                &[("lots", lot)],
            );
            error.lots = vec![if args.kind == DestKind::S3 {
                "2.7"
            } else {
                "4.9"
            }];
            return Err(error);
        }
    }
    if let Some(option) = args.type_opts.given().first() {
        return Err(usage(
            loc,
            "error-dest-option-type",
            &[("option", option), ("type", "zipline")],
        ));
    }
    let url = args
        .url
        .as_deref()
        .ok_or_else(|| usage(loc, "error-dest-url-required", &[]))?;
    let id = args.id.as_str();
    let destination = Destination::zipline(id, url);
    let compiled = destination.compile().map_err(|e| dest_error(&e, loc))?;
    // The token would be refused on every upload otherwise: say so now, before storing it.
    if compiled.url.scheme() != "https" {
        return Err(dest_error(
            &UploadError::Security {
                reason: "https_required",
                detail: "the profile uses a secret: its address must be https://".to_owned(),
            },
            loc,
        ));
    }
    let taken = load_destinations(config_dir)
        .map_err(|e| dest_error(&e, loc))?
        .iter()
        .any(|d| d.id == id)
        || ctx
            .secrets
            .profile_exists(id)
            .map_err(|e| store_error(e, loc))?;
    if taken {
        return Err(
            CliError::new(ExitCode::Config, loc, "error-dest-exists", &[("id", id)])
                .with_reason("invalid_destination")
                .at_step("dest"),
        );
    }
    let authorization = fresh_authorization(&compiled, "token", loc)?;
    let origin = authorization.origins.first().cloned().unwrap_or_default();
    // The value is read last: a command that was refused has not consumed stdin.
    let value = read_value(id, "token", &args.secret, ctx, session)?;
    ctx.secrets
        .set(id, "token", value, authorization)
        .map_err(|e| dest_write_error(&UploadError::from(e), loc))?;
    if let Err(e) = append_destination(config_dir, &destination) {
        let mut error = dest_error(&e, loc);
        if ctx.secrets.remove(id, "token").is_err() {
            let text = loc.message_args(
                "note-dest-add-rollback-left",
                &[("id", id), ("names", "token")],
            );
            session.warn(&text);
            error.warnings.push(text);
        }
        return Err(error);
    }
    session.note(&loc.message_args("note-secret-set", &[("profile", id), ("name", "token")]));
    session.note(&loc.message_args("note-dest-added", &[("id", id), ("origin", &origin)]));
    let mut default = config.default_destination.as_deref();
    if args.make_default {
        if let Err(e) = set_default_destination(config_dir, Some(id)) {
            let mut error = core_error(&e, loc);
            let text = loc.message_args(
                "note-dest-add-default-failed",
                &[("id", id), ("detail", &e.to_string())],
            );
            session.warn(&text);
            error.warnings.push(text);
            return Err(error);
        }
        default = Some(id);
        session.note(&loc.message_args("note-dest-default-set", &[("id", id)]));
    }
    report(
        session,
        "add",
        Some(id),
        default,
        &[("token".to_owned(), "created")],
    );
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// edit
// ---------------------------------------------------------------------------------------------

fn parse_kinds(value: &str) -> Option<Vec<MediaKind>> {
    let mut kinds = Vec::new();
    for part in value.split(',') {
        let kind = match part.trim() {
            "image" => MediaKind::Image,
            "file" => MediaKind::File,
            _ => return None,
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    (!kinds.is_empty()).then_some(kinds)
}

/// Applies one `--set KEY=VALUE`. The key is never repeated in a message unless it is one of ours.
fn apply_set(
    destination: &mut Destination,
    key: &str,
    value: &str,
    loc: &Localizer,
) -> Result<(), CliError> {
    let bad = |key: &str, detail: &str| {
        usage(
            loc,
            "error-dest-edit-value",
            &[("key", key), ("detail", detail)],
        )
    };
    match key {
        "label" => {
            if value.is_empty() {
                destination.label = None;
            } else if value.chars().count() <= 80 && !value.chars().any(char::is_control) {
                destination.label = Some(value.to_owned());
            } else {
                return Err(bad("label", &loc.message("error-dest-edit-label")));
            }
        }
        "kinds" => {
            destination.kinds = parse_kinds(value)
                .ok_or_else(|| bad("kinds", &loc.message("error-dest-edit-kinds")))?;
        }
        _ => {
            return Err(usage(
                loc,
                "error-dest-edit-key",
                &[("keys", &SETTABLE.join(", ")), ("id", &destination.id)],
            ));
        }
    }
    Ok(())
}

/// Where a profile sends its secrets: the origin, and the places of each declared secret.
struct Reach {
    origin: Option<String>,
    places: Vec<(String, Vec<Placement>)>,
}

fn reach(destination: &Destination, loc: &Localizer) -> Result<Reach, CliError> {
    let compiled = destination.compile().map_err(|e| dest_error(&e, loc))?;
    let slots = compiled.secret_slots();
    let places = compiled
        .secrets
        .iter()
        .map(|name| {
            let mut places: Vec<Placement> = Vec::new();
            for slot in slots.iter().filter(|s| &s.name == name) {
                if !places.contains(&slot.placement) {
                    places.push(slot.placement.clone());
                }
            }
            (name.clone(), places)
        })
        .collect();
    Ok(Reach {
        origin: compiled.origin(),
        places,
    })
}

fn same_places(a: &[Placement], b: &[Placement]) -> bool {
    a.len() == b.len() && a.iter().all(|p| b.contains(p))
}

fn places_text(places: &[Placement], loc: &Localizer) -> String {
    if places.is_empty() {
        loc.message("out-secrets-none")
    } else {
        places
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What the store allows an entry, and what a change of the profile would add to it or take out.
/// Everything is computed from the store (`inspect`), never from the file: the file is not trusted
/// (`docs/security/secrets.md` section 4.3), it may have been changed by someone else.
struct Plan {
    name: String,
    store_origins: Vec<String>,
    store_places: Vec<Placement>,
    add_origin: Option<String>,
    add_places: Vec<Placement>,
    drop_origins: Vec<String>,
    drop_places: Vec<Placement>,
}

impl Plan {
    fn adds(&self) -> bool {
        self.add_origin.is_some() || !self.add_places.is_empty()
    }

    fn drops(&self) -> bool {
        !self.drop_origins.is_empty() || !self.drop_places.is_empty()
    }
}

fn origins_text(origins: &[String], loc: &Localizer) -> String {
    if origins.is_empty() {
        loc.message("out-secrets-none")
    } else {
        origins.join(", ")
    }
}

/// One plan per declared secret that has an entry in the store. The set to add is the origin and the
/// places of the profile after the change, minus what the entry allows already. The set to take out
/// (offered in the same question) is what the entry allows and the profile no longer uses, and only
/// where the request moved the address or the place.
fn plan_secrets(
    ctx: &Context<'_>,
    id: &str,
    after: &Reach,
    moved_origin: bool,
    moved_places: &[&(String, Vec<Placement>)],
    strict: bool,
) -> Result<Vec<Plan>, CliError> {
    let mut plans = Vec::new();
    for (name, places) in &after.places {
        let info = match ctx.secrets.inspect(id, name) {
            Ok(Some(info)) => info,
            Ok(None) => continue,
            // A change that moves nothing only looks at the store to warn: an unreadable store is no
            // reason to refuse a new label.
            Err(_) if !strict => continue,
            Err(e) => return Err(store_error(e, ctx.loc)),
        };
        let add_origin = after
            .origin
            .as_ref()
            .filter(|o| !info.origins.contains(o))
            .cloned();
        let add_places: Vec<Placement> = places
            .iter()
            .filter(|p| !info.placements.contains(p))
            .cloned()
            .collect();
        let drop_origins = match &after.origin {
            Some(origin) if moved_origin => info
                .origins
                .iter()
                .filter(|o| *o != origin)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        let drop_places = if !places.is_empty() && moved_places.iter().any(|(n, _)| n == name) {
            info.placements
                .iter()
                .filter(|p| !places.contains(p))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        plans.push(Plan {
            name: name.clone(),
            store_origins: info.origins,
            store_places: info.placements,
            add_origin,
            add_places,
            drop_origins,
            drop_places,
        });
    }
    Ok(plans)
}

/// Shows, as warnings that `--quiet` does not hide, exactly what the answer would change.
fn show_plans(plans: &[Plan], loc: &Localizer, session: &mut Session<'_, '_>) {
    for plan in plans {
        if let Some(origin) = &plan.add_origin {
            session.warn(&loc.message_args(
                "out-edit-origin-add",
                &[
                    ("name", &plan.name),
                    ("old", &origins_text(&plan.store_origins, loc)),
                    ("new", origin),
                ],
            ));
        }
        if !plan.add_places.is_empty() {
            session.warn(&loc.message_args(
                "out-edit-placement-add",
                &[
                    ("name", &plan.name),
                    ("old", &places_text(&plan.store_places, loc)),
                    ("new", &places_text(&plan.add_places, loc)),
                ],
            ));
            if plan
                .add_places
                .iter()
                .any(|p| matches!(p, Placement::Query(_)))
            {
                session.warn(&loc.message("out-secrets-query-warning"));
            }
        }
        if plan.drops() {
            let gone: Vec<String> = plan
                .drop_origins
                .iter()
                .cloned()
                .chain(plan.drop_places.iter().map(ToString::to_string))
                .collect();
            session.warn(&loc.message_args(
                "out-edit-withdraw",
                &[("name", &plan.name), ("gone", &gone.join(", "))],
            ));
        }
    }
}

/// Adds what the plan says to the entry, then takes out what it says.
fn apply_plan(ctx: &Context<'_>, id: &str, plan: &Plan) -> Result<(), SecretError> {
    if let Some(origin) = &plan.add_origin {
        ctx.secrets.authorize(id, &plan.name, Some(origin), None)?;
    }
    for place in &plan.add_places {
        ctx.secrets
            .authorize(id, &plan.name, None, Some(place.clone()))?;
    }
    if plan.drops() {
        ctx.secrets
            .revoke(id, &plan.name, &plan.drop_origins, &plan.drop_places)?;
    }
    Ok(())
}

/// Writes the change. When the request moved the address or a place, the secrets of the profile may
/// follow: what is added is the profile after the change minus what the store allows already, shown
/// in full and asked on a terminal, never given by `--yes`; the old addresses and places the profile
/// no longer uses are offered for withdrawal in the same question. A declined question changes
/// nothing. When the file differs from the store without the request touching an address or a place,
/// nothing is allowed and the user is told so. Returns what happened to each declared secret.
fn apply_change(
    old: &Destination,
    new: &Destination,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
    write: impl FnOnce() -> Result<(), UploadError>,
) -> Result<Vec<(String, &'static str)>, CliError> {
    let loc = ctx.loc;
    let id = old.id.as_str();
    let (before, after) = (reach(old, loc)?, reach(new, loc)?);
    let moved_origin = before.origin != after.origin;
    let moved_places: Vec<&(String, Vec<Placement>)> = after
        .places
        .iter()
        .filter(|(name, places)| {
            before
                .places
                .iter()
                .find(|(n, _)| n == name)
                .is_none_or(|(_, old_places)| !same_places(old_places, places))
        })
        .collect();
    let touches = moved_origin || !moved_places.is_empty();
    let interactive_required = || {
        CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-dest-edit-interactive",
            &[("id", id)],
        )
        .at_step("dest")
    };
    // A profile with secrets whose address or place moves is a question only a terminal can answer,
    // whatever the store holds or can say: without one, nothing is read and nothing is written.
    if touches && (!old.secrets.is_empty() || !after.places.is_empty()) && !session.interactive {
        return Err(interactive_required());
    }
    // On a terminal an unreadable store is an error (7) and nothing is written; only a change that
    // moves nothing may go on without it.
    let mut plans = plan_secrets(ctx, id, &after, moved_origin, &moved_places, touches)?;
    if !touches {
        // The request leaves the address and the places alone: whatever the store lacks is a
        // difference between the file and the store, not something this command may allow.
        let differs: Vec<&str> = plans
            .iter()
            .filter(|p| p.adds())
            .map(|p| p.name.as_str())
            .collect();
        if !differs.is_empty() {
            session.warn(&loc.message_args(
                "note-dest-edit-store-differs",
                &[("id", id), ("names", &differs.join(", "))],
            ));
        }
        plans.clear();
    }
    let follows = plans.iter().any(|p| p.adds() || p.drops());
    if follows {
        show_plans(&plans, loc, session);
        if !(ctx.confirm)(&loc.message_args("prompt-dest-edit", &[("id", id)])) {
            return Err(cancelled(loc));
        }
    }
    write().map_err(|e| dest_write_error(&e, loc))?;
    let mut secrets: Vec<(String, &'static str)> = Vec::new();
    let mut failures: Vec<(String, UploadError)> = Vec::new();
    for (name, _) in &after.places {
        let mut action = "unchanged";
        if let Some(plan) = plans.iter().find(|p| &p.name == name)
            && (plan.adds() || plan.drops())
        {
            match apply_plan(ctx, id, plan) {
                Ok(()) => action = "authorized",
                Err(e) => {
                    action = "failed";
                    failures.push((name.clone(), UploadError::from(e)));
                }
            }
        }
        secrets.push((name.clone(), action));
    }
    if let Some((_, first)) = failures.first() {
        let mut error = dest_write_error(first, loc);
        for (name, e) in &failures {
            let text = loc.message_args(
                "note-dest-edit-authorize-failed",
                &[("id", id), ("name", name), ("detail", &e.to_string())],
            );
            session.warn(&text);
            error.warnings.push(text);
        }
        return Err(error);
    }
    if secrets.iter().any(|(_, a)| *a == "authorized") {
        session.note(&loc.message_args("note-dest-edit-followed", &[("id", id)]));
    }
    Ok(secrets)
}

/// `kropa dest edit <ID> [--url URL] [--set KEY=VALUE]...`; without option, the profile opens in
/// `$EDITOR`.
///
/// # Errors
/// [`CliError`] for an unknown profile or a result that is not valid (6), a key or a value that
/// cannot be set (2), a secret in `--set` or in the edited file, or an address that is not https (11),
/// a question that needs a terminal (12) or is declined (3), an editor that fails (15, 3).
pub fn edit(
    id: &str,
    url: Option<&str>,
    sets: &[(String, String)],
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let old = load_one(config_dir, id, loc).map_err(|e| e.at_step("dest"))?;
    let secrets = if url.is_none() && sets.is_empty() {
        let Some(secrets) = edit_in_editor(&old, config_dir, ctx, session)? else {
            session.note(&loc.message_args("note-dest-edit-unchanged", &[("id", id)]));
            report(
                session,
                "edit",
                Some(id),
                default_of(config_dir).as_deref(),
                &[],
            );
            return Ok(ExitCode::Ok);
        };
        secrets
    } else {
        let mut new = old.clone();
        if let Some(url) = url {
            match new.kind {
                DestinationType::Zipline => new.url = Some(url.to_owned()),
                DestinationType::Http => {
                    if let Some(request) = &mut new.request {
                        url.clone_into(&mut request.url);
                    }
                }
            }
        }
        for (key, value) in sets {
            apply_set(&mut new, key, value, loc)?;
        }
        require_https(&new, loc)?;
        if new == old {
            session.note(&loc.message_args("note-dest-edit-unchanged", &[("id", id)]));
            report(
                session,
                "edit",
                Some(id),
                default_of(config_dir).as_deref(),
                &[],
            );
            return Ok(ExitCode::Ok);
        }
        apply_change(&old, &new, ctx, session, || {
            replace_destination(config_dir, &new)
        })?
    };
    session.note(&loc.message_args("note-dest-edited", &[("id", id)]));
    report(
        session,
        "edit",
        Some(id),
        default_of(config_dir).as_deref(),
        &secrets,
    );
    Ok(ExitCode::Ok)
}

fn default_of(config_dir: &Path) -> Option<String> {
    Config::load_from(Some(config_dir))
        .ok()
        .and_then(|c| c.default_destination)
}

/// A profile that uses a secret is only ever reached over https.
fn require_https(destination: &Destination, loc: &Localizer) -> Result<(), CliError> {
    let compiled = destination.compile().map_err(|e| dest_error(&e, loc))?;
    if !compiled.secret_slots().is_empty() && compiled.url.scheme() != "https" {
        return Err(dest_error(
            &UploadError::Security {
                reason: "https_required",
                detail: "the profile uses secrets: its address must be https://".to_owned(),
            },
            loc,
        ));
    }
    Ok(())
}

/// A directory for the editor, private to the user (`0700`, a random name) and the file inside it
/// (`0600`). The whole directory is removed when this value goes away: the editor leaves its own
/// swap and backup files next to the file.
struct EditFile {
    dir: PathBuf,
    path: PathBuf,
}

/// 12 random bytes, in hexadecimal.
fn random_name() -> std::io::Result<String> {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 12];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().fold(String::new(), |mut name, b| {
        let _ = write!(name, "{b:02x}");
        name
    }))
}

fn make_private_dir(path: &Path, recursive: bool) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
}

impl EditFile {
    /// Under `$XDG_RUNTIME_DIR/kropa` when there is one (it is private to the user and goes away at
    /// logout, whatever happened), else in `fallback` (the configuration directory).
    fn create(
        scratch: Option<&Path>,
        fallback: &Path,
        id: &str,
        text: &str,
    ) -> std::io::Result<Self> {
        use std::io::Write as _;
        let parent = match scratch.filter(|p| p.is_absolute() && p.is_dir()) {
            Some(runtime) => {
                let parent = runtime.join("kropa");
                match make_private_dir(&parent, true) {
                    Ok(()) => parent,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => parent,
                    Err(_) => fallback.to_owned(),
                }
            }
            None => fallback.to_owned(),
        };
        let mut attempts = 0;
        let dir = loop {
            let dir = parent.join(format!(".edit-{}", random_name()?));
            match make_private_dir(&dir, false) {
                Ok(()) => break dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempts < 3 => {
                    attempts += 1;
                }
                Err(e) => return Err(e),
            }
        };
        // From here the directory is removed by `Drop`, whatever fails.
        let guard = Self {
            path: dir.join(format!("{id}.toml")),
            dir,
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&guard.path)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        Ok(guard)
    }

    fn read(&self) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        std::fs::File::open(&self.path)?
            .take(MAX_IMPORT_SIZE as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Removes the directory now (it is not needed any more). `Drop` does it again, harmlessly.
    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for EditFile {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Signals that would kill `kropa` without a `Drop` (Ctrl-C, Ctrl-\, a closed terminal) are held back
/// while the editor runs, so that its private directory is removed. The editor, which shares the
/// terminal, gets them too and deals with them as it likes; if one arrived, [`Self::raised`] says so
/// and the command is cancelled. When the guard goes away the signals act as usual again.
struct EditorSignals {
    #[cfg(unix)]
    raised: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(unix)]
impl EditorSignals {
    fn new() -> std::io::Result<Self> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        let (raised, armed) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        for signal in [SIGINT, SIGQUIT, SIGHUP] {
            // Once `armed`, the default action runs again; before, the signal is only noted.
            signal_hook::flag::register_conditional_default(signal, Arc::clone(&armed))?;
            signal_hook::flag::register(signal, Arc::clone(&raised))?;
        }
        Ok(Self { raised, armed })
    }

    fn raised(&self) -> bool {
        self.raised.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Gives the signals back their usual effect.
    fn arm(&self) {
        self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(unix)]
impl Drop for EditorSignals {
    fn drop(&mut self) {
        self.arm();
    }
}

#[cfg(not(unix))]
impl EditorSignals {
    fn new() -> std::io::Result<Self> {
        Ok(Self {})
    }

    fn raised(&self) -> bool {
        false
    }

    fn arm(&self) {}
}

/// Checks the text a user made of the profile, with the rules of an import. Returns the profile it
/// describes. A value that an import would presume secret is refused (11) only where the edit added
/// or changed it with respect to `old`: what was in the file before is not the edit's doing.
fn check_edited(bytes: &[u8], old: &Destination, loc: &Localizer) -> Result<Destination, CliError> {
    let id = old.id.as_str();
    let invalid = |detail: &str| {
        CliError::new(
            ExitCode::Config,
            loc,
            "error-dest-edit-invalid",
            &[("detail", detail)],
        )
        .with_reason("invalid_destination")
        .at_step("dest")
    };
    if bytes.len() > MAX_IMPORT_SIZE {
        return Err(invalid("larger than 64 KiB"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8 text"))?;
    let mut file =
        DestinationFile::parse(text, "the edited profile").map_err(|e| invalid(&e.to_string()))?;
    if file.destination.len() != 1 || file.destination[0].id != id {
        return Err(invalid(
            "exactly one destination, with the same id (its secrets are filed under it)",
        ));
    }
    let edited = file.destination.remove(0);
    // The shared detector (`kropa_upload::literals`) first: a literal is a refusal, not a syntax
    // error to fix in a second round.
    if !presumed_values(&edited, Some(old)).is_empty() {
        return Err(CliError::new(
            ExitCode::SecurityRefused,
            loc,
            "error-dest-edit-literal-secret",
            &[("id", id)],
        )
        .with_reason("secret_in_profile")
        .at_step("dest"));
    }
    match import(bytes, ImportFormat::Toml, Some(id)) {
        // Only a profile with a real secret needs https, and `require_https` checks that.
        Ok(_)
        | Err(UploadError::Security {
            reason: "https_required",
            ..
        }) => {}
        Err(e @ UploadError::Config { .. }) => return Err(invalid(&e.to_string())),
        Err(other) => return Err(dest_error(&other, loc)),
    }
    Ok(edited)
}

fn run_editor(editor: &str, path: &Path, loc: &Localizer) -> Result<(), CliError> {
    let mut words = editor.split_whitespace();
    let program = words.next().unwrap_or_default();
    let status = std::process::Command::new(program)
        .args(words)
        .arg(path)
        .status()
        .map_err(|e| {
            CliError::new(
                ExitCode::Io,
                loc,
                "error-dest-edit-editor-start",
                &[("detail", &e.kind().to_string())],
            )
            .at_step("dest")
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new(
            ExitCode::Cancelled,
            loc,
            "error-dest-edit-editor-status",
            &[],
        )
        .at_step("dest"))
    }
}

/// Opens the profile in the editor on a private copy, reads it back, validates it, and replaces the
/// profile in one step. `None`: the text did not change. The old file stays whenever the result is
/// not valid or the user does not go on.
fn edit_in_editor(
    old: &Destination,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<Option<Vec<(String, &'static str)>>, CliError> {
    let loc = ctx.loc;
    let id = old.id.as_str();
    if !session.interactive {
        return Err(CliError::new(
            ExitCode::InteractionRequired,
            loc,
            "error-dest-edit-editor-interactive",
            &[],
        )
        .at_step("dest"));
    }
    let editor = ctx.editor.as_deref().ok_or_else(|| {
        CliError::new(ExitCode::Config, loc, "error-dest-edit-no-editor", &[]).at_step("dest")
    })?;
    let original = destination_toml(config_dir, id).map_err(|e| dest_error(&e, loc))?;
    let io = |e: &std::io::Error| {
        CliError::new(
            ExitCode::Io,
            loc,
            "error-dest-edit-editor-start",
            &[("detail", &e.kind().to_string())],
        )
        .at_step("dest")
    };
    // Declared before the file: dropped after it, so the file is gone when the signals act again.
    let signals = EditorSignals::new().map_err(|e| io(&e))?;
    let file = EditFile::create(ctx.scratch_dir.as_deref(), config_dir, id, &original)
        .map_err(|e| io(&e))?;
    loop {
        let ran = run_editor(editor, &file.path, loc);
        if signals.raised() {
            return Err(cancelled(loc));
        }
        ran?;
        let bytes = file.read().map_err(|e| io(&e))?;
        if bytes == original.as_bytes() {
            return Ok(None);
        }
        match check_edited(&bytes, old, loc) {
            Ok(new) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                // The copy is not needed any more: gone before the question, and the signals act as
                // usual again.
                file.remove();
                signals.arm();
                require_https(&new, loc)?;
                let secrets = apply_change(old, &new, ctx, session, || {
                    replace_destination_text(config_dir, id, &text)
                })?;
                return Ok(Some(secrets));
            }
            Err(error) if error.code == ExitCode::Config => {
                session.warn(&error.message);
                if !(ctx.confirm)(&loc.message("prompt-dest-edit-again")) {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------------------------

/// `kropa dest remove <ID> [--yes]`: the profile, every entry `<ID>/*` of the store, and the default
/// if it was this one. Entries that were left by a removed profile (N2) can be removed the same way.
///
/// # Errors
/// [`CliError`] for a name that is neither a profile nor in the store (6), a declined confirmation
/// (3), or a store or file that cannot be written (7, 15).
pub fn remove(
    id: &str,
    yes: bool,
    config: &Config,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let in_config = load_destinations(config_dir)
        .map_err(|e| dest_error(&e, loc))?
        .iter()
        .any(|d| d.id == id);
    let names = ctx.secrets.names(id).map_err(|e| store_error(e, loc))?;
    if !in_config && names.is_empty() {
        return Err(CliError::new(
            ExitCode::Config,
            loc,
            "error-dest-nothing-to-remove",
            &[("id", id)],
        )
        .with_reason("unknown_destination")
        .at_step("dest"));
    }
    if !yes {
        let prompt = loc.message_args(
            "prompt-dest-remove",
            &[("id", id), ("count", &names.len().to_string())],
        );
        if !(ctx.confirm)(&prompt) {
            return Err(cancelled(loc));
        }
    }
    // The entries go first: a failure leaves the profile, and the command can be run again.
    let mut secrets = Vec::new();
    for name in &names {
        ctx.secrets
            .remove(id, name)
            .map_err(|e| dest_write_error(&UploadError::from(e), loc))?;
        secrets.push((name.clone(), "removed"));
    }
    if in_config {
        remove_destination(config_dir, id).map_err(|e| dest_write_error(&e, loc))?;
        session.note(&loc.message_args("note-dest-removed", &[("id", id)]));
    } else {
        session.note(&loc.message_args("note-dest-removed-orphans", &[("id", id)]));
    }
    let mut default = config.default_destination.as_deref();
    if default == Some(id) {
        set_default_destination(config_dir, None).map_err(|e| core_error(&e, loc))?;
        default = None;
        session.note(&loc.message_args("note-dest-remove-default-cleared", &[("id", id)]));
    }
    report(session, "remove", Some(id), default, &secrets);
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// set-default
// ---------------------------------------------------------------------------------------------

/// `kropa dest set-default <ID> | --none`.
///
/// # Errors
/// [`CliError`] for a profile that does not exist (6), or a configuration that cannot be read or
/// written (6, 15).
pub fn set_default(
    id: Option<&str>,
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    if let Some(id) = id {
        load_one(config_dir, id, loc).map_err(|e| e.at_step("dest"))?;
    }
    set_default_destination(config_dir, id).map_err(|e| core_error(&e, loc))?;
    match id {
        Some(id) => session.note(&loc.message_args("note-dest-default-set", &[("id", id)])),
        None => session.note(&loc.message("note-dest-default-cleared")),
    }
    report(session, "set-default", id, id, &[]);
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------------------------

/// `kropa dest export [<ID>...]`: the profiles as TOML on stdout, `{secret:NAME}` references only.
///
/// # Errors
/// [`CliError`] for an unknown profile (6) or a file that holds what looks like a secret value (11).
pub fn export(
    ids: &[String],
    config_dir: &Path,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let all = load_destinations(config_dir).map_err(|e| dest_error(&e, loc))?;
    let mut chosen: Vec<&Destination> = Vec::new();
    if ids.is_empty() {
        chosen.extend(&all);
    } else {
        for id in ids {
            let found = kropa_upload::profiles::find(&all, id).map_err(|e| dest_error(&e, loc))?;
            if !chosen.iter().any(|d| d.id == found.id) {
                chosen.push(found);
            }
        }
    }
    let mut text = String::new();
    for destination in &chosen {
        if !presumed_values(destination, None).is_empty() {
            return Err(CliError::new(
                ExitCode::SecurityRefused,
                loc,
                "error-dest-export-literal-secret",
                &[("id", &destination.id)],
            )
            .with_reason("secret_in_profile")
            .at_step("dest"));
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&destination.to_toml().map_err(|e| dest_error(&e, loc))?);
    }
    if session.json {
        session.document(&json!({
            "schema": EXPORT_SCHEMA, "status": "ok", "exit_code": 0,
            "destinations": chosen.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            "toml": text,
            "error": null,
        }));
    } else if !text.is_empty() {
        session.result(text.trim_end());
    }
    Ok(ExitCode::Ok)
}
