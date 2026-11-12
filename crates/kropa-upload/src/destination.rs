// SPDX-License-Identifier: GPL-3.0-or-later

//! Destinations (`docs/uploads.md` section 2): the TOML model, its validation at load time and the
//! Zipline v4 preset.
//!
//! A destination file only ever holds names of secrets (`{secret:token}`), never values. Loading
//! checks everything that can be checked without a network: templates, functions allowed on each
//! side, secrets declared, no secret in a URL, a response template or a file name, `{header:}` never
//! on an authentication header, regular expressions and JSON paths that compile.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use regex::RegexBuilder;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json_path::JsonPath;
use url::Url;

use crate::error::UploadError;
use crate::origin::origin_of;
use crate::secret::Placement;
use crate::template::{Side, Template};

/// Name of the file that holds the destinations, in the configuration folder.
pub const DESTINATIONS_FILE: &str = "destinations.toml";

/// Largest `max_size` (the file is read in memory).
pub const MAX_MAX_SIZE: u64 = 1 << 30;
/// Default `max_size`.
pub const DEFAULT_MAX_SIZE: u64 = 100 << 20;
/// Default `timeout`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest `timeout`.
pub const MAX_TIMEOUT: Duration = Duration::from_hours(1);
/// Redirections followed at most (`docs/security/secrets.md` section 4.5).
pub const MAX_REDIRECTS: usize = 5;
/// Size limit of a compiled regular expression (`{regex:}`).
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Headers a template may not set: the client owns them.
const FORBIDDEN_REQUEST_HEADERS: [&str; 7] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "te",
    "upgrade",
    "expect",
];

/// Response headers `{header:}` may never read (`docs/security/secrets.md` section 4.4).
const FORBIDDEN_RESPONSE_HEADERS: [&str; 5] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "www-authenticate",
];

/// Kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DestinationType {
    /// A custom HTTP API, described in full.
    Http,
    /// A Zipline v4 server, described by its address.
    Zipline,
}

impl DestinationType {
    /// Name as written in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Zipline => "zipline",
        }
    }
}

/// Kind of media a destination accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    /// Screenshots.
    Image,
    /// Any file.
    File,
}

/// How the file is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyKind {
    /// `multipart/form-data` with the file and the `fields`.
    #[default]
    Multipart,
    /// `application/x-www-form-urlencoded` with the `fields` (no file).
    Form,
    /// `body_template` as JSON.
    Json,
    /// `body_template` as XML.
    Xml,
    /// The raw file.
    Binary,
    /// No body.
    None,
}

/// Redirections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Redirects {
    /// Never follow.
    None,
    /// Follow while the origin does not change (default).
    #[default]
    SameOrigin,
}

/// A secret a destination expects. The value is in the store, never here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretDecl {
    /// What the secret is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A variable answered with `--var NAME=VALUE` (imported `{inputbox:}` and `{select:}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VarDecl {
    /// Question put to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Value used when none is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// The only values accepted, when not empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

/// Options of the Zipline preset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZiplineOptions {
    /// `x-zipline-format`: `random`, `date`, `uuid`, `name` or `gfycat`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// `x-zipline-original-name: true`: keep the name of the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub original_name: bool,
    /// `x-zipline-folder`: id of a folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The request of an `http` destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestSpec {
    /// HTTP method (`POST` by default).
    #[serde(default = "default_method")]
    pub method: String,
    /// Address, without query and without any template.
    pub url: String,
    /// Query parameters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query: BTreeMap<String, String>,
    /// Headers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Kind of body.
    #[serde(default)]
    pub body: BodyKind,
    /// Name of the file field (`multipart`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_field: Option<String>,
    /// Other fields (`multipart` and `form`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    /// Text of a `json` or `xml` body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_template: Option<String>,
    /// Largest file sent (`100MiB`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<String>,
    /// Total time allowed (`120s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Redirections.
    #[serde(default)]
    pub redirects: Redirects,
}

fn default_method() -> String {
    "POST".to_owned()
}

/// How the result is read from the response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseSpec {
    /// Template of the public URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Template of the thumbnail URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    /// Template of the deletion URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_url: Option<String>,
    /// Template of the error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Statuses that mean success (`[200, 201]` by default).
    #[serde(
        default = "default_success",
        skip_serializing_if = "is_default_success"
    )]
    pub success_status: Vec<u16>,
}

fn default_success() -> Vec<u16> {
    vec![200, 201]
}

fn is_default_success(v: &[u16]) -> bool {
    v == [200, 201]
}

fn default_kinds() -> Vec<MediaKind> {
    vec![MediaKind::Image, MediaKind::File]
}

/// One destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// Profile name, used by the command line.
    pub id: String,
    /// Kind.
    #[serde(rename = "type")]
    pub kind: DestinationType,
    /// Name shown to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Media accepted.
    #[serde(default = "default_kinds")]
    pub kinds: Vec<MediaKind>,
    /// Base address of a `zipline` destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Options of a `zipline` destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ZiplineOptions>,
    /// Request of an `http` destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestSpec>,
    /// Secrets expected, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretDecl>,
    /// Variables expected, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, VarDecl>,
    /// Response of an `http` destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseSpec>,
}

/// The content of `destinations.toml` (or of a file to import).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationFile {
    /// The destinations.
    #[serde(default)]
    pub destination: Vec<Destination>,
}

/// Keeps the position of a TOML error and drops every quoted fragment: the message of a type error
/// quotes the value, which in an imported file may be a secret.
pub(crate) fn toml_error(path: &str, text: &str, e: &toml::de::Error) -> UploadError {
    let (line, column) = e.span().map_or((1, 1), |s| {
        let before = &text[..s.start.min(text.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        (line, column)
    });
    let mut message = String::new();
    let mut quote: Option<char> = None;
    let mut chars = e.message().chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '"' | '`') => {
                quote = Some(c);
                message.push('…');
            }
            // An escaped character inside the quotes (`\"`) does not end them.
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, c) => message.push(c),
        }
    }
    UploadError::config(format!("{path}:{line}:{column}: {message}"))
}

impl DestinationFile {
    /// Parses and validates a file. `path` is only used in messages.
    ///
    /// # Errors
    /// [`UploadError::Config`] on a syntax error, an unknown key, a duplicated `id` or an invalid
    /// destination.
    pub fn parse(text: &str, path: &str) -> Result<Self, UploadError> {
        let file: Self = toml::from_str(text).map_err(|e| toml_error(path, text, &e))?;
        let mut seen = BTreeSet::new();
        for d in &file.destination {
            if !seen.insert(d.id.as_str()) {
                return Err(UploadError::config(format!(
                    "{path}: the destination {:?} is defined twice",
                    d.id
                )));
            }
            d.compile()
                .map_err(|e| UploadError::config(format!("{path}: destination {:?}: {e}", d.id)))?;
        }
        Ok(file)
    }
}

/// A valid identifier of a profile, a secret or a variable.
#[must_use]
pub fn is_identifier(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parses `100MiB`, `512KiB`, `2GiB`, `5MB` or a number of bytes.
pub(crate) fn parse_size(text: &str) -> Result<u64, UploadError> {
    let bad = || {
        UploadError::config(format!(
            "max_size: {text:?} is not a size (expected for example 100MiB)"
        ))
    };
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = text.split_at(digits);
    let number: u64 = number.parse().map_err(|_| bad())?;
    let factor: u64 = match unit.trim() {
        "" | "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        _ => return Err(bad()),
    };
    let size = number.checked_mul(factor).ok_or_else(bad)?;
    if size == 0 || size > MAX_MAX_SIZE {
        return Err(UploadError::config(
            "max_size: must be between 1 byte and 1GiB",
        ));
    }
    Ok(size)
}

/// Parses `500ms`, `120s`, `2m`.
pub(crate) fn parse_duration(text: &str) -> Result<Duration, UploadError> {
    let bad = || {
        UploadError::config(format!(
            "timeout: {text:?} is not a duration (expected for example 120s)"
        ))
    };
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = text.split_at(digits);
    let number: u64 = number.parse().map_err(|_| bad())?;
    let duration = match unit {
        "ms" => Duration::from_millis(number),
        "s" => Duration::from_secs(number),
        "m" => Duration::from_mins(number),
        _ => return Err(bad()),
    };
    if duration.is_zero() || duration > MAX_TIMEOUT {
        return Err(UploadError::config("timeout: must be between 1ms and 1h"));
    }
    Ok(duration)
}

/// `$.` is added to a JSON path that lacks it, as ShareX does.
#[must_use]
pub fn normalize_json_path(path: &str) -> String {
    let path = path.trim();
    if path.starts_with('$') {
        path.to_owned()
    } else if path.starts_with('[') {
        format!("${path}")
    } else {
        format!("$.{path}")
    }
}

/// Where a secret is used in a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretSlot {
    /// Name of the secret.
    pub name: String,
    /// Place in the request.
    pub placement: Placement,
}

/// A destination whose templates are parsed and whose values are checked.
#[derive(Debug, Clone)]
pub struct Compiled {
    /// Profile name.
    pub id: String,
    /// Accepted media.
    pub kinds: Vec<MediaKind>,
    /// HTTP method.
    pub method: String,
    /// Address of the request.
    pub url: Url,
    /// Query parameters.
    pub query: Vec<(String, Template)>,
    /// Headers.
    pub headers: Vec<(String, Template)>,
    /// Kind of body.
    pub body: BodyKind,
    /// File field of a multipart body.
    pub file_field: String,
    /// Other fields.
    pub fields: Vec<(String, Template)>,
    /// Body of a JSON or XML request.
    pub body_template: Option<Template>,
    /// Largest file.
    pub max_size: u64,
    /// Total time.
    pub timeout: Duration,
    /// Redirections.
    pub redirects: Redirects,
    /// Response.
    pub response: CompiledResponse,
    /// Names of the declared secrets.
    pub secrets: BTreeSet<String>,
    /// Declared variables.
    pub vars: BTreeMap<String, VarDecl>,
}

/// Parsed response templates.
#[derive(Debug, Clone)]
pub struct CompiledResponse {
    /// Public URL.
    pub url: Template,
    /// Thumbnail URL.
    pub thumbnail_url: Option<Template>,
    /// Deletion URL.
    pub deletion_url: Option<Template>,
    /// Error message.
    pub error: Option<Template>,
    /// Statuses that mean success.
    pub success_status: Vec<u16>,
}

impl Compiled {
    /// Canonical origin of the request.
    #[must_use]
    pub fn origin(&self) -> Option<String> {
        origin_of(&self.url)
    }

    /// Every use of a secret in the request, in a stable order.
    #[must_use]
    pub fn secret_slots(&self) -> Vec<SecretSlot> {
        let mut slots = Vec::new();
        let mut add = |placement: Placement, template: &Template| {
            for name in template.secrets() {
                slots.push(SecretSlot {
                    name,
                    placement: placement.clone(),
                });
            }
        };
        for (name, t) in &self.headers {
            add(Placement::header(name), t);
        }
        for (name, t) in &self.query {
            add(Placement::Query(name.clone()), t);
        }
        for (name, t) in &self.fields {
            add(Placement::Form(name.clone()), t);
        }
        if let Some(t) = &self.body_template {
            add(Placement::body(t.source()), t);
        }
        slots
    }
}

fn parse_template(source: &str, side: Side, what: &str) -> Result<Template, UploadError> {
    Template::parse(source, side).map_err(|e| UploadError::config(format!("{what}: {e}")))
}

fn compile_map(
    map: &BTreeMap<String, String>,
    what: &str,
) -> Result<Vec<(String, Template)>, UploadError> {
    map.iter()
        .map(|(k, v)| {
            if k.is_empty() || k.chars().any(crate::engine::is_hostile_char) {
                return Err(UploadError::config(format!("{what}: invalid name")));
            }
            Ok((
                k.clone(),
                parse_template(v, Side::Request, &format!("{what} {k:?}"))?,
            ))
        })
        .collect()
}

fn check_http_url(raw: &str) -> Result<Url, UploadError> {
    if raw.contains('{') {
        return Err(UploadError::config(
            "url: templates and secrets are not allowed in the address (a secret in the path is not supported in v1)",
        ));
    }
    let url = Url::parse(raw).map_err(|_| UploadError::config("url: not a valid address"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UploadError::config("url: the scheme must be http or https"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(UploadError::config("url: no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UploadError::config("url: user:password@ is refused"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(UploadError::config(
            "url: no query or fragment here, use [request.query]",
        ));
    }
    Ok(url)
}

fn check_regex(pattern: &str) -> Result<(), UploadError> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT * 2)
        .build()
        .map(|_| ())
        .map_err(|_| UploadError::config("response: a {regex:} pattern is invalid or too large"))
}

fn check_json_path(path: &str) -> Result<(), UploadError> {
    JsonPath::parse(&normalize_json_path(path))
        .map(|_| ())
        .map_err(|_| UploadError::config("response: a {json:} path is not valid (RFC 9535)"))
}

impl Destination {
    /// A Zipline v4 destination for `base_url`.
    #[must_use]
    pub fn zipline(id: &str, base_url: &str) -> Self {
        Self {
            id: id.to_owned(),
            kind: DestinationType::Zipline,
            label: None,
            kinds: default_kinds(),
            url: Some(base_url.to_owned()),
            options: None,
            request: None,
            secrets: BTreeMap::from([("token".to_owned(), SecretDecl::default())]),
            vars: BTreeMap::new(),
            response: None,
        }
    }

    /// The base address, for display: the `url` of a Zipline destination or the request address.
    #[must_use]
    pub fn address(&self) -> Option<&str> {
        match self.kind {
            DestinationType::Zipline => self.url.as_deref(),
            DestinationType::Http => self.request.as_ref().map(|r| r.url.as_str()),
        }
    }

    /// Canonical origin of the destination, `None` when the address is not valid.
    #[must_use]
    pub fn origin(&self) -> Option<String> {
        origin_of(&Url::parse(self.address()?).ok()?)
    }

    /// The request and response of the destination, with the preset expanded.
    ///
    /// # Errors
    /// [`UploadError::Config`] when the destination is incomplete or mixes the fields of the two
    /// types.
    pub fn expand(&self) -> Result<(RequestSpec, ResponseSpec), UploadError> {
        match self.kind {
            DestinationType::Zipline => {
                if self.request.is_some() || self.response.is_some() {
                    return Err(UploadError::config(
                        "a zipline destination has no [request] or [response]: use type = \"http\" to describe one",
                    ));
                }
                let base = self
                    .url
                    .as_deref()
                    .ok_or_else(|| UploadError::config("a zipline destination needs url"))?;
                Ok(zipline_spec(
                    base,
                    self.options.as_ref().unwrap_or(&ZiplineOptions::default()),
                )?)
            }
            DestinationType::Http => {
                if self.url.is_some() || self.options.is_some() {
                    return Err(UploadError::config(
                        "url and [options] belong to zipline destinations: use [request] url",
                    ));
                }
                let request = self
                    .request
                    .clone()
                    .ok_or_else(|| UploadError::config("an http destination needs [request]"))?;
                let response = self
                    .response
                    .clone()
                    .ok_or_else(|| UploadError::config("an http destination needs [response]"))?;
                Ok((request, response))
            }
        }
    }

    /// Checks the destination without keeping the result.
    ///
    /// # Errors
    /// See [`Destination::compile`].
    pub fn validate(&self) -> Result<(), UploadError> {
        self.compile().map(|_| ())
    }

    /// Parses and checks everything that can be checked without a network.
    ///
    /// # Errors
    /// [`UploadError::Config`] naming what is wrong (never a secret value).
    pub fn compile(&self) -> Result<Compiled, UploadError> {
        if !is_identifier(&self.id) {
            return Err(UploadError::config(
                "id: a profile name uses 1 to 64 letters, digits, '-' or '_'",
            ));
        }
        if self.kinds.is_empty() {
            return Err(UploadError::config("kinds: at least one kind of media"));
        }
        for name in self.secrets.keys().chain(self.vars.keys()) {
            if !is_identifier(name) {
                return Err(UploadError::config(
                    "secrets and vars: names use 1 to 64 letters, digits, '-' or '_'",
                ));
            }
        }
        let (request, response) = self.expand()?;
        let url = check_http_url(&request.url)?;
        let method = request.method.to_ascii_uppercase();
        if !matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
            return Err(UploadError::config("method: POST, PUT or PATCH"));
        }
        let query = compile_map(&request.query, "query")?;
        let headers = compile_map(&request.headers, "header")?;
        let fields = compile_map(&request.fields, "field")?;
        for (name, _) in &headers {
            let lower = name.to_ascii_lowercase();
            if HeaderName::from_bytes(name.as_bytes()).is_err()
                || FORBIDDEN_REQUEST_HEADERS.contains(&lower.as_str())
                || (lower == "content-type" && request.body == BodyKind::Multipart)
            {
                return Err(UploadError::config(format!(
                    "header {name:?} is not allowed"
                )));
            }
        }
        match request.body {
            BodyKind::Multipart | BodyKind::Form => {}
            _ if !fields.is_empty() => {
                return Err(UploadError::config(
                    "fields: only for multipart and form bodies",
                ));
            }
            _ => {}
        }
        if request.body != BodyKind::Multipart && request.file_field.is_some() {
            return Err(UploadError::config("file_field: only for multipart bodies"));
        }
        let body_template = match (&request.body_template, request.body) {
            (Some(t), BodyKind::Json | BodyKind::Xml) => {
                Some(parse_template(t, Side::Request, "body_template")?)
            }
            (None, BodyKind::Json | BodyKind::Xml) => {
                return Err(UploadError::config(
                    "body_template: needed for json and xml bodies",
                ));
            }
            (Some(_), _) => {
                return Err(UploadError::config(
                    "body_template: only for json and xml bodies",
                ));
            }
            (None, _) => None,
        };
        let file_field = request
            .file_field
            .clone()
            .unwrap_or_else(|| "file".to_owned());
        if file_field.is_empty() || file_field.chars().any(crate::engine::is_hostile_char) {
            return Err(UploadError::config("file_field: invalid name"));
        }
        let max_size = request
            .max_size
            .as_deref()
            .map_or(Ok(DEFAULT_MAX_SIZE), parse_size)?;
        let timeout = request
            .timeout
            .as_deref()
            .map_or(Ok(DEFAULT_TIMEOUT), parse_duration)?;

        let response = compile_response(&response, &headers)?;
        let compiled = Compiled {
            id: self.id.clone(),
            kinds: self.kinds.clone(),
            method,
            url,
            query,
            headers,
            body: request.body,
            file_field,
            fields,
            body_template,
            max_size,
            timeout,
            redirects: request.redirects,
            response,
            secrets: self.secrets.keys().cloned().collect(),
            vars: self.vars.clone(),
        };
        for slot in compiled.secret_slots() {
            if !is_identifier(&slot.name) {
                return Err(UploadError::config(
                    "{secret:} names use 1 to 64 letters, digits, '-' or '_'",
                ));
            }
            if !compiled.secrets.contains(&slot.name) {
                return Err(UploadError::config(format!(
                    "secret {:?} is used but not declared under [secrets]",
                    slot.name
                )));
            }
        }
        Ok(compiled)
    }

    /// The destination as TOML, for `dest export` and for the import. Holds names of secrets only.
    ///
    /// # Errors
    /// [`UploadError::Config`] if the destination cannot be written (cannot happen for a valid one).
    pub fn to_toml(&self) -> Result<String, UploadError> {
        toml::to_string(&DestinationFile {
            destination: vec![self.clone()],
        })
        .map_err(|e| UploadError::config(format!("cannot write the destination: {e}")))
    }
}

fn compile_response(
    spec: &ResponseSpec,
    request_headers: &[(String, Template)],
) -> Result<CompiledResponse, UploadError> {
    let parse = |source: &Option<String>, what: &str| -> Result<Option<Template>, UploadError> {
        source
            .as_deref()
            .map(|s| parse_template(s, Side::Response, what))
            .transpose()
    };
    let url = parse(&spec.url, "response.url")?
        .ok_or_else(|| UploadError::config("response.url: the template of the public URL"))?;
    let thumbnail_url = parse(&spec.thumbnail_url, "response.thumbnail_url")?;
    let deletion_url = parse(&spec.deletion_url, "response.deletion_url")?;
    let error = parse(&spec.error, "response.error")?;
    if spec.success_status.is_empty()
        || spec.success_status.iter().any(|s| !(100..=599).contains(s))
    {
        return Err(UploadError::config(
            "success_status: HTTP statuses from 100 to 599",
        ));
    }
    // Headers that carry a secret in the request may not be read back (B1).
    let mut forbidden: BTreeSet<String> = FORBIDDEN_RESPONSE_HEADERS
        .iter()
        .map(|h| (*h).to_owned())
        .collect();
    for (name, t) in request_headers {
        if !t.secrets().is_empty() {
            forbidden.insert(name.to_ascii_lowercase());
        }
    }
    for template in [
        Some(&url),
        thumbnail_url.as_ref(),
        deletion_url.as_ref(),
        error.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for header in template.header_reads() {
            if forbidden.contains(&header.to_ascii_lowercase())
                || HeaderName::from_bytes(header.as_bytes()).is_err()
            {
                return Err(UploadError::config(
                    "response: {header:} may not read an authentication header or a header that carries a secret",
                ));
            }
        }
        for pattern in template.regexes() {
            check_regex(&pattern)?;
        }
        for path in template.json_paths() {
            check_json_path(&path)?;
        }
    }
    Ok(CompiledResponse {
        url,
        thumbnail_url,
        deletion_url,
        error,
        success_status: spec.success_status.clone(),
    })
}

/// The request and response of a Zipline v4 server (`docs/uploads.md` section 1.1): multipart POST
/// of the file in `file`, the token alone (without `Bearer`) in `Authorization`, the URL in
/// `files[0].url`.
fn zipline_spec(
    base: &str,
    options: &ZiplineOptions,
) -> Result<(RequestSpec, ResponseSpec), UploadError> {
    let base = Url::parse(base).map_err(|_| UploadError::config("url: not a valid address"))?;
    let mut address = base.as_str().trim_end_matches('/').to_owned();
    if base.query().is_some() || base.fragment().is_some() {
        return Err(UploadError::config("url: no query or fragment"));
    }
    address.push_str("/api/upload");
    let mut headers = BTreeMap::from([
        ("Authorization".to_owned(), "{secret:token}".to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
    ]);
    if let Some(format) = &options.format {
        if !["random", "date", "uuid", "name", "gfycat"].contains(&format.as_str()) {
            return Err(UploadError::config(
                "options.format: random, date, uuid, name or gfycat",
            ));
        }
        headers.insert("x-zipline-format".to_owned(), format.clone());
    }
    if options.original_name {
        headers.insert("x-zipline-original-name".to_owned(), "true".to_owned());
    }
    if let Some(folder) = &options.folder {
        if folder.is_empty()
            || !folder
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(UploadError::config("options.folder: a folder id"));
        }
        headers.insert("x-zipline-folder".to_owned(), folder.clone());
    }
    for value in headers.values() {
        if HeaderValue::from_str(value).is_err() {
            return Err(UploadError::config("options: invalid header value"));
        }
    }
    Ok((
        RequestSpec {
            method: "POST".to_owned(),
            url: address,
            query: BTreeMap::new(),
            headers,
            body: BodyKind::Multipart,
            file_field: Some("file".to_owned()),
            fields: BTreeMap::new(),
            body_template: None,
            max_size: None,
            timeout: None,
            redirects: Redirects::SameOrigin,
        },
        ResponseSpec {
            url: Some("{json:$.files[0].url}".to_owned()),
            thumbnail_url: None,
            deletion_url: None,
            error: Some("{json:$.error}".to_owned()),
            success_status: vec![200],
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        BodyKind, Destination, DestinationFile, normalize_json_path, parse_duration, parse_size,
    };
    use crate::error::UploadError;
    use crate::secret::Placement;

    const MAISON: &str = r#"
[[destination]]
id      = "maison"
type    = "http"
label   = "Mon serveur d'images"
kinds   = ["image", "file"]

[destination.request]
method  = "POST"
url     = "https://img.example.org/api/upload"
query   = { visibility = "unlisted" }
headers = { Authorization = "Bearer {secret:token}", Accept = "application/json" }
body    = "multipart"
file_field = "file"
fields  = { title = "{filename}" }
max_size = "100MiB"
timeout  = "120s"
redirects = "same-origin"

[destination.secrets]
token = {}

[destination.response]
url          = "{json:$.data.url}"
thumbnail_url = "{json:$.data.thumb}"
deletion_url = "https://img.example.org/del/{json:$.data.delete_token}"
error        = "{json:$.error.message}"
success_status = [200, 201]
"#;

    fn config_error(text: &str) -> String {
        match DestinationFile::parse(text, "d.toml") {
            Err(UploadError::Config { detail, .. }) => detail,
            other => panic!("expected a config error, got {other:?}"),
        }
    }

    #[test]
    fn the_documented_example_loads() {
        let file = DestinationFile::parse(MAISON, "d.toml").unwrap();
        let compiled = file.destination[0].compile().unwrap();
        assert_eq!(
            compiled.origin().as_deref(),
            Some("https://img.example.org:443")
        );
        let slots = compiled.secret_slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "token");
        assert_eq!(slots[0].placement, Placement::header("authorization"));
        assert_eq!(compiled.body, BodyKind::Multipart);
    }

    #[test]
    fn zipline_preset() {
        let d = Destination::zipline("z", "https://zipline.example.org/");
        let c = d.compile().unwrap();
        assert_eq!(c.url.as_str(), "https://zipline.example.org/api/upload");
        assert_eq!(
            c.secret_slots()[0].placement,
            Placement::header("Authorization")
        );
        assert_eq!(c.response.success_status, [200]);
        let (request, _) = d.expand().unwrap();
        assert_eq!(request.headers["Authorization"], "{secret:token}");
        // Round trip through TOML.
        let text = d.to_toml().unwrap();
        let back = DestinationFile::parse(&text, "z.toml").unwrap();
        assert_eq!(back.destination[0], d);
    }

    #[test]
    fn zipline_options() {
        let mut d = Destination::zipline("z", "https://zipline.example.org");
        d.options = Some(super::ZiplineOptions {
            format: Some("uuid".into()),
            original_name: true,
            folder: Some("abc123".into()),
        });
        let (request, _) = d.expand().unwrap();
        assert_eq!(request.headers["x-zipline-format"], "uuid");
        assert_eq!(request.headers["x-zipline-original-name"], "true");
        assert_eq!(request.headers["x-zipline-folder"], "abc123");
        d.options.as_mut().unwrap().format = Some("nope".into());
        assert!(d.compile().is_err());
    }

    #[test]
    fn secrets_must_be_declared_and_named_simply() {
        let text = MAISON.replace("token = {}", "");
        assert!(config_error(&text).contains("not declared"));
        let text = MAISON.replace("{secret:token}", "{secret:a b}");
        assert!(!config_error(&text).is_empty());
    }

    #[test]
    fn secrets_are_refused_where_they_must_not_be() {
        // In the address (decision U2).
        let text = MAISON.replace(
            "https://img.example.org/api/upload",
            "https://img.example.org/{secret:token}/upload",
        );
        assert!(config_error(&text).contains("not allowed in the address"));
        // In a response template.
        let text = MAISON.replace("{json:$.data.url}", "{secret:token}");
        assert!(config_error(&text).contains("cannot be used in a response template"));
        let text = MAISON.replace(
            "https://img.example.org/del/{json:$.data.delete_token}",
            "https://img.example.org/del/{secret:token}",
        );
        assert!(config_error(&text).contains("response"));
        // A user:password@ address, a query in the address.
        let text = MAISON.replace(
            "https://img.example.org/api",
            "https://u:p@img.example.org/api",
        );
        assert!(config_error(&text).contains("user:password@"));
        let text = MAISON.replace("/api/upload\"", "/api/upload?key=abc\"");
        assert!(config_error(&text).contains("query"));
    }

    #[test]
    fn response_headers_that_carry_credentials_cannot_be_read_back() {
        for header in ["Authorization", "Set-Cookie", "Cookie"] {
            let text = MAISON.replace("{json:$.data.url}", &format!("{{header:{header}}}"));
            assert!(config_error(&text).contains("{header:}"), "{header}");
        }
        // A header the request uses for a secret.
        let text = MAISON
            .replace("Authorization = ", "\"X-Api-Key\" = ")
            .replace("{json:$.data.url}", "{header:x-api-key}");
        assert!(config_error(&text).contains("{header:}"));
        // Ordinary headers are fine.
        let text = MAISON.replace("{json:$.data.url}", "{header:Location}");
        assert!(DestinationFile::parse(&text, "d.toml").is_ok());
    }

    #[test]
    fn bad_values_are_named() {
        for (from, to, needle) in [
            ("max_size = \"100MiB\"", "max_size = \"2TB\"", "max_size"),
            ("timeout  = \"120s\"", "timeout  = \"soon\"", "timeout"),
            ("method  = \"POST\"", "method  = \"DELETE\"", "method"),
            (
                "success_status = [200, 201]",
                "success_status = [99]",
                "success_status",
            ),
            ("{json:$.data.thumb}", "{json:$.data[}", "path"),
            ("{json:$.data.thumb}", "{regex:(unclosed}", "regex"),
            ("{json:$.data.thumb}", "{xml:/a}", "not supported"),
            ("{filename}", "{random:a|b}", "not supported"),
            ("kinds   = [\"image\", \"file\"]", "kinds   = []", "kinds"),
            ("id      = \"maison\"", "id      = \"a/b\"", "id"),
        ] {
            let text = MAISON.replace(from, to);
            let detail = config_error(&text);
            assert!(detail.contains(needle), "{to}: {detail}");
        }
    }

    #[test]
    fn unknown_keys_and_duplicates_are_refused() {
        assert!(
            config_error(&MAISON.replace("label   =", "color = 1\nlabel   ="))
                .contains("unknown field")
        );
        let twice = format!("{MAISON}\n{MAISON}");
        assert!(config_error(&twice).contains("twice"));
    }

    #[test]
    fn toml_errors_do_not_quote_values() {
        let text = MAISON.replace(
            "headers = { Authorization = \"Bearer {secret:token}\", Accept = \"application/json\" }",
            "headers = \"Bearer FAKE-value-for-test-0001\"",
        );
        let detail = config_error(&text);
        assert!(!detail.contains("FAKE-value"), "{detail}");
        assert!(detail.contains("d.toml:"), "{detail}");
    }

    #[test]
    fn toml_errors_keep_dropping_after_an_escaped_quote() {
        // Built at run time: nothing in the source looks like a token.
        let tail = ["Zq7-", "Hx3Lm9"].concat();
        let text = MAISON.replace(
            "headers = { Authorization = \"Bearer {secret:token}\", Accept = \"application/json\" }",
            &format!("headers = \"ab\\\"{tail}\""),
        );
        let detail = config_error(&text);
        assert!(!detail.contains(&tail), "{detail}");
        assert!(!detail.contains("ab"), "{detail}");
    }

    #[test]
    fn invisible_characters_in_a_name_are_refused_at_load() {
        for bad in ["a\\u202Eb", "a\\u200Bb", "a\\uFEFF", "a\\u2066b"] {
            let text = MAISON.replace(
                "Accept = \"application/json\"",
                &format!("Accept = \"application/json\", \"{bad}\" = \"x\""),
            );
            assert!(config_error(&text).contains("invalid name"), "{bad:?}");
        }
    }

    #[test]
    fn sizes_durations_and_paths() {
        assert_eq!(parse_size("100MiB").unwrap(), 100 << 20);
        assert_eq!(parse_size("5MB").unwrap(), 5_000_000);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert!(parse_size("0").is_err());
        assert!(parse_size("2GiB").is_err());
        assert!(parse_size("MiB").is_err());
        assert_eq!(parse_duration("2m").unwrap().as_secs(), 120);
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("2h").is_err());
        assert_eq!(normalize_json_path("data.link"), "$.data.link");
        assert_eq!(normalize_json_path("[0].link"), "$[0].link");
        assert_eq!(normalize_json_path("$.a"), "$.a");
    }
}
