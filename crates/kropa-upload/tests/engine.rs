// SPDX-License-Identifier: GPL-3.0-or-later

//! The upload engine against `wiremock` servers on the loopback, **without a secret**: plain
//! `http://127.0.0.1` with the client of the product. Nothing here leaves the machine.
//!
//! The cases that carry a secret need HTTPS (decision U3, no exception) and run in
//! `src/engine_tests.rs`, against a real TLS listener whose CA only the test client trusts.

#![allow(clippy::default_trait_access, clippy::assert_is_empty)]

use kropa_upload::destination::{Destination, DestinationFile};
use kropa_upload::engine::{UploadInput, Uploader};
use kropa_upload::error::{Retry, UploadError};
use kropa_upload::masked::SecretSet;
use kropa_upload::secret::MemorySecretSource;
use wiremock::matchers::{any, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn input() -> UploadInput {
    UploadInput::from_bytes("shot.png", b"\x89PNG fake image".to_vec())
}

/// An http destination parsed from `toml`, `BASE` replaced by the server address.
fn http_destination(server: &MockServer, toml: &str) -> Destination {
    let text = toml.replace("BASE", &server.uri());
    let mut file = DestinationFile::parse(&text, "test.toml").expect("valid destination");
    file.destination.remove(0)
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

// ---------------------------------------------------------------------------------------------
// Authorization before any connection
// ---------------------------------------------------------------------------------------------

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
async fn a_redirect_to_another_port_is_another_origin() {
    // Same host, other port: not the same origin (a squatted loopback port).
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/x", second.uri()).as_str()),
        )
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
async fn the_response_url_has_no_query_fragment_or_userinfo() {
    let server = MockServer::start().await;
    Mock::given(path("/up"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/u/final.png?token=SERVERSIDE#frag"),
        )
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
    assert_eq!(
        outcome.url.as_str(),
        format!("{}/u/final.png", server.uri())
    );
}

#[tokio::test]
async fn regex_templates_read_the_body() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("saved as https://f.example.org/i/42.png ok"),
        )
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
        uploader
            .upload(&dest, &input(), &wrong)
            .await
            .unwrap_err()
            .reason(),
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
    Mock::given(any())
        .respond_with(ok_json("http://x/y"))
        .mount(&server)
        .await;
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
    let dest = DestinationFile::parse(&toml, "t.toml")
        .unwrap()
        .destination
        .remove(0);
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
