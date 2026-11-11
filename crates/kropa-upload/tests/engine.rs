// SPDX-License-Identifier: GPL-3.0-or-later

//! The upload engine against `wiremock` servers on the loopback. Nothing here leaves the machine.
//!
//! Servers are plain `http://127.0.0.1`: the `test-loopback` feature (enabled for this test build
//! only, see `Cargo.toml`) lets a secret travel there. A release build refuses (`https_required`),
//! which `https_is_required_for_a_secret_off_the_loopback` and the unit tests of the policy cover.

use kropa_upload::destination::{Destination, DestinationFile};
use kropa_upload::engine::{LOOPBACK_HTTP_WITH_SECRET, UploadInput, Uploader};
use kropa_upload::error::{Retry, UploadError};
use kropa_upload::masked::SecretSet;
use kropa_upload::origin::canonical_origin;
use kropa_upload::secret::{MemorySecretSource, Placement};
use wiremock::matchers::{any, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = concat!("tok-", "Zq7kLm9", "Xt2vRw-abcdef");

fn input() -> UploadInput {
    UploadInput::from_bytes("shot.png", b"\x89PNG fake image".to_vec())
}

/// An http destination parsed from `toml`, `BASE` replaced by the server address.
fn http_destination(server: &MockServer, toml: &str) -> Destination {
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

fn store_for(server: &MockServer, profile: &str, placements: Vec<Placement>) -> MemorySecretSource {
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

// ---------------------------------------------------------------------------------------------
// Zipline
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn zipline_upload_end_to_end() {
    let server = MockServer::start().await;
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
    let uploader = Uploader::new(&store).unwrap();
    let outcome = uploader.upload(&dest, &input(), &[]).await.unwrap();
    assert_eq!(outcome.url.as_str(), public);
    assert_eq!(outcome.status, 200);
    assert!(outcome.deletion_url.is_none());
    assert!(outcome.warnings.is_empty());
}

#[tokio::test]
async fn zipline_error_message_comes_from_the_error_field() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": "invalid token", "code": 401})),
        )
        .mount(&server)
        .await;
    let store = store_for(&server, "zipline", vec![Placement::header("Authorization")]);
    let dest = Destination::zipline("zipline", &server.uri());
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(401));
    assert_eq!(err.server_message().unwrap().as_str(), "invalid token");
    assert_eq!(err.retry(), Retry::Final);
    assert_eq!(err.reason(), "http_status");
    assert_eq!(err.exit_code().code(), 9);
}

// ---------------------------------------------------------------------------------------------
// Authorization before any connection
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_secret_for_another_origin_is_refused_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&server).await;
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
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "origin_not_allowed");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_secret_moved_to_another_place_is_refused_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&server).await;
    // The entry only allows `form:title`; the profile puts it in `Authorization`.
    let store = store_for(&server, "maison", vec![Placement::Form("title".into())]);
    let dest = http_destination(&server, HTTP_TOML);
    let err = Uploader::new(&store)
        .unwrap()
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
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "placement_not_allowed");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_changed_body_template_changes_the_placement() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&server).await;
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
    Uploader::new(&store).unwrap().upload(&dest, &input(), &[]).await.unwrap();
    // Edited template: the authorization does not follow.
    let edited = http_destination(&server, &toml.replace(r#"{"key": "#, r#"{"other": "#));
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&edited, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "placement_not_allowed");
}

#[tokio::test]
async fn a_missing_secret_is_code_7_and_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&server).await;
    let store = MemorySecretSource::new();
    let dest = http_destination(&server, HTTP_TOML);
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert!(matches!(err, UploadError::SecretMissing { .. }));
    assert_eq!(err.exit_code().code(), 7);
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn https_is_required_for_a_secret_off_the_loopback() {
    assert!(LOOPBACK_HTTP_WITH_SECRET, "these tests run with the test-loopback feature");
    let store = MemorySecretSource::new();
    store
        .insert(
            "zipline",
            "token",
            TOKEN,
            &["http://z.example.org:80"],
            vec![Placement::header("Authorization")],
        )
        .unwrap();
    let dest = Destination::zipline("zipline", "http://z.example.org");
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "https_required");
    assert_eq!(err.exit_code().code(), 11);
}

#[tokio::test]
async fn http_without_a_secret_is_allowed() {
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
    let store = MemorySecretSource::new();
    let dest = http_destination(&server, toml);
    let outcome = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "http://files.example.org/a.png");
}

// ---------------------------------------------------------------------------------------------
// Redirections
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_redirect_to_another_origin_is_refused_and_never_followed() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&second).await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(302).insert_header("location", format!("{}/steal", second.uri()).as_str()))
        .mount(&first)
        .await;
    let store = store_for(&first, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&first, HTTP_TOML);
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "cross_origin_redirect");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&second).await, 0);
    assert_eq!(requests(&first).await, 1);
}

#[tokio::test]
async fn a_redirect_to_another_port_is_another_origin() {
    // Same host, other port: not the same origin (a squatted loopback port).
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(307).insert_header("location", format!("{}/x", second.uri()).as_str()))
        .mount(&first)
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
    let dest = http_destination(&first, toml);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "cross_origin_redirect");
    assert_eq!(requests(&second).await, 0);
}

#[tokio::test]
async fn same_origin_redirects_are_followed_with_the_method_rules() {
    let server = MockServer::start().await;
    // 307 keeps POST and the body; 302 turns into a GET.
    Mock::given(method("POST"))
        .and(path("/up"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/up2"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/up2"))
        .and(|req: &Request| String::from_utf8_lossy(&req.body).contains("fake image"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/done"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/done"))
        .respond_with(ok_json("http://files.example.org/final.png"))
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
    let dest = http_destination(&server, toml);
    let outcome = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "http://files.example.org/final.png");
}

#[tokio::test]
async fn redirect_loops_stop_after_five_hops() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/again"))
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
    let dest = http_destination(&server, toml);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "bad_response");
    assert_eq!(requests(&server).await, 6);
}

#[tokio::test]
async fn redirects_none_leaves_the_answer_as_it_is() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "open"
type = "http"
[destination.request]
url = "BASE/up"
redirects = "none"
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = http_destination(&server, toml);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(302));
    assert_eq!(requests(&server).await, 1);
}

// ---------------------------------------------------------------------------------------------
// Response limits and errors
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_response_over_one_mebibyte_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; 2 << 20]))
        .mount(&server)
        .await;
    let dest = http_destination(&server, &HTTP_TOML.replace("Bearer {secret:token}", "none"));
    let dest = Destination {
        secrets: Default::default(),
        ..dest
    };
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "response_too_large");
    assert_eq!(err.exit_code().code(), 9);
}

#[tokio::test]
async fn a_response_just_under_the_limit_is_read() {
    let server = MockServer::start().await;
    let mut body = br#"{"data":{"url":"http://f.example.org/a.png"},"pad":""#.to_vec();
    body.extend(std::iter::repeat_n(b'a', (1 << 20) - body.len() - 2));
    body.extend_from_slice(b"\"}");
    assert_eq!(body.len(), 1 << 20);
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
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
    let dest = http_destination(&server, toml);
    let outcome = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "http://f.example.org/a.png");
}

#[tokio::test]
async fn client_and_server_errors_are_code_9_with_a_retry_class() {
    let server = MockServer::start().await;
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
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({
                "error": { "message": format!("failed with {status}") }
            })))
            .mount(&server)
            .await;
        let err = Uploader::new(&store)
            .unwrap()
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
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {}})))
        .mount(&server)
        .await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    let err = Uploader::new(&store)
        .unwrap()
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
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "bad_response");
}

#[tokio::test]
async fn hostile_urls_are_refused() {
    let server = MockServer::start().await;
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
        Mock::given(any()).respond_with(ok_json(url)).mount(&server).await;
        let err = Uploader::new(&store)
            .unwrap()
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err();
        assert_eq!(err.reason(), "url_invalid", "{url:?}");
    }
}

#[tokio::test]
async fn the_response_url_has_no_query_fragment_or_userinfo() {
    let server = MockServer::start().await;
    Mock::given(path("/up"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/u/final.png?token=SERVERSIDE#frag"))
        .mount(&server)
        .await;
    Mock::given(path("/u/final.png"))
        .and(query_param("token", "SERVERSIDE"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "redirecting"
type = "http"
[destination.request]
url = "BASE/up"
[destination.response]
url = "{responseurl}"
"#;
    let dest = http_destination(&server, toml);
    let outcome = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), format!("{}/u/final.png", server.uri()));
}

#[tokio::test]
async fn regex_templates_read_the_body() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_string("saved as https://f.example.org/i/42.png ok"))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "text"
type = "http"
[destination.request]
url = "BASE/up"
[destination.response]
url = "{regex:(https://\\S+)}"
"#;
    let dest = http_destination(&server, toml);
    let outcome = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), "https://f.example.org/i/42.png");
}

// ---------------------------------------------------------------------------------------------
// The secret in the answer
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_secret_echoed_in_the_link_is_refused_in_every_form() {
    use base64::Engine as _;
    let server = MockServer::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let toml = HTTP_TOML.replace("{json:$.data.url}", "https://files.example.org/{json:$.echo}");
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"echo": form})))
            .mount(&server)
            .await;
        let err = Uploader::new(&store)
            .unwrap()
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
    let server = MockServer::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, HTTP_TOML);
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "url": "https://files.example.org/a.png", "del": TOKEN }
        })))
        .mount(&server)
        .await;
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "secret_in_output");
}

#[tokio::test]
async fn a_secret_in_the_query_never_reaches_the_link_through_the_response_url() {
    let server = MockServer::start().await;
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
    let outcome = Uploader::new(&store)
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap();
    assert_eq!(outcome.url.as_str(), format!("{}/up", server.uri()));
}

#[tokio::test]
async fn a_secret_echoed_in_an_error_is_masked_not_refused() {
    use base64::Engine as _;
    let server = MockServer::start().await;
    let store = store_for(&server, "maison", vec![Placement::header("Authorization")]);
    let dest = http_destination(&server, &HTTP_TOML.replace("error        = \"{json:$.error.message}\"", ""));
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
        let err = Uploader::new(&store)
            .unwrap()
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
    let first = MockServer::start().await;
    let second = MockServer::start().await;
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
    Mock::given(any()).respond_with(ok_json("http://f.example.org/a.png")).mount(&first).await;
    Uploader::new(&store)
        .unwrap()
        .upload(&http_destination(&first, &HTTP_TOML.replace("maison", "a")), &input(), &[])
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
    let err = Uploader::new(&store)
        .unwrap()
        .upload(&http_destination(&second, open), &input(), &[])
        .await
        .unwrap_err();
    assert!(err.server_message().unwrap().was_masked());
    assert!(!err.server_message().unwrap().as_str().contains("Qw8rTy"));
}

// ---------------------------------------------------------------------------------------------
// Variables, limits, network
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn variables_fill_fields_but_never_credentials() {
    let server = MockServer::start().await;
    Mock::given(path("/up"))
        .and(|req: &Request| String::from_utf8_lossy(&req.body).contains("my album"))
        .respond_with(ok_json("http://f.example.org/a.png"))
        .expect(1)
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "open"
type = "http"
[destination.request]
url = "BASE/up"
fields = { album = "{var:album}" }
[destination.vars.album]
choices = ["my album", "other"]
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = http_destination(&server, toml);
    let store = MemorySecretSource::new();
    let uploader = Uploader::new(&store).unwrap();
    let given = [("album".to_owned(), "my album".to_owned())];
    uploader.upload(&dest, &input(), &given).await.unwrap();
    // Missing: usage error. Outside the choices: usage error.
    let err = uploader.upload(&dest, &input(), &[]).await.unwrap_err();
    assert_eq!(err.reason(), "missing_variable");
    assert_eq!(err.exit_code().code(), 2);
    let wrong = [("album".to_owned(), "nope".to_owned())];
    assert_eq!(
        uploader.upload(&dest, &input(), &wrong).await.unwrap_err().reason(),
        "invalid_variable"
    );

    // A variable that feeds a field named like a credential: code 11.
    let sensitive = toml.replace("album = \"{var:album}\"", "api_key = \"{var:album}\"");
    let dest = http_destination(&server, &sensitive);
    let err = uploader.upload(&dest, &input(), &given).await.unwrap_err();
    assert_eq!(err.reason(), "secret_in_argument");
    assert_eq!(err.exit_code().code(), 11);
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn a_file_over_max_size_is_refused_locally() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ok_json("http://x/y")).mount(&server).await;
    let toml = r#"
[[destination]]
id = "small"
type = "http"
[destination.request]
url = "BASE/up"
max_size = "10B"
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = http_destination(&server, toml);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "file_too_large");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_slow_server_times_out() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ok_json("http://x/y").set_delay(std::time::Duration::from_secs(5)))
        .mount(&server)
        .await;
    let toml = r#"
[[destination]]
id = "slow"
type = "http"
[destination.request]
url = "BASE/up"
timeout = "300ms"
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = http_destination(&server, toml);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "timeout");
    assert_eq!(err.exit_code().code(), 8);
    assert_eq!(err.retry(), Retry::Transient);
}

#[tokio::test]
async fn a_closed_port_is_a_network_error_without_the_address() {
    // Bind a port, then close it: the connection is refused.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let toml = format!(
        r#"
[[destination]]
id = "down"
type = "http"
[destination.request]
url = "http://127.0.0.1:{port}/up"
query = {{ x = "1" }}
[destination.response]
url = "{{json:$.data.url}}"
"#
    );
    let dest = DestinationFile::parse(&toml, "t.toml").unwrap().destination.remove(0);
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &input(), &[])
        .await
        .unwrap_err();
    assert_eq!(err.exit_code().code(), 8);
    assert_eq!(err.reason(), "connect");
    assert!(!err.to_string().contains("127.0.0.1"), "{err}");
}

#[tokio::test]
async fn a_kind_the_destination_does_not_accept_is_refused() {
    let server = MockServer::start().await;
    let toml = r#"
[[destination]]
id = "images"
type = "http"
kinds = ["image"]
[destination.request]
url = "BASE/up"
[destination.response]
url = "{json:$.data.url}"
"#;
    let dest = http_destination(&server, toml);
    let doc = UploadInput::from_bytes("notes.txt", b"hello".to_vec());
    let err = Uploader::new(&MemorySecretSource::new())
        .unwrap()
        .upload(&dest, &doc, &[])
        .await
        .unwrap_err();
    assert_eq!(err.exit_code().code(), 6);
    assert_eq!(requests(&server).await, 0);
}

#[test]
fn the_unavailable_source_says_lot_1_8() {
    use kropa_upload::secret::UnavailableSecretSource;
    let dest = Destination::zipline("zipline", "https://zipline.example.org");
    let err = kropa_upload::http::block_on(async {
        Uploader::new(&UnavailableSecretSource)
            .unwrap()
            .upload(&dest, &input(), &[])
            .await
            .unwrap_err()
    });
    assert!(matches!(err, UploadError::SecretStoreNotDelivered));
    assert_eq!(err.exit_code().code(), 5);
}

#[test]
fn secret_set_is_empty_by_default() {
    assert!(SecretSet::new().is_empty());
}
