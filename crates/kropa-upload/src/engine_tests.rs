// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine with a secret, against `wiremock` servers behind a **real HTTPS** listener on the
//! loopback (`testing::Front`, certificates made by `rcgen`). Nothing leaves the machine.
//!
//! Decision U3: a secret never travels over `http://`, not even on the loopback, and no build of the
//! product has an exception for it. The client that trusts the test CA is compiled in test builds of
//! this crate only (`HttpClient::with_extra_root`). The cases without a secret are in
//! `tests/engine.rs`, which uses the client of the product over plain `http://`.

#![allow(clippy::default_trait_access)]

use wiremock::matchers::{any, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::destination::{Destination, DestinationFile};
use crate::engine::{UploadInput, Uploader};
use crate::error::{Retry, UploadError};
use crate::origin::canonical_origin;
use crate::secret::{MemorySecretSource, Placement};
use crate::testing::Front;

const TOKEN: &str = concat!("tok-", "Zq7kLm9", "Xt2vRw-abcdef");

fn input() -> UploadInput {
    UploadInput::from_bytes("shot.png", b"\x89PNG fake image".to_vec())
}

/// A destination parsed from `toml`, `BASE` replaced by the address of the server.
fn http_destination(server: &Front, toml: &str) -> Destination {
    let text = toml.replace("BASE", &server.uri());
    let mut file = DestinationFile::parse(&text, "test.toml").expect("valid destination");
    file.destination.remove(0)
}

const HTTP_TOML: &str = r#"
[[destination]]
id = "maison"
type = "http"

[destination.request]
url = "BASE/up"
headers = { Authorization = "Bearer {secret:token}" }
fields = { title = "{filename}" }

[destination.secrets]
token = {}

[destination.response]
url = "{json:$.data.url}"
deletion_url = "BASE/del/{json:$.data.del}"
error = "{json:$.error.message}"
"#;

fn store_for(server: &Front, profile: &str, placements: Vec<Placement>) -> MemorySecretSource {
    let store = MemorySecretSource::new();
    let origin = canonical_origin(&server.uri()).unwrap();
    store
        .insert(profile, "token", TOKEN, &[origin.as_str()], placements)
        .unwrap();
    store
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

fn ok_json(url: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "data": { "url": url, "del": "d-1" }
    }))
}

#[tokio::test]
async fn zipline_upload_end_to_end() {
    let server = Front::start().await;
    let public = format!("{}/u/abc123.png", server.uri());
    Mock::given(method("POST"))
        .and(path("/api/upload"))
        .and(header("authorization", TOKEN))
        .and(header("user-agent", format!("kropa/{}", env!("CARGO_PKG_VERSION")).as_str()))
        .and(|req: &Request| {
            let body = String::from_utf8_lossy(&req.body);
            let content_type = req
                .headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            content_type.starts_with("multipart/form-data")
                && body.contains("name=\"file\"")
                && body.contains("filename=\"shot.png\"")
                && body.contains("fake image")
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{ "id": "x", "name": "abc123.png", "type": "image/png", "url": public, "pending": false }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let store = store_for(&server, "zipline", vec![Placement::header("Authorization")]);
    let dest = Destination::zipline("zipline", &server.uri());
    let uploader = server.uploader(&store);
    let outcome = uploader.upload(&dest, &input(), &[]).await.unwrap();
    assert_eq!(outcome.url.as_str(), public);
    assert_eq!(outcome.status, 200);
    assert!(outcome.deletion_url.is_none());
    assert!(outcome.warnings.is_empty());
}

#[tokio::test]
async fn zipline_error_message_comes_from_the_error_field() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"error": "invalid token", "code": 401})),
        )
        .mount(&server)
        .await;
    let store = store_for(&server, "zipline", vec![Placement::header("Authorization")]);
    let dest = Destination::zipline("zipline", &server.uri());
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(401));
    assert_eq!(err.server_message().unwrap().as_str(), "invalid token");
    assert_eq!(err.retry(), Retry::Final);
    assert_eq!(err.reason(), "http_status");
    assert_eq!(err.exit_code().code(), 9);
}

#[tokio::test]
async fn a_secret_for_another_origin_is_refused_before_any_request() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    let store = MemorySecretSource::new();
    store
        .insert(
            "maison",
            "token",
            TOKEN,
            &["https://elsewhere.example.org:443"],
            vec![Placement::header("Authorization")],
        )
        .unwrap();
    let dest = http_destination(&server, HTTP_TOML);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "origin_not_allowed");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_secret_moved_to_another_place_is_refused_before_any_request() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    // The entry only allows `form:title`; the profile puts it in `Authorization`.
    let store = store_for(&server, "maison", vec![Placement::Form("title".into())]);
    let dest = http_destination(&server, HTTP_TOML);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "placement_not_allowed");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 0);

    // The same secret asked for through `query:key` by a profile that was edited.
    let moved = HTTP_TOML.replace(
        "headers = { Authorization = \"Bearer {secret:token}\" }",
        "query = { key = \"{secret:token}\" }",
    );
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, &moved);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "placement_not_allowed");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_changed_body_template_changes_the_placement() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "maison"
type = "http"
[destination.request]
url = "BASE/up"
body = "json"
body_template = '{"key": "{secret:token}"}'
[destination.secrets]
token = {}
[destination.response]
url = "{json:$.data.url}"
"#;
    let original = r#"{"key": "{secret:token}"}"#;
    let store = store_for(&server, "maison", vec![Placement::body(original)]);
    let dest = http_destination(&server, toml);
    // Same template: allowed.
    server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    // Edited template: the authorization does not follow.
    let edited = http_destination(&server, &toml.replace(r#"{"key": "#, r#"{"other": "#));
    let err = server
        .uploader(&store)
        .upload(&edited, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "placement_not_allowed");
}

#[tokio::test]
async fn a_missing_secret_is_code_7_and_sends_nothing() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    let store = MemorySecretSource::new();
    let dest = http_destination(&server, HTTP_TOML);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, UploadError::SecretMissing { .. }));
    assert_eq!(err.exit_code().code(), 7);
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn https_is_required_for_a_secret_even_on_the_loopback() {
    // Decision U3: no exception. A plain `http://127.0.0.1` server and a secret: refused before the
    // secret is read and before any connection, with the real client of the product.
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    let store = MemorySecretSource::new();
    store
        .insert(
            "zipline",
            "token",
            TOKEN,
            &[canonical_origin(&server.uri()).unwrap().as_str()],
            vec![Placement::header("Authorization")],
        )
        .unwrap();
    let dest = Destination::zipline("zipline", &server.uri());
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "https_required");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 0);
    // Off the loopback, the same.
    let dest = Destination::zipline("zipline", "http://z.example.org");
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "https_required");
}

#[tokio::test]
async fn http_without_a_secret_still_works_with_the_real_client() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/up"))
        .respond_with(ok_json("http://files.example.org/a.png"))
        .expect(1)
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "open"
type = "http"
[destination.request]
url = "BASE/up"
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = DestinationFile::parse(&toml.replace("BASE", &server.uri()), "t.toml")
        .unwrap()
        .destination
        .remove(0);
    let outcome = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "http://files.example.org/a.png");
}

#[tokio::test]
async fn a_certificate_the_system_does_not_trust_is_refused_and_nothing_is_sent() {
    // The product client does not know the CA of the test server: the handshake fails before the
    // secret leaves (docs/security/secrets.md section 7, "certificat rcgen non approuve").
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "tls");
    assert_eq!(err.exit_code().code(), 8);
    assert!(!err.to_string().contains(TOKEN));
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_redirect_to_another_origin_is_refused_and_never_followed() {
    let first = Front::start().await;
    let second = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&second)
        .await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/steal", second.uri()).as_str()),
        )
        .mount(&first)
        .await;
    let store = store_for(&first, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&first, HTTP_TOML);
    let err = first
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "cross_origin_redirect");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&second).await, 0);
    assert_eq!(requests(&first).await, 1);
}

#[tokio::test]
async fn client_and_server_errors_are_code_9_with_a_retry_class() {
    let server = Front::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    for (status, retry) in [
        (400, Retry::Final),
        (403, Retry::Final),
        (404, Retry::Final),
        (408, Retry::Transient),
        (429, Retry::Transient),
        (500, Retry::Transient),
        (503, Retry::Transient),
    ] {
        server.reset().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": { "message": format!("failed with {status}") }
                })),
            )
            .mount(&server)
            .await;
        let err = server
            .uploader(&store)
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err();
        assert_eq!(err.exit_code().code(), 9, "{status}");
        assert_eq!(err.reason(), "http_status");
        assert_eq!(err.status(), Some(status));
        assert_eq!(err.retry(), retry, "{status}");
        assert_eq!(
            err.server_message().unwrap().as_str(),
            format!("failed with {status}")
        );
    }
}

#[tokio::test]
async fn an_answer_without_a_url_is_an_error() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {}})))
        .mount(&server)
        .await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "url_missing");
    assert_eq!(err.exit_code().code(), 9);
    // Not JSON at all.
    server.reset().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "bad_response");
}

#[tokio::test]
async fn hostile_urls_are_refused() {
    let server = Front::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    for url in [
        "javascript:alert(1)",
        "ftp://files.example.org/a",
        "https://user:pw@files.example.org/a",
        "https://files.example.org/a\u{202e}gnp.exe",
        "https://files.example.org/\u{200b}a",
        "https://files.example.org/a\nb",
        "not a url",
    ] {
        server.reset().await;
        Mock::given(any())
            .respond_with(ok_json(url))
            .mount(&server)
            .await;
        let err = server
            .uploader(&store)
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err();
        assert_eq!(err.reason(), "url_invalid", "{url:?}");
    }
}

#[tokio::test]
async fn a_secret_echoed_in_the_link_is_refused_in_every_form() {
    use base64::Engine as _;
    let server = Front::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let toml = HTTP_TOML.replace(
        "{json:$.data.url}",
        "https://files.example.org/{json:$.echo}",
    );
    let dest = http_destination(&server, &toml);
    let standard = base64::engine::general_purpose::STANDARD;
    let forms = [
        TOKEN.to_owned(),
        standard.encode(format!("user:{TOKEN}")),
        standard.encode(format!("Bearer {TOKEN}")),
        TOKEN.replace('-', "%2D"),
        TOKEN.replace('-', "\\u002d"),
        TOKEN.replace('-', "&#45;"),
        format!("{TOKEN}.png"),
    ];
    for form in forms {
        server.reset().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"echo": form})),
            )
            .mount(&server)
            .await;
        let err = server
            .uploader(&store)
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err();
        assert_eq!(err.reason(), "secret_in_output", "{form}");
        assert_eq!(err.exit_code().code(), 11);
        assert!(err.is_secret_in_output());
        assert!(!err.to_string().contains(TOKEN));
    }
}

#[tokio::test]
async fn a_secret_echoed_in_the_deletion_url_is_refused_too() {
    let server = Front::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "url": "https://files.example.org/a.png", "del": TOKEN }
        })))
        .mount(&server)
        .await;
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "secret_in_output");
}

#[tokio::test]
async fn a_secret_in_the_query_never_reaches_the_link_through_the_response_url() {
    let server = Front::start().await;
    let toml = r#"
[[destination]]
id = "maison"
type = "http"
[destination.request]
url = "BASE/up"
query = { key = "{secret:token}" }
[destination.secrets]
token = {}
[destination.response]
url = "{responseurl}"
"#;
    Mock::given(path("/up"))
        .and(query_param("key", TOKEN))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let store = store_for(&server, "maison", vec![Placement::Query("key".into())]);
    let dest = http_destination(&server, toml);
    let outcome = server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), format!("{}/up", server.uri()));
}

#[tokio::test]
async fn a_secret_echoed_in_an_error_is_masked_not_refused() {
    use base64::Engine as _;
    let server = Front::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(
        &server,
        &HTTP_TOML.replace("error        = \"{json:$.error.message}\"", ""),
    );
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("user:{TOKEN}"));
    for body in [
        format!("bad credentials: {TOKEN}"),
        format!("Authorization: Bearer {TOKEN} rejected"),
        format!("{{\"hint\":\"{basic}\"}}"),
        format!("%{}%", TOKEN.replace('-', "%2D")),
    ] {
        server.reset().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(400).set_body_string(body))
            .mount(&server)
            .await;
        let err = server
            .uploader(&store)
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(400));
        let message = err.server_message().unwrap();
        assert!(message.was_masked());
        assert!(message.as_str().contains("***"), "{message}");
        assert!(!message.as_str().contains("Zq7kLm9"), "{message}");
        assert!(!message.as_str().contains(&basic[8..24]), "{message}");
        assert!(!err.to_string().contains("Zq7kLm9"));
    }
}

#[tokio::test]
async fn a_secret_loaded_earlier_in_the_run_is_masked_in_a_later_error() {
    // N5: the detector knows every secret loaded since the start, not only the current request's.
    let first = Front::start().await;
    let second = Front::start().await;
    let earlier = "earlier-secret-Qw8rTy5uIo";
    let store = MemorySecretSource::new();
    store
        .insert(
            "a",
            "token",
            earlier,
            &[canonical_origin(&first.uri()).unwrap().as_str()],
            vec![Placement::header("Authorization")],
        )
        .unwrap();
    Mock::given(any())
        .respond_with(ok_json("http://f.example.org/a.png"))
        .mount(&first)
        .await;
    first
        .uploader(&store)
        .upload(
            &http_destination(&first, &HTTP_TOML.replace("maison", "a")),
            &input(),
            &[],
        )
        .await
        .unwrap();
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500).set_body_string(format!("oops {earlier}")))
        .mount(&second)
        .await;
    let open = r#"
[[destination]]
id = "open"
type = "http"
[destination.request]
url = "BASE/up"
[destination.response]
url = "{json:$.data.url}"
"#;
    let err = second
        .uploader(&store)
        .upload(&http_destination(&second, open), &input(), &[])
        .await
        .unwrap_err();
    assert!(err.server_message().unwrap().was_masked());
    assert!(!err.server_message().unwrap().as_str().contains("Qw8rTy"));
}

// ---------------------------------------------------------------------------------------------
// Review 1.7: an imported `Basic {base64:user:password}` is really rendered at send time
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_imported_basic_header_is_rendered_and_sent_over_https() {
    use base64::Engine as _;
    let server = Front::start().await;
    let password = format!("{}{}", "Pw-Zq7kLm9", "Xt2vRw-abcdef");
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("alice:{password}"))
    );
    Mock::given(path("/up"))
        .and(header("authorization", expected.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "link": "https://files.example.org/a.png"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let sxcu = serde_json::json!({
        "Version": "16.0.1",
        "Name": "Basic auth",
        "DestinationType": "ImageUploader",
        "RequestURL": format!("{}/up", server.uri()),
        "Headers": { "Authorization": format!("Basic {{base64:alice:{password}}}") },
        "Body": "MultipartFormData",
        "URL": "{json:link}"
    })
    .to_string();
    let plan =
        crate::import::import(sxcu.as_bytes(), crate::import::ImportFormat::Sxcu, None).unwrap();
    assert_eq!(plan.secrets.len(), 1);
    // What lot 1.8 will do with the plan: the value goes to the store for its origin and place.
    let store = MemorySecretSource::new();
    let entry = &plan.secrets[0];
    store
        .insert(
            &plan.destination.id,
            &entry.name,
            entry.value.as_ref().unwrap().expose(),
            &[entry.origin.as_str()],
            vec![entry.placement.clone()],
        )
        .unwrap();
    let outcome = server
        .uploader(&store)
        .upload(&plan.destination, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "https://files.example.org/a.png");
}

// ---------------------------------------------------------------------------------------------
// Review 1.7 (I2): a `--var` may not feed a place of a secret, whatever the name of the slot
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_var_may_not_feed_the_body_that_holds_a_secret() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("https://f.example.org/a.png"))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "maison"
type = "http"
[destination.request]
url = "BASE/up"
body = "json"
body_template = '{"key": "{secret:token}", "note": "{var:note}"}'
[destination.secrets]
token = {}
[destination.vars.note]
default = "none"
[destination.response]
url = "{json:$.data.url}"
"#;
    let template = r#"{"key": "{secret:token}", "note": "{var:note}"}"#;
    let store = store_for(&server, "maison", vec![Placement::body(template)]);
    let dest = http_destination(&server, toml);
    let given = [("note".to_owned(), "x".to_owned())];
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &given)
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "secret_in_argument");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 0);
    // The default of the profile (nobody typed it) is fine.
    server
        .uploader(&store)
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn a_var_may_not_feed_a_slot_with_a_neutral_name_that_holds_a_secret() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("https://f.example.org/a.png"))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "maison"
type = "http"
[destination.request]
url = "BASE/up"
headers = { X-Custom = "v1-{secret:token}-{var:suffix}" }
[destination.secrets]
token = {}
[destination.response]
url = "{json:$.data.url}"
"#;
    let store = store_for(&server, "maison", vec![Placement::header("X-Custom")]);
    let dest = http_destination(&server, toml);
    let given = [("suffix".to_owned(), "x".to_owned())];
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &given)
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "secret_in_argument");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_var_may_not_feed_a_place_the_loaded_secret_is_allowed_in() {
    let server = Front::start().await;
    Mock::given(any())
        .respond_with(ok_json("https://f.example.org/a.png"))
        .mount(&server)
        .await;
    // The secret is used in `Authorization`; the entry also allows `form:data`, a neutral name that
    // a variable could use to move text next to the secret.
    let toml = r#"
[[destination]]
id = "maison"
type = "http"
[destination.request]
url = "BASE/up"
headers = { Authorization = "Bearer {secret:token}" }
fields = { data = "{var:data}", album = "{var:album}" }
[destination.secrets]
token = {}
[destination.response]
url = "{json:$.data.url}"
"#;
    let store = store_for(
        &server,
        "maison",
        vec![
            Placement::header("Authorization"),
            Placement::Form("data".into()),
        ],
    );
    let dest = http_destination(&server, toml);
    let album = [("album".to_owned(), "a".to_owned())];
    let data = [("data".to_owned(), "d".to_owned())];
    // `album` is no place of the secret: allowed (the other variable has a default of its own).
    let with_default = toml.replace(
        "[destination.secrets]",
        "[destination.vars.data]\ndefault = \"d\"\n[destination.secrets]",
    );
    let dest_ok = http_destination(&server, &with_default);
    server
        .uploader(&store)
        .upload(&dest_ok, &input(), &album)
        .await
        .unwrap();
    // `data` is: refused, before any request of this test body.
    let before = requests(&server).await;
    let given = [album[0].clone(), data[0].clone()];
    let err = server
        .uploader(&store)
        .upload(&dest, &input(), &given)
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "secret_in_argument");
    assert_eq!(requests(&server).await, before);
}
