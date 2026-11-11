// SPDX-License-Identifier: GPL-3.0-or-later

//! The upload itself: resolve the secrets, build the request, follow same-origin redirects, read at
//! most 1 MiB, read the URLs out of the response and check them before they leave.
//!
//! Order of the checks (`docs/security/secrets.md`): the destination is compiled; the transport is
//! checked (HTTPS as soon as a secret is used); each secret is read from the store and must allow the
//! origin of the request and the place it is put in; **only then** is a connection made.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;

use regex::RegexBuilder;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, LOCATION};
use reqwest::multipart::{Form, Part};
use reqwest::{Method, RequestBuilder};
use serde_json::Value;
use serde_json_path::JsonPath;
use url::Url;

use crate::debug_log::{BODY_LIMIT, DebugLog, redact_header_map, redact_headers, redact_url};
use crate::destination::{
    BodyKind, Compiled, Destination, MAX_REDIRECTS, MediaKind, REGEX_SIZE_LIMIT, Redirects,
    normalize_json_path,
};
use crate::error::{Retry, UploadError};
use crate::http::{HttpClient, network_error};
use crate::masked::{CheckedOutput, MaskedText, SecretSet, register_secret};
use crate::origin::{canonical_origin, origin_of};
use crate::secret::{Placement, SHORT_SECRET_LEN, SecretSource, SecretValue};
use crate::sensitive::is_sensitive_name;
use crate::template::{Env, Function, Group, Template};

/// Largest response read; a bigger one is an error, not a truncation (`docs/cli.md` section 3).
pub const MAX_RESPONSE: usize = 1 << 20;
/// Characters of a server text kept for a message.
pub const MESSAGE_LIMIT: usize = 500;

/// A 1x1 PNG, the "témoin" image of `kropa dest test`.
pub const TEST_IMAGE_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x00, 0x02, 0x00,
    0x00, 0x05, 0x00, 0x01, 0x7A, 0x5E, 0xAB, 0x3F, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
    0xAE, 0x42, 0x60, 0x82,
];

/// What is sent.
#[derive(Debug, Clone)]
pub struct UploadInput {
    /// File name shown to the server and used by `{filename}`.
    pub name: String,
    /// Content.
    pub bytes: Vec<u8>,
    /// Media type.
    pub mime: String,
}

fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned
    }
}

/// Media type of a file name, by extension.
#[must_use]
pub fn mime_of(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("txt" | "log") => "text/plain",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

impl UploadInput {
    /// The built-in test image.
    #[must_use]
    pub fn test_image() -> Self {
        Self {
            name: "kropa-test.png".to_owned(),
            bytes: TEST_IMAGE_PNG.to_vec(),
            mime: "image/png".to_owned(),
        }
    }

    /// An in-memory file.
    #[must_use]
    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Self {
        let name = clean_name(name);
        let mime = mime_of(&name).to_owned();
        Self { name, bytes, mime }
    }

    /// Reads a file of at most `max` bytes.
    ///
    /// # Errors
    /// [`UploadError::Io`] if it cannot be read, [`UploadError::Failed`] (`file_too_large`) if it is
    /// bigger than `max`. Nothing is read beyond `max`.
    pub fn from_file(path: &Path, max: u64) -> Result<Self, UploadError> {
        use std::io::Read as _;
        let io = |source| UploadError::Io {
            path: path.to_owned(),
            source,
        };
        let file = std::fs::File::open(path).map_err(io)?;
        let metadata = file.metadata().map_err(io)?;
        if !metadata.is_file() {
            return Err(io(std::io::Error::other("not a regular file")));
        }
        let too_large =
            || UploadError::failed("file_too_large", "the file is larger than max_size");
        if metadata.len() > max {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        file.take(max + 1).read_to_end(&mut bytes).map_err(io)?;
        if bytes.len() as u64 > max {
            return Err(too_large());
        }
        let name = path
            .file_name()
            .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
        Ok(Self::from_bytes(&name, bytes))
    }

    fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

/// A note about a successful upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadWarning {
    /// Machine code: `secret_short`, `thumbnail_url_invalid`, `deletion_url_invalid`.
    pub code: &'static str,
    /// Name of the secret or of the field; never a value.
    pub detail: String,
}

/// A response header kept for `dest test`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderLine {
    /// Header name, lower case.
    pub name: String,
    /// Value, masked.
    pub value: MaskedText,
}

/// A successful upload. Every text that leaves goes through [`CheckedOutput`].
#[derive(Debug, Clone)]
pub struct UploadOutcome {
    /// HTTP status of the answer.
    pub status: u16,
    /// Public URL.
    pub url: CheckedOutput,
    /// Thumbnail URL.
    pub thumbnail_url: Option<CheckedOutput>,
    /// Deletion URL (sensitive: JSON and history only, never stderr or logs).
    pub deletion_url: Option<CheckedOutput>,
    /// Notes.
    pub warnings: Vec<UploadWarning>,
    /// A few response headers (`content-type`, `content-length`, `retry-after`, `location` without
    /// query).
    pub headers: Vec<HeaderLine>,
}

/// The engine.
pub struct Uploader<'a> {
    secrets: &'a dyn SecretSource,
    http: HttpClient,
    log: DebugLog,
}

struct Resolved {
    values: HashMap<String, SecretValue>,
    /// Every place a loaded secret is allowed in (normalised): a `--var` may not feed any of them.
    placements: Vec<Placement>,
    warnings: Vec<UploadWarning>,
}

struct Answer {
    status: u16,
    headers: HeaderMap,
    body: Vec<u8>,
    url: Url,
}

fn normalized(p: &Placement) -> Placement {
    match p {
        Placement::Header(n) => Placement::header(n),
        other => other.clone(),
    }
}

/// Values of the variables of a run: what the user gave, then the defaults.
struct Vars<'a> {
    given: &'a [(String, String)],
    compiled: &'a Compiled,
}

impl Vars<'_> {
    /// `(value, came_from_the_user)`.
    fn get(&self, name: &str) -> Result<(String, bool), UploadError> {
        let decl = self.compiled.vars.get(name);
        if let Some((_, v)) = self.given.iter().rev().find(|(n, _)| n == name) {
            if let Some(decl) = decl
                && !decl.choices.is_empty()
                && !decl.choices.contains(v)
            {
                return Err(UploadError::Variable {
                    reason: "invalid_variable",
                    detail: name.to_owned(),
                });
            }
            return Ok((v.clone(), true));
        }
        decl.and_then(|d| d.default.clone())
            .map(|d| (d, false))
            .ok_or_else(|| UploadError::Variable {
                reason: "missing_variable",
                detail: name.to_owned(),
            })
    }
}

struct RequestEnv<'a> {
    filename: &'a str,
    secrets: &'a HashMap<String, SecretValue>,
    vars: &'a Vars<'a>,
    /// The slot is a place of credentials: a variable may not feed it (`--var`, code 11). It is when
    /// it is named like one, **or** holds a `{secret:}` itself (the body included), **or** is a
    /// place a loaded secret is allowed in.
    sensitive_slot: bool,
}

impl Env for RequestEnv<'_> {
    fn call(
        &self,
        function: Function,
        args: &[String],
        _regex: Option<&(String, Group)>,
    ) -> Result<String, UploadError> {
        match function {
            Function::Filename => Ok(self.filename.to_owned()),
            Function::Secret => self
                .secrets
                .get(&args[0])
                .map(|s| s.expose().to_owned())
                .ok_or_else(|| UploadError::SecretMissing {
                    name: args[0].clone(),
                }),
            Function::Var => {
                let (value, from_user) = self.vars.get(&args[0])?;
                if from_user && self.sensitive_slot {
                    return Err(UploadError::security(
                        "secret_in_argument",
                        format!(
                            "the variable {:?} feeds a slot of credentials: use {{secret:}}",
                            args[0]
                        ),
                    ));
                }
                Ok(value)
            }
            _ => Err(UploadError::config("function not allowed in a request")),
        }
    }
}

/// What the request carries, secrets resolved.
struct Plan {
    query: Vec<(String, String)>,
    headers: Vec<(HeaderName, HeaderValue)>,
    fields: Vec<(String, String)>,
    body_text: Option<String>,
}

fn render_slot(
    template: &Template,
    slot_name: &str,
    placement: &Placement,
    input: &UploadInput,
    resolved: &Resolved,
    vars: &Vars<'_>,
) -> Result<(String, bool), UploadError> {
    let env = RequestEnv {
        filename: &input.name,
        secrets: &resolved.values,
        vars,
        sensitive_slot: is_sensitive_name(slot_name)
            || !template.secrets().is_empty()
            || resolved.placements.contains(placement),
    };
    let text = template.render(&env)?;
    let carries_secret = !template.secrets().is_empty();
    if carries_secret {
        // `Bearer <secret>` and `base64(user:<secret>)` are as good as the secret for the detector.
        register_secret(&text);
    }
    Ok((text, carries_secret))
}

impl<'a> Uploader<'a> {
    /// The engine, with the one HTTP client.
    ///
    /// # Errors
    /// See [`HttpClient::new`].
    pub fn new(secrets: &'a dyn SecretSource) -> Result<Self, UploadError> {
        Ok(Self {
            secrets,
            http: HttpClient::new()?,
            log: DebugLog::new(0),
        })
    }

    /// Keeps a redacted journal of the exchange, up to `level` (1: `-v`, 2: `-vv`).
    #[must_use]
    pub fn with_debug_log(mut self, level: u8) -> Self {
        self.log = DebugLog::new(level);
        self
    }

    /// The lines of the journal so far, as `(level, text)`; each one went through the detector.
    #[must_use]
    pub fn take_log(&self) -> Vec<(u8, String)> {
        self.log.take()
    }

    /// The engine on a given client (tests only: a client that trusts the CA of a test server).
    #[cfg(test)]
    pub(crate) fn with_client(secrets: &'a dyn SecretSource, http: HttpClient) -> Self {
        Self {
            secrets,
            http,
            log: DebugLog::new(0),
        }
    }

    /// Sends `input` to `destination`.
    ///
    /// `vars` are the `--var NAME=VALUE` of the command line.
    ///
    /// # Errors
    /// See [`UploadError`]; nothing in it holds a secret.
    pub async fn upload(
        &self,
        destination: &Destination,
        input: &UploadInput,
        vars: &[(String, String)],
    ) -> Result<UploadOutcome, UploadError> {
        let result = self.upload_inner(destination, input, vars).await;
        match &result {
            Ok(outcome) => self
                .log
                .record(1, &format!("upload done: HTTP {}", outcome.status)),
            Err(e) => self
                .log
                .record(1, &format!("upload failed ({}): {e}", e.reason())),
        }
        result
    }

    async fn upload_inner(
        &self,
        destination: &Destination,
        input: &UploadInput,
        vars: &[(String, String)],
    ) -> Result<UploadOutcome, UploadError> {
        let compiled = destination.compile()?;
        self.log.record(
            1,
            &format!(
                "upload: {} to {} ({} bytes, {})",
                compiled.method,
                compiled.origin().unwrap_or_default(),
                input.bytes.len(),
                input.mime,
            ),
        );
        if input.bytes.len() as u64 > compiled.max_size {
            return Err(UploadError::failed(
                "file_too_large",
                "the file is larger than max_size",
            ));
        }
        let accepted = compiled.kinds.contains(&MediaKind::File)
            || (input.is_image() && compiled.kinds.contains(&MediaKind::Image));
        if !accepted {
            return Err(UploadError::config(
                "this destination does not accept this kind of file",
            ));
        }
        let var_source = Vars {
            given: vars,
            compiled: &compiled,
        };
        let resolved = self.resolve_secrets(&compiled)?;
        let plan = build_plan(&compiled, input, &resolved, &var_source)?;
        let mut known = SecretSet::loaded();
        for value in resolved.values.values() {
            known.add(value.expose());
        }
        let answer = tokio::time::timeout(compiled.timeout, self.send(&compiled, &plan, input))
            .await
            .map_err(|_| UploadError::Network {
                reason: "timeout",
                detail: "the server did not answer in time".to_owned(),
            })??;
        interpret(
            &compiled,
            &answer,
            input,
            &var_source,
            &known,
            resolved.warnings,
        )
    }

    fn resolve_secrets(&self, compiled: &Compiled) -> Result<Resolved, UploadError> {
        let slots = compiled.secret_slots();
        let mut resolved = Resolved {
            values: HashMap::new(),
            placements: Vec::new(),
            warnings: Vec::new(),
        };
        if slots.is_empty() {
            return Ok(resolved);
        }
        // Transport first: no secret is even read for a request that could not carry it.
        // No exception, not even for the loopback (decision U3): the tests use real HTTPS.
        if compiled.url.scheme() != "https" {
            return Err(UploadError::security(
                "https_required",
                "a secret is used: the address must be https://",
            ));
        }
        let origin = compiled
            .origin()
            .ok_or_else(|| UploadError::config("url: not a valid address"))?;
        for slot in &slots {
            if resolved.values.contains_key(&slot.name) {
                continue;
            }
            let entry = self
                .secrets
                .load(&compiled.id, &slot.name)
                .map_err(UploadError::from)?
                .ok_or_else(|| UploadError::SecretMissing {
                    name: slot.name.clone(),
                })?;
            if !entry
                .origins
                .iter()
                .any(|o| canonical_origin(o).as_deref() == Some(origin.as_str()))
            {
                return Err(UploadError::security(
                    "origin_not_allowed",
                    format!("the secret {:?} is not allowed for this origin", slot.name),
                ));
            }
            if entry.value.len() < SHORT_SECRET_LEN {
                resolved.warnings.push(UploadWarning {
                    code: "secret_short",
                    detail: slot.name.clone(),
                });
            }
            register_secret(entry.value.expose());
            let allowed: Vec<Placement> = entry.placements.iter().map(normalized).collect();
            // Every use of this secret in the request must be an allowed place.
            for other in slots.iter().filter(|s| s.name == slot.name) {
                if !allowed.contains(&other.placement) {
                    return Err(UploadError::security(
                        "placement_not_allowed",
                        format!(
                            "the secret {:?} is not allowed in {}",
                            slot.name, other.placement
                        ),
                    ));
                }
            }
            resolved.placements.extend(allowed);
            resolved.values.insert(slot.name.clone(), entry.value);
        }
        Ok(resolved)
    }

    fn request(
        &self,
        compiled: &Compiled,
        plan: &Plan,
        input: &UploadInput,
        method: &Method,
        url: &Url,
        with_body: bool,
    ) -> Result<RequestBuilder, UploadError> {
        let mut builder = self
            .http
            .inner
            .request(method.clone(), url.clone())
            .timeout(compiled.timeout);
        for (name, value) in &plan.headers {
            builder = builder.header(name, value);
        }
        if !with_body {
            return Ok(builder);
        }
        let bad_file = |_| UploadError::config("the media type of the file is not valid");
        Ok(match compiled.body {
            BodyKind::Multipart => {
                let part = Part::bytes(input.bytes.clone())
                    .file_name(input.name.clone())
                    .mime_str(&input.mime)
                    .map_err(bad_file)?;
                let mut form = Form::new();
                for (k, v) in &plan.fields {
                    form = form.text(k.clone(), v.clone());
                }
                builder.multipart(form.part(compiled.file_field.clone(), part))
            }
            BodyKind::Form => {
                let body = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(&plan.fields)
                    .finish();
                builder
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(body)
            }
            BodyKind::Json | BodyKind::Xml => {
                let kind = if compiled.body == BodyKind::Json {
                    "application/json"
                } else {
                    "application/xml"
                };
                let has_type = plan.headers.iter().any(|(n, _)| n == CONTENT_TYPE);
                let builder = if has_type {
                    builder
                } else {
                    builder.header(CONTENT_TYPE, kind)
                };
                builder.body(plan.body_text.clone().unwrap_or_default())
            }
            BodyKind::Binary => builder
                .header(CONTENT_TYPE, input.mime.clone())
                .body(input.bytes.clone()),
            BodyKind::None => builder,
        })
    }

    async fn send(
        &self,
        compiled: &Compiled,
        plan: &Plan,
        input: &UploadInput,
    ) -> Result<Answer, UploadError> {
        let mut method = Method::from_bytes(compiled.method.as_bytes())
            .map_err(|_| UploadError::config("method: not valid"))?;
        let mut url = compiled.url.clone();
        url.query_pairs_mut().clear().extend_pairs(&plan.query);
        if plan.query.is_empty() {
            url.set_query(None);
        }
        let first_origin = origin_of(&url);
        let mut with_body = true;
        let mut hops = 0usize;
        loop {
            let builder = self.request(compiled, plan, input, &method, &url, with_body)?;
            self.log
                .record(2, &format!("request: {method} {}", redact_url(&url)));
            self.log.record(
                2,
                &format!(
                    "request headers: {}",
                    redact_headers(plan.headers.iter().map(|(n, v)| (n, v)))
                ),
            );
            self.log.record(
                2,
                &format!(
                    "request body: {:?} (content not logged)",
                    if with_body {
                        compiled.body
                    } else {
                        BodyKind::None
                    }
                ),
            );
            let response = builder.send().await.map_err(|e| network_error(&e))?;
            let status = response.status();
            self.log
                .record(1, &format!("response: HTTP {}", status.as_u16()));
            self.log.record(
                2,
                &format!(
                    "response headers: {}",
                    redact_header_map(response.headers())
                ),
            );
            if status.is_redirection()
                && compiled.redirects == Redirects::SameOrigin
                && let Some(location) = response.headers().get(LOCATION)
            {
                hops += 1;
                if hops > MAX_REDIRECTS {
                    return Err(UploadError::failed("bad_response", "too many redirections"));
                }
                let next = location
                    .to_str()
                    .ok()
                    .and_then(|l| url.join(l).ok())
                    .ok_or_else(|| UploadError::failed("bad_response", "invalid redirection"))?;
                if origin_of(&next) != first_origin {
                    return Err(UploadError::security(
                        "cross_origin_redirect",
                        "the server redirected to another origin",
                    ));
                }
                if !matches!(status.as_u16(), 307 | 308) {
                    // As browsers do: the request becomes a GET without a body.
                    method = Method::GET;
                    with_body = false;
                }
                self.log.record(
                    2,
                    &format!("redirect {} to {}", status.as_u16(), redact_url(&next)),
                );
                url = next;
                continue;
            }
            let (headers, final_url, code) = (
                response.headers().clone(),
                response.url().clone(),
                status.as_u16(),
            );
            let mut response = response;
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| network_error(&e))? {
                if body.len() + chunk.len() > MAX_RESPONSE {
                    return Err(UploadError::failed(
                        "response_too_large",
                        "the answer of the server is larger than 1 MiB",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            self.log
                .record(2, &format!("response body: {} bytes", body.len()));
            if !status.is_success() && self.log.enabled(2) {
                // Only an error body, masked on the whole text and then cut; the body of a success
                // holds the URL of deletion, which no journal gets.
                let excerpt = MaskedText::from_response(&body, &SecretSet::loaded(), BODY_LIMIT);
                self.log
                    .record(2, &format!("response body (error): {excerpt}"));
            }
            return Ok(Answer {
                status: code,
                headers,
                body,
                url: final_url,
            });
        }
    }
}

fn build_plan(
    compiled: &Compiled,
    input: &UploadInput,
    resolved: &Resolved,
    vars: &Vars<'_>,
) -> Result<Plan, UploadError> {
    let mut plan = Plan {
        query: Vec::new(),
        headers: Vec::new(),
        fields: Vec::new(),
        body_text: None,
    };
    for (name, template) in &compiled.query {
        plan.query.push((
            name.clone(),
            render_slot(
                template,
                name,
                &Placement::Query(name.clone()),
                input,
                resolved,
                vars,
            )?
            .0,
        ));
    }
    for (name, template) in &compiled.headers {
        let (text, secret) = render_slot(
            template,
            name,
            &Placement::header(name),
            input,
            resolved,
            vars,
        )?;
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| UploadError::config(format!("header {name:?} is not valid")))?;
        let mut value = HeaderValue::from_str(&text).map_err(|_| {
            UploadError::config(format!("the value of the header {name:?} is not valid"))
        })?;
        value.set_sensitive(secret || is_sensitive_name(name));
        plan.headers.push((header_name, value));
    }
    for (name, template) in &compiled.fields {
        plan.fields.push((
            name.clone(),
            render_slot(
                template,
                name,
                &Placement::Form(name.clone()),
                input,
                resolved,
                vars,
            )?
            .0,
        ));
    }
    if let Some(template) = &compiled.body_template {
        plan.body_text = Some(
            render_slot(
                template,
                "body",
                &Placement::body(template.source()),
                input,
                resolved,
                vars,
            )?
            .0,
        );
    }
    Ok(plan)
}

// ---------------------------------------------------------------------------------------------
// The answer
// ---------------------------------------------------------------------------------------------

struct ResponseEnv<'a> {
    answer: &'a Answer,
    input: &'a UploadInput,
    vars: &'a Vars<'a>,
    json: OnceCell<Result<Value, ()>>,
}

impl ResponseEnv<'_> {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.answer.body).into_owned()
    }

    fn body_json(&self) -> Result<&Value, UploadError> {
        self.json
            .get_or_init(|| serde_json::from_slice(&self.answer.body).map_err(|_| ()))
            .as_ref()
            .map_err(|()| UploadError::failed("bad_response", "the answer is not JSON"))
    }

    /// The URL of the answer without query, fragment or `user:pass@`.
    fn response_url(&self) -> String {
        let mut url = self.answer.url.clone();
        url.set_query(None);
        url.set_fragment(None);
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.to_string()
    }
}

fn json_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

impl Env for ResponseEnv<'_> {
    fn call(
        &self,
        function: Function,
        args: &[String],
        regex: Option<&(String, Group)>,
    ) -> Result<String, UploadError> {
        match function {
            Function::Filename => Ok(self.input.name.clone()),
            Function::Var => Ok(self.vars.get(&args[0])?.0),
            Function::Response => Ok(self.text()),
            Function::ResponseUrl => Ok(self.response_url()),
            Function::Header => Ok(self
                .answer
                .headers
                .get(args[0].as_str())
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()),
            Function::Json => {
                let path = normalize_json_path(args.last().map_or("", String::as_str));
                let parsed;
                let value = if args.len() == 2 {
                    parsed = serde_json::from_str::<Value>(&args[0]).map_err(|_| {
                        UploadError::failed("bad_response", "the input of {json:} is not JSON")
                    })?;
                    &parsed
                } else {
                    self.body_json()?
                };
                let found = JsonPath::parse(&path).map_err(|_| {
                    UploadError::failed("bad_response", "the JSON path is not valid")
                })?;
                Ok(json_text(found.query(value).first()))
            }
            Function::Regex => {
                let (pattern, group) = regex.ok_or_else(|| UploadError::config("regex"))?;
                let re = RegexBuilder::new(pattern)
                    .size_limit(REGEX_SIZE_LIMIT)
                    .dfa_size_limit(REGEX_SIZE_LIMIT * 2)
                    .build()
                    .map_err(|_| {
                        UploadError::config("a {regex:} pattern is invalid or too large")
                    })?;
                let text = self.text();
                let found = re.captures(&text).and_then(|c| match group {
                    Group::Index(i) => c.get(*i),
                    Group::Name(n) => c.name(n),
                });
                Ok(found.map_or_else(String::new, |m| m.as_str().to_owned()))
            }
            Function::Secret | Function::Base64 => Err(UploadError::config(
                "function not allowed in a response template",
            )),
        }
    }
}

/// Characters that make a URL taken from a response unusable: controls, bidirectional overrides
/// and zero-width characters (`docs/security/secrets.md` section 4.4).
pub(crate) fn is_hostile_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

fn validate_extracted_url(text: &str) -> Result<Url, UploadError> {
    let invalid =
        || UploadError::failed("url_invalid", "the URL read from the answer is not usable");
    let text = text.trim();
    if text.chars().any(is_hostile_char) {
        return Err(invalid());
    }
    let url = Url::parse(text).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(url)
}

/// The text read from the answer is checked as the server wrote it **and** once the URL parser has
/// normalised it (which turns a `\` into `/`, hiding a JSON-escaped secret from a check made on the
/// result alone).
fn check_output(raw: &str, url: &Url, known: &SecretSet) -> Result<CheckedOutput, UploadError> {
    let refused = |_| {
        UploadError::security(
            "secret_in_output",
            "the answer of the server holds the secret: the link is withheld",
        )
    };
    CheckedOutput::check(raw, known).map_err(refused)?;
    CheckedOutput::check(url.as_str(), known).map_err(refused)
}

fn kept_headers(headers: &HeaderMap, known: &SecretSet) -> Vec<HeaderLine> {
    let mut lines = Vec::new();
    for name in ["content-type", "content-length", "retry-after", "location"] {
        let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) else {
            continue;
        };
        let value = if name == "location" {
            // Without query or fragment: they may carry a token.
            value.split(['?', '#']).next().unwrap_or_default()
        } else {
            value
        };
        lines.push(HeaderLine {
            name: name.to_owned(),
            value: MaskedText::from_response(value.as_bytes(), known, 200),
        });
    }
    lines
}

fn retry_of(status: u16) -> Retry {
    if matches!(status, 408 | 429) || (500..=599).contains(&status) {
        Retry::Transient
    } else {
        Retry::Final
    }
}

fn interpret(
    compiled: &Compiled,
    answer: &Answer,
    input: &UploadInput,
    vars: &Vars<'_>,
    known: &SecretSet,
    mut warnings: Vec<UploadWarning>,
) -> Result<UploadOutcome, UploadError> {
    let env = ResponseEnv {
        answer,
        input,
        vars,
        json: OnceCell::new(),
    };
    if !compiled.response.success_status.contains(&answer.status) {
        // The `error` template when it gives something, else the start of the body. Either way the
        // secret is masked on the whole text before it is cut.
        let from_template = compiled
            .response
            .error
            .as_ref()
            .and_then(|t| t.render(&env).ok())
            .filter(|s| !s.trim().is_empty());
        let source = from_template.unwrap_or_else(|| env.text());
        return Err(UploadError::Server {
            status: answer.status,
            message: MaskedText::from_response(
                source.as_bytes(),
                known,
                crate::engine::MESSAGE_LIMIT,
            ),
            retry: retry_of(answer.status),
        });
    }
    let url_text = compiled.response.url.render(&env)?;
    if url_text.trim().is_empty() {
        return Err(UploadError::failed(
            "url_missing",
            "the answer holds no URL (check the response template of the destination)",
        ));
    }
    let url = check_output(&url_text, &validate_extracted_url(&url_text)?, known)?;
    let mut optional = |template: &Option<Template>,
                        code: &'static str|
     -> Result<Option<CheckedOutput>, UploadError> {
        let Some(template) = template else {
            return Ok(None);
        };
        let text = template.render(&env)?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        if let Ok(parsed) = validate_extracted_url(&text) {
            check_output(&text, &parsed, known).map(Some)
        } else {
            warnings.push(UploadWarning {
                code,
                detail: String::new(),
            });
            Ok(None)
        }
    };
    let thumbnail_url = optional(&compiled.response.thumbnail_url, "thumbnail_url_invalid")?;
    let deletion_url = optional(&compiled.response.deletion_url, "deletion_url_invalid")?;
    Ok(UploadOutcome {
        status: answer.status,
        url,
        thumbnail_url,
        deletion_url,
        warnings,
        headers: kept_headers(&answer.headers, known),
    })
}
