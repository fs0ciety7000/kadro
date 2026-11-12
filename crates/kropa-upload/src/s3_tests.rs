// SPDX-License-Identifier: GPL-3.0-or-later

//! The S3 engine (lot 2.7a): `object_store` on the one HTTP client, against `wiremock` servers behind a
//! real HTTPS listener on the loopback (`testing::Front`), as the other tests that carry a secret.
//! Garage, a real service, is exercised by `crates/kropa-cli/tests/s3_integration.rs`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::destination::{Destination, DestinationFile};
use crate::engine::{UploadInput, Uploader};
use crate::error::{Retry, UploadError};
use crate::s3::{S3Backend, S3Settings};
use crate::secret::{MemorySecretSource, Placement};
use crate::testing::Front;

/// Built at run time: nothing in the source looks like a key.
const SECRET: &str = concat!("sk-", "Zq7kLm9", "Xt2vRw-abcdef");
const KEY_ID: &str = concat!("GK", "test", "0123456789abcdef");

const TOML: &str = r#"
[[destination]]
id = "garage"
type = "s3"
endpoint = "BASE"
region = "garage"
bucket = "kropa-test"
path_style = true
prefix = "shots/"
public_base_url = "https://cdn.example.org/pub"
access_key_id = "KEYID"

[destination.secrets]
secret_access_key = {}
"#;

const PUBLIC: &str = "public_base_url = \"https://cdn.example.org/pub\"";

fn destination(base: &str, edit: impl Fn(String) -> String) -> Destination {
    let text = edit(TOML.replace("BASE", base).replace("KEYID", KEY_ID));
    let mut file = DestinationFile::parse(&text, "s3.toml").expect("valid destination");
    file.destination.remove(0)
}

fn presigned(base: &str, ttl: &str) -> Destination {
    destination(base, |t| t.replace(PUBLIC, &format!("presign = \"{ttl}\"")))
}

/// A store with the key of `dest`, bound to its origin.
fn store(dest: &Destination) -> MemorySecretSource {
    let store = MemorySecretSource::new();
    store
        .insert(
            &dest.id,
            "secret_access_key",
            SECRET,
            &[dest.origin().unwrap().as_str()],
            vec![Placement::Sigv4],
        )
        .unwrap();
    store
}

fn input() -> UploadInput {
    UploadInput::from_bytes("my shot #1.png", b"\x89PNG fake image".to_vec())
}

fn config_error(text: &str) -> String {
    match DestinationFile::parse(text, "s3.toml") {
        Err(UploadError::Config { detail, .. }) => detail,
        other => panic!("expected a config error, got {other:?}"),
    }
}

fn is_security(err: &UploadError, expected: &str) -> bool {
    matches!(err, UploadError::Security { reason, .. } if *reason == expected)
}

async fn put_ok(server: &Front) {
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"abc\""))
        .mount(server)
        .await;
}

#[tokio::test]
async fn put_object_is_signed_and_the_link_is_public_base_url_and_the_key() {
    let server = Front::start().await;
    Mock::given(method("PUT"))
        .and(path("/kropa-test/shots/my%20shot%20%231.png"))
        .and(|req: &Request| {
            let h = |n: &str| {
                req.headers
                    .get(n)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned()
            };
            let auth = h("authorization");
            auth.starts_with(&format!("AWS4-HMAC-SHA256 Credential={KEY_ID}/"))
                && auth.contains("/garage/s3/aws4_request")
                && auth.contains("Signature=")
                && !auth.contains(SECRET)
                && !h("x-amz-content-sha256").is_empty()
                && h("content-type") == "image/png"
                && !req.headers.contains_key("x-amz-acl")
                && !req.headers.contains_key("x-amz-checksum-crc32")
                && req.body == b"\x89PNG fake image"
        })
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"abc\""))
        .expect(1)
        .mount(&server)
        .await;
    let dest = destination(&server.uri(), |t| t);
    let store = store(&dest);
    let outcome = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(
        outcome.url.as_str(),
        "https://cdn.example.org/pub/shots/my%20shot%20%231.png"
    );
    assert_eq!(outcome.status, 200);
}

#[tokio::test]
async fn presigned_link_holds_the_key_id_and_never_the_secret() {
    let server = Front::start().await;
    put_ok(&server).await;
    let dest = presigned(&server.uri(), "90m");
    let store = store(&dest);
    let outcome = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .expect("the output check accepts the access key id of a presigned URL");
    let url = outcome.url.as_str();
    assert!(url.starts_with(&server.uri()), "{url}");
    assert!(url.contains(&format!("X-Amz-Credential={KEY_ID}")), "{url}");
    assert!(url.contains("X-Amz-Expires=5400"), "{url}");
    assert!(url.contains("X-Amz-Signature="), "{url}");
    assert!(!url.contains(SECRET), "{url}");
}

#[tokio::test]
async fn a_presigned_link_lasts_seven_days_at_most() {
    let seven = presigned("https://s3.example.org", "7d");
    assert!(seven.compile().is_ok());
    for bad in ["8d", "169h", "0s", "7days", "", "-1h"] {
        let text = TOML
            .replace("BASE", "https://s3.example.org")
            .replace("KEYID", KEY_ID)
            .replace(PUBLIC, &format!("presign = {bad:?}"));
        assert!(config_error(&text).contains("presign"), "{bad}");
    }
    // Also at the backend, whatever the file says.
    let server = Front::start().await;
    let settings = seven.s3_settings().unwrap();
    let secret = crate::secret::SecretValue::new(SECRET).unwrap();
    let http = crate::http::HttpClient::with_extra_root(server.ca_for_tests()).unwrap();
    let backend = crate::s3::ObjectStoreBackend::new(&settings, &secret, &http).unwrap();
    for ttl in [Duration::ZERO, Duration::from_secs(7 * 86_400 + 1)] {
        assert!(matches!(
            backend.presign_get("a.png", ttl).await,
            Err(UploadError::Config { .. })
        ));
    }
    let url = backend
        .presign_get("a.png", Duration::from_secs(7 * 86_400))
        .await
        .unwrap();
    assert!(url.as_str().contains("X-Amz-Expires=604800"), "{url}");
}

#[tokio::test]
async fn an_error_of_the_service_is_code_9_with_a_masked_body() {
    let server = Front::start().await;
    let body =
        format!("<Error><Code>AccessDenied</Code><Message>key {SECRET} refused</Message></Error>");
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(403).set_body_string(body))
        .mount(&server)
        .await;
    let dest = destination(&server.uri(), |t| t);
    let store = store(&dest);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    let UploadError::Server {
        status,
        message,
        retry,
    } = &err
    else {
        panic!("expected a server error, got {err:?}");
    };
    assert_eq!(*status, 403);
    assert_eq!(*retry, Retry::Final);
    assert!(message.as_str().contains("AccessDenied"), "{message}");
    assert!(!message.as_str().contains(SECRET), "{message}");
    assert!(!format!("{err:?}").contains(SECRET));
    // A 503 is worth another try.
    server.reset().await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        UploadError::Server {
            status: 503,
            retry: Retry::Transient,
            ..
        }
    ));
}

#[tokio::test]
async fn no_request_without_the_key_nor_over_plain_http_nor_for_another_bucket() {
    // Plain `http://` + secret: code 11 before the secret is even read.
    let plain = MockServer::start().await;
    let dest = destination(&plain.uri(), |t| t);
    let store_plain = store(&dest);
    let err = Uploader::new(&store_plain)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(is_security(&err, "https_required"), "{err:?}");
    assert!(plain.received_requests().await.unwrap().is_empty());

    let server = Front::start().await;
    put_ok(&server).await;
    // No key in the store: refused before anything is built or sent.
    let dest = destination(&server.uri(), |t| t);
    let empty = MemorySecretSource::new();
    let err = server
        .uploader(&empty)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, UploadError::SecretMissing { .. }), "{err:?}");
    // The key was stored for another bucket, or another region (the origin is a triplet): refused.
    let store = store(&dest);
    for other in [
        destination(&server.uri(), |t| t.replace("kropa-test", "other-bucket")),
        destination(&server.uri(), |t| {
            t.replace("region = \"garage\"", "region = \"eu-west-1\"")
        }),
    ] {
        let err = server
            .uploader(&store)
            .upload(&other, &input(), &[])
            .await
            .unwrap_err();
        assert!(is_security(&err, "origin_not_allowed"), "{err:?}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn the_origin_is_the_triplet_and_the_slot_is_sigv4() {
    let dest = destination("https://S3.Example.org:9000", |t| t);
    let compiled = dest.compile().unwrap();
    let expected = "s3:https://s3.example.org:9000|garage|kropa-test";
    assert_eq!(compiled.origin().as_deref(), Some(expected));
    assert_eq!(dest.origin().as_deref(), Some(expected));
    let slots = compiled.secret_slots();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].name, "secret_access_key");
    assert_eq!(slots[0].placement, Placement::Sigv4);
    // Written and read back by the stores.
    assert_eq!(Placement::Sigv4.to_string(), "sigv4:s3");
    assert_eq!("sigv4:s3".parse::<Placement>(), Ok(Placement::Sigv4));
    assert!("sigv4:other".parse::<Placement>().is_err());
    // No endpoint: AWS, by region.
    let aws = destination("x", |t| t.replace("endpoint = \"x\"\n", ""));
    assert_eq!(
        aws.origin().as_deref(),
        Some("s3:https://s3.garage.amazonaws.com:443|garage|kropa-test")
    );
}

#[test]
fn keys_hosts_and_links() {
    let dest = destination("https://s3.example.org", |t| t);
    let s = dest.s3_settings().unwrap();
    assert_eq!(s.prefix, "shots");
    assert_eq!(s.object_key("a b.png").unwrap(), "shots/a b.png");
    assert_eq!(s.object_key("../x/..").unwrap(), "shots/.._x_..");
    assert_eq!(s.object_key("..").unwrap(), "shots/file");
    assert_eq!(s.object_key("").unwrap(), "shots/file");
    assert_eq!(s.object_key("é\u{7}\\.png").unwrap(), "shots/é__.png");
    assert!(s.object_key(&"a".repeat(2000)).is_err());
    assert_eq!(
        s.public_url("shots/é & ü.png").unwrap().as_str(),
        "https://cdn.example.org/pub/shots/%C3%A9%20%26%20%C3%BC.png"
    );
    let bare: S3Settings = destination("https://s3.example.org", |t| {
        t.replace("prefix = \"shots/\"\n", "")
    })
    .s3_settings()
    .unwrap();
    assert_eq!(bare.object_key("a.png").unwrap(), "a.png");
}

#[test]
fn the_file_is_checked_when_it_is_loaded() {
    let base = |t: &str| {
        t.replace("BASE", "https://s3.example.org")
            .replace("KEYID", KEY_ID)
    };
    let key_line = format!("access_key_id = \"{KEY_ID}\"\n");
    for (from, to, needle) in [
        ("region = \"garage\"\n", "", "region"),
        ("region = \"garage\"", "region = \"a b\"", "region"),
        ("bucket = \"kropa-test\"\n", "", "bucket"),
        ("kropa-test", "Kropa_Test", "bucket"),
        ("kropa-test", "ab", "bucket"),
        (key_line.as_str(), "", "access_key_id"),
        (
            "endpoint = \"https://s3.example.org\"",
            "endpoint = \"ftp://x\"",
            "endpoint",
        ),
        (
            "endpoint = \"https://s3.example.org\"",
            "endpoint = \"https://u:p@x.org\"",
            "endpoint",
        ),
        (
            "endpoint = \"https://s3.example.org\"",
            "endpoint = \"https://x.org/bucket\"",
            "path",
        ),
        ("prefix = \"shots/\"", "prefix = \"a/../b\"", "prefix"),
        (
            "public_base_url = \"https://cdn.example.org/pub\"\n",
            "",
            "public_base_url or presign",
        ),
        (
            PUBLIC,
            "public_base_url = \"https://cdn.example.org/p?x=1\"",
            "public_base_url",
        ),
        ("secret_access_key = {}", "", "secret_access_key"),
        ("secret_access_key = {}", "token = {}", "secret_access_key"),
        (
            "type = \"s3\"",
            "type = \"s3\"\nurl = \"https://z.org\"",
            "no url",
        ),
    ] {
        let text = base(TOML).replace(from, to);
        let detail = config_error(&text);
        assert!(detail.contains(needle), "{from} -> {detail}");
    }
    // Both ways to make the link, or none.
    let both = base(TOML).replace("path_style = true", "path_style = true\npresign = \"1h\"");
    assert!(config_error(&both).contains("exclusive"));
    // A mix with the fields of another type.
    let mixed = base(TOML).replace("type = \"s3\"", "type = \"zipline\"");
    assert!(DestinationFile::parse(&mixed, "s3.toml").is_err());
    // The round trip keeps the file as it was written.
    let dest = destination("https://s3.example.org", |t| t);
    let again = DestinationFile::parse(&dest.to_toml().unwrap(), "s3.toml").unwrap();
    assert_eq!(again.destination[0], dest);
}

#[test]
fn a_secret_key_written_in_the_file_is_refused_without_being_quoted() {
    let base = TOML
        .replace("BASE", "https://s3.example.org")
        .replace("KEYID", KEY_ID);
    // As a value of the declaration, and as a key of the destination.
    for text in [
        base.replace(
            "secret_access_key = {}",
            &format!("secret_access_key = \"{SECRET}\""),
        ),
        base.replace(
            "path_style",
            &format!("secret_access_key = \"{SECRET}\"\npath_style"),
        ),
    ] {
        let detail = config_error(&text);
        assert!(!detail.contains(SECRET), "{detail}");
        assert!(!detail.contains("Zq7kLm9"), "{detail}");
    }
}

/// A backend that records what the engine asks of it.
struct Fake {
    calls: std::sync::Mutex<Vec<(String, usize, String)>>,
    fail: bool,
}

impl Fake {
    fn new(fail: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: std::sync::Mutex::default(),
            fail,
        })
    }
}

#[async_trait]
impl S3Backend for Fake {
    async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), UploadError> {
        self.calls
            .lock()
            .unwrap()
            .push((key.to_owned(), body.len(), content_type.to_owned()));
        if self.fail {
            Err(UploadError::failed("bad_response", "refused"))
        } else {
            Ok(())
        }
    }

    async fn presign_get(&self, key: &str, _ttl: Duration) -> Result<url::Url, UploadError> {
        Ok(url::Url::parse(&format!(
            "https://s3.example.org/{key}?X-Amz-Credential={KEY_ID}%2Fx&X-Amz-Signature=00"
        ))
        .unwrap())
    }
}

#[tokio::test]
async fn the_engine_runs_on_a_fake_backend() {
    let dest = destination("https://s3.example.org", |t| t);
    let store = store(&dest);
    let fake = Fake::new(false);
    let uploader = Uploader::new(&store)
        .unwrap()
        .with_s3_backend(Arc::clone(&fake) as Arc<dyn S3Backend>);
    let outcome = uploader.upload(&dest, &input(), &[]).await.unwrap();
    assert_eq!(
        outcome.url.as_str(),
        "https://cdn.example.org/pub/shots/my%20shot%20%231.png"
    );
    assert_eq!(
        fake.calls.lock().unwrap().as_slice(),
        [(
            "shots/my shot #1.png".to_owned(),
            15,
            "image/png".to_owned()
        )]
    );
    // A kind of file the destination refuses never reaches the backend.
    let images = destination("https://s3.example.org", |t| {
        t.replace("type = \"s3\"", "type = \"s3\"\nkinds = [\"image\"]")
    });
    let zip = UploadInput::from_bytes("a.zip", vec![1, 2, 3]);
    assert!(matches!(
        uploader.upload(&images, &zip, &[]).await,
        Err(UploadError::Config { .. })
    ));
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    // A failure comes back as it is.
    let err = Uploader::new(&store)
        .unwrap()
        .with_s3_backend(Fake::new(true) as Arc<dyn S3Backend>)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, UploadError::Failed { .. }));
}

#[tokio::test]
async fn the_output_check_accepts_a_presigned_url_but_not_the_secret() {
    let dest = presigned("https://s3.example.org", "1h");
    let store = store(&dest);
    let outcome = Uploader::new(&store)
        .unwrap()
        .with_s3_backend(Fake::new(false) as Arc<dyn S3Backend>)
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert!(outcome.url.as_str().contains(KEY_ID));
    // A file name that holds the secret ends in the link: refused (code 11).
    let leaky = UploadInput::from_bytes(&format!("{SECRET}.png"), vec![1, 2, 3]);
    let err = Uploader::new(&store)
        .unwrap()
        .with_s3_backend(Fake::new(false) as Arc<dyn S3Backend>)
        .upload(&dest, &leaky, &[])
        .await
        .unwrap_err();
    assert!(is_security(&err, "secret_in_output"), "{err:?}");
}

/// `AWS_*`, a profile, the metadata endpoint and the proxy variables are not read: the parent starts
/// this test again in a child whose environment holds them (`set_var` is `unsafe`, which this crate
/// does not allow).
#[test]
fn aws_environment_and_proxy_are_ignored() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "s3_tests::aws_environment_child", "--nocapture"])
        .env("KROPA_S3_CHILD", "1")
        .env("AWS_ACCESS_KEY_ID", "AWSENVKEYIDSHOULDNOTBEUSED")
        .env("AWS_SECRET_ACCESS_KEY", "aws-env-secret-should-not-be-used")
        .env("AWS_SESSION_TOKEN", "aws-env-session-should-not-be-used")
        .env("AWS_REGION", "ap-south-1")
        .env("AWS_DEFAULT_REGION", "ap-south-1")
        .env("AWS_ENDPOINT_URL", "https://aws-env.invalid")
        .env("AWS_ENDPOINT_URL_S3", "https://aws-env.invalid")
        .env("AWS_PROFILE", "should-not-exist")
        .env("AWS_EC2_METADATA_DISABLED", "false")
        .env(
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
            "http://127.0.0.1:9/creds",
        )
        .env("AWS_WEB_IDENTITY_TOKEN_FILE", "/nonexistent")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("https_proxy", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9");
    let output = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("aws_environment_child"),
        "the child did not run: {stdout}"
    );
}

#[tokio::test]
async fn aws_environment_child() {
    if std::env::var_os("KROPA_S3_CHILD").is_none() {
        return;
    }
    let server = Front::start().await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"abc\""))
        .expect(1)
        .mount(&server)
        .await;
    let dest = destination(&server.uri(), |t| t);
    let store = store(&dest);
    server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .expect("the proxy of the environment is not used");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let auth = requests[0]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        auth.starts_with(&format!("AWS4-HMAC-SHA256 Credential={KEY_ID}/"))
            && auth.contains("/garage/s3/"),
        "{auth}"
    );
    assert!(!requests[0].headers.contains_key("x-amz-security-token"));
}
