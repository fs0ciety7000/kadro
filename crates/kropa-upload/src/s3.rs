// SPDX-License-Identifier: GPL-3.0-or-later

//! S3 and compatible services (`docs/uploads.md` section 1.2, `docs/security/secrets.md` section 4.6).
//!
//! `object_store` does the SigV4 signing and the `PutObject`; everything around it is ours:
//!
//! - **the one HTTP client**: `object_store` is built with `aws-base` and `reqwest` and **without** its
//!   `aws` feature (which would turn on `reqwest/rustls` and a second TLS stack). Its connector hands it
//!   the client of [`crate::http`] (same `aws-lc-rs`, same system verifier, no proxy, no redirect), and its
//!   hashes and HMAC come from our own [`CryptoProvider`] on the `aws-lc-rs` that `rustls` already uses;
//! - **static credentials only**: the builder is made with [`AmazonS3Builder::new`], never `from_env`, and
//!   only with the access key id of the file and the secret access key of the store. No `AWS_*` variable,
//!   profile, IMDS or web identity is ever read; [`ObjectStoreBackend::new`] needs the secret as an argument,
//!   so there is no way to build the store before the key is loaded;
//! - **no `x-amz-acl`**, a single `PutObject` (multipart is lot 4.5), no retry (the queue of lot 4.5 owns
//!   that), no checksum header;
//! - the link is `public_base_url` + the encoded key, or a presigned `GET` of at most seven days.

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use aws_lc_rs::{digest, hmac};
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::client::{
    ClientOptions, CryptoProvider, DigestAlgorithm, DigestContext, HmacContext, HttpClient,
    HttpConnector, HttpError, HttpRequest, HttpResponse, HttpService, Signer as RsaSigner,
    SigningAlgorithm,
};
use object_store::path::Path;
use object_store::signer::Signer;
use object_store::{
    Attribute, AttributeValue, Attributes, ObjectStore, PutOptions, PutPayload, RetryConfig,
};
use url::Url;

use crate::error::UploadError;
use crate::http::{HttpClient as OurClient, network_error};
use crate::masked::{MaskedText, SecretSet};
use crate::secret::SecretValue;

/// Longest validity of a presigned URL (the limit of SigV4).
pub const MAX_PRESIGN: Duration = Duration::from_hours(7 * 24);

/// Name of the one secret of an S3 destination.
pub const SECRET_NAME: &str = "secret_access_key";

/// Everything the engine knows about an S3 destination, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Settings {
    /// Address of the service, without the bucket; `None` is AWS.
    pub endpoint: Option<Url>,
    /// Region used by the signature.
    pub region: String,
    /// Bucket.
    pub bucket: String,
    /// Path style (`https://host/bucket/key`) instead of virtual host (`https://bucket.host/key`).
    pub path_style: bool,
    /// Prefix of every key, without a leading or trailing `/`; may be empty.
    pub prefix: String,
    /// Where the objects are public, for the link.
    pub public_base_url: Option<Url>,
    /// Validity of a presigned link, instead of `public_base_url`.
    pub presign: Option<Duration>,
    /// Identifier of the key: not a secret (it is in every signed request and in presigned URLs).
    pub access_key_id: String,
}

impl S3Settings {
    /// The origin a secret is bound to: the triplet `s3:<https://host:port>|<region>|<bucket>`
    /// (`docs/security/secrets.md` section 4.6). Changing one element is another origin.
    #[must_use]
    pub fn origin(&self) -> String {
        format!(
            "s3:{}|{}|{}",
            crate::origin::origin_of(&self.endpoint_url()).unwrap_or_default(),
            self.region,
            self.bucket
        )
    }

    /// The address of the service: the endpoint, or the regional endpoint of AWS.
    #[must_use]
    pub fn endpoint_url(&self) -> Url {
        self.endpoint.clone().unwrap_or_else(|| {
            Url::parse(&format!("https://s3.{}.amazonaws.com", self.region))
                .unwrap_or_else(|_| Url::parse("https://s3.amazonaws.com").expect("constant"))
        })
    }

    /// The key of an object: `prefix/name`, the name without any `/`, control character, `.` or `..`.
    ///
    /// # Errors
    /// [`UploadError::Failed`] (`invalid_key`) when the result is not a key `object_store` accepts.
    pub fn object_key(&self, name: &str) -> Result<String, UploadError> {
        let name: String = name
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, '/' | '\\') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let name = if matches!(name.as_str(), "" | "." | "..") {
            "file".to_owned()
        } else {
            name
        };
        let key = if self.prefix.is_empty() {
            name
        } else {
            format!("{}/{name}", self.prefix)
        };
        // 1024 bytes is the limit of S3.
        if key.len() > 1024 || Path::parse(&key).is_err() {
            return Err(UploadError::failed(
                "invalid_key",
                "the name of the file cannot be used as an S3 key",
            ));
        }
        Ok(key)
    }

    /// The public address of `key`: `public_base_url` and the encoded key.
    ///
    /// # Errors
    /// [`UploadError::Config`] when the destination has no `public_base_url`.
    pub fn public_url(&self, key: &str) -> Result<Url, UploadError> {
        let base = self.public_base_url.as_ref().ok_or_else(|| {
            UploadError::config("an s3 destination needs public_base_url or presign")
        })?;
        let text = format!(
            "{}/{}",
            base.as_str().trim_end_matches('/'),
            encode_key(key)
        );
        Url::parse(&text).map_err(|_| UploadError::config("public_base_url: not a valid address"))
    }
}

/// The encoding S3 uses for a key in a path: everything but unreserved characters and `/`, byte by byte
/// on the UTF-8 text.
fn encode_key(key: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// What the engine needs from the S3 side; implemented by [`ObjectStoreBackend`] and, in tests, by a fake.
#[async_trait]
pub trait S3Backend: Send + Sync {
    /// One `PutObject` of `body` at `key`, with `content_type`.
    ///
    /// # Errors
    /// [`UploadError::Server`] (the status of the service, the body masked), [`UploadError::Network`] or
    /// [`UploadError::Failed`].
    async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), UploadError>;

    /// A presigned `GET` of `key`, valid for `ttl` (at most [`MAX_PRESIGN`]). It holds the access key id
    /// and a signature, never the secret key.
    ///
    /// # Errors
    /// [`UploadError::Config`] when `ttl` is zero or longer than seven days.
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url, UploadError>;
}

/// Hashes and HMAC of `object_store`, on `aws-lc-rs` (the provider of our `rustls`). RSA signing (Google
/// service accounts) is not needed and refused.
#[derive(Debug)]
struct AwsLcProvider;

struct Digest(Option<digest::Context>, Option<digest::Digest>);
struct Hmac(Option<hmac::Context>, Option<hmac::Tag>);

impl DigestContext for Digest {
    fn update(&mut self, data: &[u8]) {
        if let Some(ctx) = self.0.as_mut() {
            ctx.update(data);
        }
    }

    fn finish(&mut self) -> object_store::Result<&[u8]> {
        if let Some(ctx) = self.0.take() {
            self.1 = Some(ctx.finish());
        }
        self.1
            .as_ref()
            .map(AsRef::as_ref)
            .ok_or_else(|| crypto_error("digest finished twice"))
    }
}

impl HmacContext for Hmac {
    fn update(&mut self, data: &[u8]) {
        if let Some(ctx) = self.0.as_mut() {
            ctx.update(data);
        }
    }

    fn finish(&mut self) -> object_store::Result<&[u8]> {
        if let Some(ctx) = self.0.take() {
            self.1 = Some(ctx.sign());
        }
        self.1
            .as_ref()
            .map(AsRef::as_ref)
            .ok_or_else(|| crypto_error("hmac finished twice"))
    }
}

fn crypto_error(what: &str) -> object_store::Error {
    object_store::Error::Generic {
        store: "S3",
        source: what.to_owned().into(),
    }
}

impl CryptoProvider for AwsLcProvider {
    fn digest(&self, algorithm: DigestAlgorithm) -> object_store::Result<Box<dyn DigestContext>> {
        match algorithm {
            DigestAlgorithm::Sha256 => Ok(Box::new(Digest(
                Some(digest::Context::new(&digest::SHA256)),
                None,
            ))),
            _ => Err(crypto_error("digest not supported")),
        }
    }

    fn hmac(
        &self,
        algorithm: DigestAlgorithm,
        secret: &[u8],
    ) -> object_store::Result<Box<dyn HmacContext>> {
        match algorithm {
            DigestAlgorithm::Sha256 => Ok(Box::new(Hmac(
                Some(hmac::Context::with_key(&hmac::Key::new(
                    hmac::HMAC_SHA256,
                    secret,
                ))),
                None,
            ))),
            _ => Err(crypto_error("hmac not supported")),
        }
    }

    fn sign(
        &self,
        _algorithm: SigningAlgorithm,
        _pem: &[u8],
    ) -> object_store::Result<Box<dyn RsaSigner>> {
        Err(crypto_error("signing is not used by S3"))
    }
}

/// The last HTTP status the service answered: `object_store` does not give it back in a usable form.
#[derive(Debug, Default)]
struct StatusProbe(AtomicU16);

/// The client of `object_store`: ours, plus a note of the status of each answer.
#[derive(Debug)]
struct Service {
    client: reqwest::Client,
    probe: Arc<StatusProbe>,
}

#[async_trait]
impl HttpService for Service {
    async fn call(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        self.probe.0.store(0, Ordering::Relaxed);
        let response = HttpService::call(&self.client, req).await?;
        self.probe
            .0
            .store(response.status().as_u16(), Ordering::Relaxed);
        Ok(response)
    }
}

/// Gives `object_store` the one client of the program instead of letting it build its own.
#[derive(Debug)]
struct Connector {
    client: reqwest::Client,
    probe: Arc<StatusProbe>,
}

impl HttpConnector for Connector {
    fn connect(&self, _options: &ClientOptions) -> object_store::Result<HttpClient> {
        Ok(HttpClient::new(Service {
            client: self.client.clone(),
            probe: Arc::clone(&self.probe),
        }))
    }
}

/// `object_store` on static credentials and the one HTTP client.
pub struct ObjectStoreBackend {
    store: object_store::aws::AmazonS3,
    probe: Arc<StatusProbe>,
    known: SecretSet,
}

impl std::fmt::Debug for ObjectStoreBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectStoreBackend").finish_non_exhaustive()
    }
}

impl ObjectStoreBackend {
    /// Builds the store. The secret is an argument: nothing is built without a key, and no other source
    /// of credentials exists here.
    ///
    /// # Errors
    /// [`UploadError::Config`] when `object_store` refuses the settings.
    pub fn new(
        settings: &S3Settings,
        secret: &SecretValue,
        http: &OurClient,
    ) -> Result<Self, UploadError> {
        let probe = Arc::new(StatusProbe::default());
        let connector = Connector {
            client: http.inner.clone(),
            probe: Arc::clone(&probe),
        };
        let mut builder = AmazonS3Builder::new()
            .with_region(&settings.region)
            .with_bucket_name(&settings.bucket)
            .with_access_key_id(&settings.access_key_id)
            .with_secret_access_key(secret.expose())
            .with_virtual_hosted_style_request(!settings.path_style)
            .with_allow_http(false)
            .with_http_connector(connector)
            .with_crypto_provider(Arc::new(AwsLcProvider))
            .with_retry(RetryConfig {
                max_retries: 0,
                ..RetryConfig::default()
            });
        if let Some(endpoint) = &settings.endpoint {
            builder = builder.with_endpoint(endpoint_text(endpoint, settings));
        }
        let store = builder.build().map_err(|_| {
            UploadError::config("the S3 settings are not valid (endpoint, region or bucket)")
        })?;
        Ok(Self {
            store,
            probe,
            known: SecretSet::from_values(&[secret.expose()]),
        })
    }

    /// An error of `object_store`, as the engine reports it: the network error of the client, or the
    /// status of the service with a masked text; never the text of the error as it is.
    fn error(&self, e: &object_store::Error) -> UploadError {
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
        let mut text = String::new();
        while let Some(err) = source {
            if let Some(net) = err.downcast_ref::<reqwest::Error>() {
                return network_error(net);
            }
            if !text.is_empty() {
                text.push_str(": ");
            }
            text.push_str(&err.to_string());
            source = err.source();
        }
        let status = self.probe.0.load(Ordering::Relaxed);
        if !(300..=599).contains(&status) {
            // No answer, or a success `object_store` could not read (no `ETag`).
            return UploadError::failed("bad_response", "the S3 service could not be used");
        }
        let mut known = SecretSet::loaded();
        known.extend(&self.known);
        UploadError::Server {
            status,
            message: MaskedText::from_response(
                text.as_bytes(),
                &known,
                crate::engine::MESSAGE_LIMIT,
            ),
            retry: crate::engine::retry_of(status),
        }
    }
}

/// The endpoint `object_store` is given: the address of the service, with the bucket in the host for
/// the virtual-host style (`object_store` takes a custom endpoint as complete in that style).
fn endpoint_text(endpoint: &Url, settings: &S3Settings) -> String {
    let mut url = endpoint.clone();
    if !settings.path_style
        && let Some(host) = url.host_str().map(str::to_owned)
    {
        let _ = url.set_host(Some(&format!("{}.{host}", settings.bucket)));
    }
    url.as_str().trim_end_matches('/').to_owned()
}

#[async_trait]
impl S3Backend for ObjectStoreBackend {
    async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), UploadError> {
        let path = Path::parse(key)
            .map_err(|_| UploadError::failed("invalid_key", "the S3 key is not valid"))?;
        let mut attributes = Attributes::new();
        attributes.insert(
            Attribute::ContentType,
            AttributeValue::from(content_type.to_owned()),
        );
        let options = PutOptions {
            attributes,
            ..PutOptions::default()
        };
        self.store
            .put_opts(&path, PutPayload::from_bytes(body), options)
            .await
            .map(|_| ())
            .map_err(|e| self.error(&e))
    }

    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url, UploadError> {
        if ttl.is_zero() || ttl > MAX_PRESIGN {
            return Err(UploadError::config("presign: between 1 second and 7 days"));
        }
        let path = Path::parse(key)
            .map_err(|_| UploadError::failed("invalid_key", "the S3 key is not valid"))?;
        self.store
            .signed_url(reqwest::Method::GET, &path, ttl)
            .await
            .map_err(|_| UploadError::failed("presign_failed", "the link could not be signed"))
    }
}
