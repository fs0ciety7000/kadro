// SPDX-License-Identifier: GPL-3.0-or-later

//! Import of `.sxcu` (ShareX) and `.toml` destination files (`docs/uploads.md` section 2.4,
//! `docs/security/secrets.md` section 5).
//!
//! A file is read as **data**: at most 64 KiB, nothing executed, no request made. Both formats follow
//! the same rules, a received TOML being no more trustworthy than a `.sxcu`:
//!
//! - every value of a header, a query parameter, a field or a body is presumed to be a secret, except
//!   a white list (`Accept`, `Content-Type`, `User-Agent`, `x-zipline-*`, pure templates such as
//!   `{filename}`, values too short to be a secret). The profile keeps `{secret:name}` and the plan
//!   hands the values over for the store (lot 1.8), each tied to the origin of the address and to the
//!   place it was found in;
//! - a `{secret:x}` in a received TOML is an **empty** entry to fill, never a reference to an entry
//!   that already exists;
//! - `{inputbox:}` in a place of credentials becomes a secret, elsewhere a variable (`--var`);
//! - a token in the path of the address, a user and password in the address, and `http://` with a
//!   secret are refused.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::destination::{
    BodyKind, Destination, DestinationFile, DestinationType, MediaKind, Redirects, RequestSpec,
    ResponseSpec, SecretDecl, VarDecl, is_identifier,
};
use crate::error::UploadError;
use crate::literals::presumed_value;
use crate::origin::origin_of;
use crate::secret::{MIN_SECRET_LEN, Placement, SecretValue};
use crate::sensitive::{
    is_public_header, is_sensitive_name, looks_like_token_segment, split_scheme,
};
use crate::template::{Side, Template};

/// Largest file imported.
pub const MAX_IMPORT_SIZE: usize = 64 * 1024;

/// Format of a file to import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// A ShareX custom uploader.
    Sxcu,
    /// A `destinations.toml` extract.
    Toml,
}

/// Where in the request a value was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// A header.
    Header(String),
    /// A query parameter.
    Query(String),
    /// A multipart or urlencoded field.
    Form(String),
    /// The body template.
    Body,
}

/// A secret found in an imported file.
#[derive(Debug)]
pub struct ImportedSecret {
    /// Name in the profile (`{secret:name}`).
    pub name: String,
    /// Where the profile uses it.
    pub slot: Slot,
    /// Place the secret will be allowed in.
    pub placement: Placement,
    /// Canonical origin it will be tied to.
    pub origin: String,
    /// The value found in the file, `None` for an empty entry to fill (`{secret:x}`, `{inputbox:}`).
    pub value: Option<SecretValue>,
    /// What to show: at most the last 4 characters of a long value, else a mask.
    pub preview: String,
}

/// Something the user should know before accepting the import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportNote {
    /// Machine code: `text_types_ignored`, `legacy_syntax`, `query_secret`, `body_template_secret`.
    pub code: &'static str,
    /// Names of fields or secrets; never a value.
    pub detail: String,
}

/// What an import would do, to show and to confirm.
#[derive(Debug)]
pub struct ImportPlan {
    /// The profile to create; it holds names of secrets only.
    pub destination: Destination,
    /// Secrets to store, in the order they appear in the profile.
    pub secrets: Vec<ImportedSecret>,
    /// Notes for the summary.
    pub notes: Vec<ImportNote>,
    /// Canonical origin of the request.
    pub origin: String,
    /// Host of the request, lower case, without port.
    pub host: String,
    /// Whether the request is `https`.
    pub https: bool,
}

impl ImportPlan {
    /// Whether `expected` (`--expect-host`) is the host of the request, or `host:port`.
    #[must_use]
    pub fn host_matches(&self, expected: &str) -> bool {
        let expected = expected.trim().trim_end_matches('.').to_ascii_lowercase();
        if expected.is_empty() {
            return false;
        }
        let with_port = self
            .origin
            .split_once("://")
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or_default();
        expected == self.host || expected == with_port
    }

    /// Whether any secret has a value to store or an entry to fill.
    #[must_use]
    pub fn needs_secrets(&self) -> bool {
        !self.secrets.is_empty()
    }

    /// Takes a secret back out: the user says the value is not a secret. The value returns to its
    /// place in the profile.
    ///
    /// # Errors
    /// [`UploadError::Config`] if there is no such secret or it has no value to put back.
    pub fn mark_public(&mut self, name: &str) -> Result<(), UploadError> {
        let index = self
            .secrets
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| UploadError::import("import_invalid", "no such secret"))?;
        let Some(value) = self.secrets[index]
            .value
            .as_ref()
            .map(|v| v.expose().to_owned())
        else {
            return Err(UploadError::import(
                "import_invalid",
                "this secret has no value to keep",
            ));
        };
        let reference = format!("{{secret:{name}}}");
        let secret = self.secrets.remove(index);
        let replace = |template: &mut String| *template = template.replace(&reference, &value);
        if let Some(request) = &mut self.destination.request {
            match &secret.slot {
                Slot::Header(n) => request.headers.get_mut(n).map(replace),
                Slot::Query(n) => request.query.get_mut(n).map(replace),
                Slot::Form(n) => request.fields.get_mut(n).map(replace),
                Slot::Body => request.body_template.as_mut().map(replace),
            };
        }
        self.destination.secrets.remove(name);
        self.destination
            .compile()
            .map_err(|e| UploadError::import("import_invalid", e.to_string()))?;
        Ok(())
    }
}

fn refuse(detail: impl Into<String>) -> UploadError {
    UploadError::import("import_unsupported", detail)
}

fn invalid(detail: impl Into<String>) -> UploadError {
    UploadError::import("import_invalid", detail)
}

/// A compiled-in `Regex`; the patterns are constants.
fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("constant pattern")
}

fn sanitize_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    name = name.trim_matches('-').to_owned();
    while name.contains("--") {
        name = name.replace("--", "-");
    }
    if name.is_empty() {
        "secret".clone_into(&mut name);
    }
    name.truncate(56);
    name.trim_end_matches('-').to_owned()
}

fn preview_of(value: Option<&SecretValue>, slot: &Slot) -> String {
    let Some(value) = value else {
        return "(to be entered)".to_owned();
    };
    if *slot == Slot::Body {
        // The end of a body is structure, not a token.
        return "***".to_owned();
    }
    let chars: Vec<char> = value.expose().chars().collect();
    let shown = (chars.len() / 4).min(4);
    if shown == 0 {
        "***".to_owned()
    } else {
        format!(
            "***{}",
            chars[chars.len() - shown..].iter().collect::<String>()
        )
    }
}

struct Builder {
    origin: String,
    secrets: Vec<ImportedSecret>,
    vars: BTreeMap<String, VarDecl>,
    taken: BTreeSet<String>,
    notes: Vec<ImportNote>,
}

impl Builder {
    fn unique(&mut self, base: &str) -> String {
        let base = sanitize_name(base);
        let mut name = base.clone();
        let mut n = 2;
        while self.taken.contains(&name) {
            name = format!("{base}-{n}");
            n += 1;
        }
        self.taken.insert(name.clone());
        name
    }

    fn add_secret(
        &mut self,
        name: String,
        slot: &Slot,
        value: Option<String>,
    ) -> Result<(), UploadError> {
        let value = value
            .map(SecretValue::new)
            .transpose()
            .map_err(|_| invalid("a value is too short to be a secret"))?;
        let placement = match slot {
            Slot::Header(n) => Placement::header(n),
            Slot::Query(n) => Placement::Query(n.clone()),
            Slot::Form(n) => Placement::Form(n.clone()),
            // Bound to the final template once the profile is complete.
            Slot::Body => Placement::Body(String::new()),
        };
        if matches!(slot, Slot::Query(_)) && !self.notes.iter().any(|n| n.code == "query_secret") {
            self.notes.push(ImportNote {
                code: "query_secret",
                detail: name.clone(),
            });
        }
        self.secrets.push(ImportedSecret {
            preview: preview_of(value.as_ref(), slot),
            name,
            slot: slot.clone(),
            placement,
            origin: self.origin.clone(),
            value,
        });
        Ok(())
    }

    /// A unique variable name made of `hint` (the title of a prompt, or the name of the slot).
    fn var_name(&mut self, hint: &str) -> String {
        let mut base: String = hint
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        base = base.trim_matches('_').to_owned();
        if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
            base.insert_str(0, "var_");
        }
        base.truncate(56);
        let mut name = base.clone();
        let mut n = 2;
        while self.vars.contains_key(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        name
    }

    /// Turns the text of one slot into the template the profile keeps.
    fn classify(&mut self, slot: &Slot, value: &str) -> Result<String, UploadError> {
        if value.is_empty() {
            return Ok(String::new());
        }
        let name = match slot {
            Slot::Header(n) | Slot::Query(n) | Slot::Form(n) => n.as_str(),
            Slot::Body => "body",
        };
        let prompts = regex(r"\{(inputbox|select):([^{}]*)\}");
        let public_slot = matches!(slot, Slot::Header(n) if is_public_header(n));
        // A prompt feeds a variable unless the slot is named like a credential: the engine refuses a
        // `--var` there (code 11), so it must be a secret to enter.
        let prompt_as_var = public_slot || !is_sensitive_name(name);
        let (prefix, rest) = match slot {
            Slot::Header(_) => split_scheme(value).unwrap_or(("", value)),
            _ => ("", value),
        };
        // A prompt: a variable in a public place, a secret to enter in a place of credentials.
        if prompts.is_match(value) {
            if !prompt_as_var {
                if let Some(c) = prompts.captures(rest)
                    && c.get(0).is_some_and(|m| m.as_str() == rest)
                {
                    let secret = self.unique(name);
                    self.add_secret(secret.clone(), slot, None)?;
                    return Ok(format!("{prefix}{{secret:{secret}}}"));
                }
                return Err(refuse(format!(
                    "{name}: a prompt mixed with other text in a place of credentials is not supported"
                )));
            }
            let mut out = String::new();
            let mut last = 0;
            for c in prompts.captures_iter(value) {
                let whole = c.get(0).expect("whole match");
                out.push_str(&value[last..whole.start()]);
                let (kind, args) = (&c[1], &c[2]);
                let parts: Vec<&str> = args.split('|').collect();
                // `{select:}` has no title: the variable is named after the slot.
                let var = self.var_name(if kind == "select" { name } else { parts[0] });
                let decl = if kind == "select" {
                    VarDecl {
                        prompt: None,
                        default: parts.first().map(|s| (*s).to_owned()),
                        choices: parts.iter().map(|s| (*s).to_owned()).collect(),
                    }
                } else {
                    VarDecl {
                        prompt: Some(parts[0].to_owned()),
                        default: parts
                            .get(1)
                            .filter(|d| !d.is_empty())
                            .map(|d| (*d).to_owned()),
                        choices: Vec::new(),
                    }
                };
                self.vars.insert(var.clone(), decl);
                out.push_str("{var:");
                out.push_str(&var);
                out.push('}');
                last = whole.end();
            }
            out.push_str(&value[last..]);
            return Ok(out);
        }
        let template = Template::parse(value, Side::Request).map_err(|e| refuse(e.to_string()))?;
        // Already a reference: an empty entry to fill.
        let references = template.secrets();
        if !references.is_empty() {
            // A reference does not excuse the text next to it (`{secret:key}:abcdef`): the shared
            // detector of `literals` decides, as it does for `dest edit` and `dest export`.
            if presumed_value(slot, value) {
                return Err(refuse(format!(
                    "{name}: a value that mixes a {{secret:}} reference with literal text is not supported; put the whole value in the secret"
                )));
            }
            for reference in references {
                if !is_identifier(&reference) {
                    return Err(invalid("a secret name uses letters, digits, '-' and '_'"));
                }
                self.taken.insert(reference.clone());
                self.add_secret(reference, slot, None)?;
            }
            return Ok(value.to_owned());
        }
        // A "pure" template holds no literal value. `{base64:user:password}` does: it is a credential.
        if public_slot || template.is_pure_call() {
            return Ok(value.to_owned());
        }
        if !matches!(slot, Slot::Body) {
            let rest_template =
                Template::parse(rest, Side::Request).map_err(|e| refuse(e.to_string()))?;
            if let Some(literal) = rest_template.base64_literal() {
                // `Basic {base64:user:password}`: the text inside goes to the store, the profile
                // keeps the function, which is rendered when the request is built.
                if literal.len() < MIN_SECRET_LEN {
                    return Ok(value.to_owned());
                }
                let secret = self.unique(name);
                self.add_secret(secret.clone(), slot, Some(literal))?;
                return Ok(format!("{prefix}{{base64:{{secret:{secret}}}}}"));
            }
            if rest_template.has_base64_text() {
                return Err(refuse(format!(
                    "{name}: a {{base64:}} that mixes text with other text or functions is not supported"
                )));
            }
        }
        if rest.len() < MIN_SECRET_LEN {
            return Ok(value.to_owned());
        }
        let secret = self.unique(name);
        if matches!(slot, Slot::Body) {
            self.notes.push(ImportNote {
                code: "body_template_secret",
                detail: secret.clone(),
            });
        }
        self.add_secret(secret.clone(), slot, Some(rest.to_owned()))?;
        Ok(format!("{prefix}{{secret:{secret}}}"))
    }

    fn classify_map(
        &mut self,
        input: &[(String, String)],
        make: fn(String) -> Slot,
    ) -> Result<BTreeMap<String, String>, UploadError> {
        let mut out = BTreeMap::new();
        for (name, value) in input {
            let template = self.classify(&make(name.clone()), value)?;
            out.insert(name.clone(), template);
        }
        Ok(out)
    }
}

fn object_of_strings(
    value: Option<&Value>,
    field: &str,
) -> Result<Vec<(String, String)>, UploadError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let object = value
        .as_object()
        .ok_or_else(|| invalid(format!("{field}: an object was expected")))?;
    object
        .iter()
        .map(|(k, v)| match v {
            Value::String(s) => Ok((k.clone(), s.clone())),
            Value::Number(n) => Ok((k.clone(), n.to_string())),
            Value::Bool(b) => Ok((k.clone(), b.to_string())),
            _ => Err(invalid(format!("{field}: the value of {k:?} must be text"))),
        })
        .collect()
}

fn string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, UploadError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(invalid(format!("{key}: text was expected"))),
    }
}

/// `(major, minor, patch)` of a ShareX version string.
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// `$json:x$` to `{json:x}` (ShareX before 13.7.1).
fn migrate_legacy(text: &str) -> String {
    regex(r"\$([a-z]+)(?::([^$]*))?\$")
        .replace_all(text, |c: &regex::Captures<'_>| match c.get(2) {
            Some(args) => format!("{{{}:{}}}", &c[1], args.as_str()),
            None => format!("{{{}}}", &c[1]),
        })
        .into_owned()
}

/// A token in the path of the address is refused (decision U2): the path is not a place for a secret.
/// Every import goes through here, `.sxcu` (`check_url`) and `.toml` or preset (`finish`).
fn refuse_token_in_path(url: &Url) -> Result<(), UploadError> {
    if url
        .path_segments()
        .is_some_and(|mut segments| segments.any(looks_like_token_segment))
    {
        return Err(UploadError::security(
            "secret_in_path",
            "the address holds what looks like a token in its path: not supported in v1",
        ));
    }
    Ok(())
}

fn check_url(raw: &str) -> Result<(Url, Vec<(String, String)>), UploadError> {
    if raw.contains('{') {
        return Err(refuse(
            "RequestURL: templates in the address are not supported (a token in the address is refused in v1)",
        ));
    }
    let url = Url::parse(raw.trim()).map_err(|_| invalid("RequestURL: not a valid address"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("RequestURL: an http or https address was expected"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("RequestURL: user:password@ is refused"));
    }
    if url.fragment().is_some() {
        return Err(invalid(
            "RequestURL: a fragment makes no sense in a request",
        ));
    }
    refuse_token_in_path(&url)?;
    let query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut bare = url;
    bare.set_query(None);
    Ok((bare, query))
}

fn slug(text: &str) -> String {
    let id = sanitize_name(text);
    if id == "secret" && !text.to_ascii_lowercase().contains("secret") {
        "imported".to_owned()
    } else {
        id
    }
}

const SXCU_FIELDS: [&str; 16] = [
    "Version",
    "Name",
    "DestinationType",
    "RequestMethod",
    "RequestURL",
    "Parameters",
    "Headers",
    "Body",
    "Arguments",
    "FileFormName",
    "Data",
    "URL",
    "ThumbnailURL",
    "DeletionURL",
    "ErrorMessage",
    "ResponseType",
];

fn sxcu_destination(
    object: &serde_json::Map<String, Value>,
    id: Option<&str>,
) -> Result<(Destination, Builder, Vec<ImportNote>), UploadError> {
    let unknown: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| !SXCU_FIELDS.contains(k))
        .collect();
    if !unknown.is_empty() {
        return Err(refuse(format!(
            "unsupported field(s): {}",
            unknown.join(", ")
        )));
    }
    let mut notes = Vec::new();
    // `Version`: 12.3.1 and below are refused by ShareX itself; below 13.7.1 the functions are
    // written `$json:x$`.
    let version = string_field(object, "Version")?
        .map(|v| parse_version(v).ok_or_else(|| invalid("Version: not a ShareX version")))
        .transpose()?;
    let legacy = version.is_some_and(|v| v < (13, 7, 1));
    if version.is_some_and(|v| v <= (12, 3, 1)) {
        return Err(refuse(
            "Version 12.3.1 and below are not supported (ShareX refuses them too)",
        ));
    }
    if legacy {
        notes.push(ImportNote {
            code: "legacy_syntax",
            detail: "$function$ migrated to {function}".to_owned(),
        });
    }
    let label = string_field(object, "Name")?
        .map(|n| {
            n.chars()
                .filter(|c| !c.is_control())
                .take(100)
                .collect::<String>()
        })
        .filter(|n| !n.trim().is_empty());
    // Kinds: image and file uploaders only; text types are dropped, shorteners refused.
    let mut kinds = Vec::new();
    if let Some(types) = string_field(object, "DestinationType")? {
        for part in types.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part {
                "ImageUploader" => kinds.push(MediaKind::Image),
                "FileUploader" => kinds.push(MediaKind::File),
                "TextUploader" => notes.push(ImportNote {
                    code: "text_types_ignored",
                    detail: part.to_owned(),
                }),
                "URLShortener" | "URLSharingService" => {
                    return Err(refuse(format!(
                        "DestinationType {part} is out of scope in v1"
                    )));
                }
                _ => return Err(refuse("DestinationType: unknown type")),
            }
        }
        if kinds.is_empty() {
            return Err(refuse("DestinationType: no image or file uploader"));
        }
    } else {
        kinds = vec![MediaKind::Image, MediaKind::File];
    }
    let method = string_field(object, "RequestMethod")?
        .unwrap_or("POST")
        .to_ascii_uppercase();
    let raw_url =
        string_field(object, "RequestURL")?.ok_or_else(|| invalid("RequestURL is missing"))?;
    let (url, url_query) = check_url(raw_url)?;
    let origin = origin_of(&url).ok_or_else(|| invalid("RequestURL: not a valid address"))?;
    let body = match string_field(object, "Body")? {
        None | Some("None") => BodyKind::None,
        Some("MultipartFormData") => BodyKind::Multipart,
        Some("FormURLEncoded") => BodyKind::Form,
        Some("JSON") => BodyKind::Json,
        Some("XML") => BodyKind::Xml,
        Some("Binary") => BodyKind::Binary,
        Some(_) => return Err(refuse("Body: unknown kind")),
    };
    let mut builder = Builder {
        origin,
        secrets: Vec::new(),
        vars: BTreeMap::new(),
        taken: BTreeSet::new(),
        notes: Vec::new(),
    };
    let mut parameters = url_query;
    parameters.extend(object_of_strings(object.get("Parameters"), "Parameters")?);
    let headers = object_of_strings(object.get("Headers"), "Headers")?;
    let arguments = object_of_strings(object.get("Arguments"), "Arguments")?;
    if !arguments.is_empty() && !matches!(body, BodyKind::Multipart | BodyKind::Form) {
        return Err(refuse(
            "Arguments are only supported with MultipartFormData and FormURLEncoded bodies",
        ));
    }
    let query = builder.classify_map(&parameters, Slot::Query)?;
    let headers = builder.classify_map(&headers, Slot::Header)?;
    let fields = builder.classify_map(&arguments, Slot::Form)?;
    let data = string_field(object, "Data")?.unwrap_or_default();
    let body_template = if matches!(body, BodyKind::Json | BodyKind::Xml) {
        if data.is_empty() {
            return Err(refuse("Data is needed with a JSON or XML body"));
        }
        Some(builder.classify(&Slot::Body, data)?)
    } else if data.is_empty() {
        None
    } else {
        return Err(refuse("Data is only supported with a JSON or XML body"));
    };
    let convert = |key: &str| -> Result<Option<String>, UploadError> {
        Ok(string_field(object, key)?
            .map(|t| {
                if legacy {
                    migrate_legacy(t)
                } else {
                    t.to_owned()
                }
            })
            .filter(|t| !t.is_empty()))
    };
    let mut response_url = convert("URL")?;
    match string_field(object, "ResponseType")? {
        None | Some("Text" | "RedirectionURL") if response_url.is_some() => {}
        None | Some("Text") => {}
        Some("RedirectionURL") => response_url = Some("{responseurl}".to_owned()),
        Some(_) => {
            return Err(refuse(
                "ResponseType: only Text and RedirectionURL are supported",
            ));
        }
    }
    let mut destination = Destination {
        id: id.map_or_else(
            || slug(label.as_deref().unwrap_or("imported")),
            str::to_owned,
        ),
        kind: DestinationType::Http,
        label,
        kinds,
        url: None,
        options: None,
        request: Some(RequestSpec {
            method,
            url: url.to_string(),
            query,
            headers,
            body,
            file_field: (body == BodyKind::Multipart).then(|| {
                string_field(object, "FileFormName")
                    .ok()
                    .flatten()
                    .filter(|f| !f.is_empty())
                    .unwrap_or("file")
                    .to_owned()
            }),
            fields,
            body_template,
            max_size: None,
            timeout: None,
            redirects: Redirects::SameOrigin,
        }),
        secrets: BTreeMap::new(),
        vars: BTreeMap::new(),
        response: Some(ResponseSpec {
            // ShareX: no `URL` means the whole answer is the address.
            url: Some(response_url.unwrap_or_else(|| "{response}".to_owned())),
            thumbnail_url: convert("ThumbnailURL")?,
            deletion_url: convert("DeletionURL")?,
            error: convert("ErrorMessage")?,
            success_status: vec![200, 201],
        }),
    };
    destination.vars.clone_from(&builder.vars);
    Ok((destination, builder, notes))
}

fn finish(
    mut destination: Destination,
    mut builder: Builder,
    mut notes: Vec<ImportNote>,
) -> Result<ImportPlan, UploadError> {
    for secret in &builder.secrets {
        destination
            .secrets
            .insert(secret.name.clone(), SecretDecl::default());
    }
    let compiled = destination.compile().map_err(|e| refuse(e.to_string()))?;
    let url = compiled.url.clone();
    refuse_token_in_path(&url)?;
    let https = url.scheme() == "https";
    if !https && !builder.secrets.is_empty() {
        return Err(UploadError::security(
            "https_required",
            "the profile uses secrets: its address must be https://",
        ));
    }
    // The body placement is tied to the final text of the template.
    let body_placement = compiled
        .body_template
        .as_ref()
        .map(|t| Placement::body(t.source()));
    for secret in &mut builder.secrets {
        if secret.slot == Slot::Body
            && let Some(p) = &body_placement
        {
            secret.placement = p.clone();
        }
    }
    notes.append(&mut builder.notes);
    Ok(ImportPlan {
        origin: compiled.origin().unwrap_or_default(),
        host: url
            .host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_ascii_lowercase(),
        https,
        destination,
        secrets: builder.secrets,
        notes,
    })
}

fn import_sxcu(text: &str, id: Option<&str>) -> Result<ImportPlan, UploadError> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| invalid("the file is not valid JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("a JSON object was expected"))?;
    let (destination, builder, notes) = sxcu_destination(object, id)?;
    finish(destination, builder, notes)
}

fn import_toml(text: &str, id: Option<&str>) -> Result<ImportPlan, UploadError> {
    let file = DestinationFile::parse(text, "the file")?;
    let mut destinations = file.destination;
    if destinations.len() != 1 {
        return Err(invalid("one destination per file was expected"));
    }
    let mut destination = destinations.remove(0);
    if let Some(id) = id {
        id.clone_into(&mut destination.id);
    }
    let compiled = destination.compile()?;
    let origin = compiled
        .origin()
        .ok_or_else(|| invalid("the address is not valid"))?;
    let mut builder = Builder {
        origin,
        secrets: Vec::new(),
        vars: BTreeMap::new(),
        taken: destination.secrets.keys().cloned().collect(),
        notes: Vec::new(),
    };
    if destination.kind == DestinationType::Http
        && let Some(request) = destination.request.take()
    {
        let mut request = request;
        let headers: Vec<_> = request.headers.clone().into_iter().collect();
        let query: Vec<_> = request.query.clone().into_iter().collect();
        let fields: Vec<_> = request.fields.clone().into_iter().collect();
        request.headers = builder.classify_map(&headers, Slot::Header)?;
        request.query = builder.classify_map(&query, Slot::Query)?;
        request.fields = builder.classify_map(&fields, Slot::Form)?;
        if let Some(body) = request.body_template.clone() {
            request.body_template = Some(builder.classify(&Slot::Body, &body)?);
        }
        destination.request = Some(request);
        destination.vars.extend(builder.vars.clone());
    } else {
        // A preset: its secrets are the ones it declares, placed where the preset puts them.
        for slot in compiled.secret_slots() {
            builder.add_secret(
                slot.name.clone(),
                &match &slot.placement {
                    Placement::Header(n) => Slot::Header(n.clone()),
                    Placement::Query(n) => Slot::Query(n.clone()),
                    Placement::Form(n) => Slot::Form(n.clone()),
                    Placement::Body(_) => Slot::Body,
                },
                None,
            )?;
        }
    }
    // Entries already declared and used keep their place; the declaration is rebuilt from use.
    destination.secrets.clear();
    finish(destination, builder, Vec::new())
}

/// Reads an imported file as data and plans the import.
///
/// `id` overrides the identifier of the profile. Nothing is stored and no request is made.
///
/// # Errors
/// [`UploadError::Config`] (`import_too_large`, `import_invalid`, `import_unsupported`) for a file that
/// cannot be imported, [`UploadError::Security`] (`https_required`, `secret_in_path`) for one that
/// the rules of `docs/security/secrets.md` refuse.
pub fn import(
    bytes: &[u8],
    format: ImportFormat,
    id: Option<&str>,
) -> Result<ImportPlan, UploadError> {
    if bytes.len() > MAX_IMPORT_SIZE {
        return Err(UploadError::import(
            "import_too_large",
            "the file is larger than 64 KiB",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("the file is not UTF-8 text"))?;
    if let Some(id) = id
        && !is_identifier(id)
    {
        return Err(invalid(
            "id: a profile name uses 1 to 64 letters, digits, '-' or '_'",
        ));
    }
    match format {
        ImportFormat::Sxcu => import_sxcu(text.trim_start_matches('\u{feff}'), id),
        ImportFormat::Toml => import_toml(text, id),
    }
}
