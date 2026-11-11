// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 1.9, end of task: a complete upload over a real HTTPS listener (`testing::Front`, `rcgen`),
//! with a known secret, at the most talkative level of the journal. Everything the engine lets
//! out is collected (the journal, the outcome, its `Debug`, the links in the three formats, the
//! errors) and the secret must appear in none of it, in any form: raw, base64 (both alphabets,
//! three alignments), percent-encoded, hex. The server echoes the secret in its answer, in headers
//! and in the body, to make that hard.
//!
//! The command line (stdout, stderr, JSON) is covered by `kropa-cli/tests/upload_cli.rs`, which
//! cannot reach a TLS server: the CA of the tests exists in this crate only (decision U3).

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use wiremock::matchers::any;
use wiremock::{Mock, ResponseTemplate};

use crate::destination::{Destination, DestinationFile};
use crate::engine::{UploadInput, UploadOutcome};
use crate::error::UploadError;
use crate::links::{LinkFormat, format_link};
use crate::masked::SecretSet;
use crate::origin::canonical_origin;
use crate::secret::{MemorySecretSource, Placement, SecretEntry, SecretValue};
use crate::testing::Front;

/// Built at run time: nothing here looks like a token in the source.
fn token() -> String {
    ["jrnl-", "Zq7kLm9", "Xt2+vRw", "/abcdef-QW"].concat()
}

/// Every form of `secret` that must not come out. For base64, the part of the encoding that does
/// not depend on what surrounds the secret, at each of the three alignments.
fn forms(secret: &str) -> Vec<String> {
    let mut forms = vec![secret.to_owned()];
    for lower in [false, true] {
        let mut pct = String::new();
        let mut hex = String::new();
        for b in secret.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                pct.push(char::from(b));
            } else if lower {
                pct.push_str(&format!("%{b:02x}"));
            } else {
                pct.push_str(&format!("%{b:02X}"));
            }
            if lower {
                hex.push_str(&format!("{b:02x}"));
            } else {
                hex.push_str(&format!("{b:02X}"));
            }
        }
        forms.push(pct);
        forms.push(hex);
    }
    for pad in 0..3 {
        let mut bytes = vec![b'#'; pad];
        bytes.extend_from_slice(secret.as_bytes());
        for encoded in [
            STANDARD_NO_PAD.encode(&bytes),
            URL_SAFE_NO_PAD.encode(&bytes),
        ] {
            // Drop the edges, which mix the secret with its neighbours.
            forms.push(encoded[4..encoded.len() - 4].to_owned());
        }
    }
    forms
}

/// The forms of the secret that `haystack` holds.
fn leaks(haystack: &str, secret: &str) -> Vec<String> {
    forms(secret)
        .into_iter()
        .filter(|f| haystack.contains(f.as_str()))
        .collect()
}

#[test]
fn the_check_itself_sees_every_form() {
    let secret = token();
    let all = forms(&secret);
    assert!(all.len() >= 10, "{all:?}");
    for form in &all {
        assert!(!leaks(&format!("a {form} b"), &secret).is_empty(), "{form}");
    }
    assert!(leaks("nothing here", &secret).is_empty());
}

const TOML: &str = r#"
[[destination]]
id = "journal"
type = "http"

[destination.request]
url = "BASE/up"
query = { api_key = "{secret:token}", ref = "{secret:token}", title = "{filename}" }
headers = { Authorization = "Bearer {secret:token}", Cookie = "sid={secret:token}" }
fields = { title = "{filename}" }

[destination.secrets]
token = {}

[destination.response]
url = "{json:$.data.url}"
deletion_url = "BASE/del/{json:$.data.del}"
error = "{json:$.error.message}"
"#;

fn setup(server: &Front) -> (Destination, MemorySecretSource) {
    let text = TOML.replace("BASE", &server.uri());
    let mut file = DestinationFile::parse(&text, "journal.toml").expect("destination");
    let store = MemorySecretSource::new();
    store
        .insert(
            "journal",
            "token",
            &token(),
            &[canonical_origin(&server.uri()).unwrap().as_str()],
            vec![
                Placement::header("Authorization"),
                Placement::header("Cookie"),
                Placement::Query("api_key".into()),
                // Not named like a secret: only the detector can hide this one in the journal.
                Placement::Query("ref".into()),
            ],
        )
        .unwrap();
    (file.destination.remove(0), store)
}

/// What the server puts back: the secret in headers, in the answer and in a nested field.
fn echoing(status: u16, body: serde_json::Value) -> ResponseTemplate {
    let secret = token();
    ResponseTemplate::new(status)
        .insert_header("x-echo", secret.as_str())
        .insert_header("set-cookie", format!("sid={secret}; HttpOnly").as_str())
        .insert_header(
            "www-authenticate",
            format!("Bearer realm=\"{secret}\"").as_str(),
        )
        .set_body_json(body)
}

/// Everything that can leave the engine after a success, as text.
fn everything_of(outcome: &UploadOutcome, secrets: &SecretSet, name: &str) -> String {
    let mut all = format!("{outcome:?}\n");
    for format in [LinkFormat::Url, LinkFormat::Markdown, LinkFormat::Html] {
        let link = format_link(format, &outcome.url, name, secrets).expect("link");
        all.push_str(&format!("{link}\n{link:?}\n"));
    }
    for h in &outcome.headers {
        all.push_str(&format!("{}: {}\n", h.name, h.value));
    }
    all
}

#[tokio::test]
async fn no_secret_in_the_journal_the_outcome_or_the_links_even_when_the_server_echoes_it() {
    let secret = token();
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    let public = format!("{}/u/abc123.png", server.uri());
    Mock::given(any())
        .respond_with(echoing(
            200,
            serde_json::json!({
                "data": { "url": public, "del": "d-1" },
                "debug": { "authorization": format!("Bearer {secret}"), "raw": secret },
            }),
        ))
        .mount(&server)
        .await;
    let uploader = server.uploader(&store).with_debug_log(2);
    let outcome = uploader
        .upload(
            &dest,
            &UploadInput::from_bytes("shot.png", b"\x89PNG img".to_vec()),
            &[],
        )
        .await
        .expect("upload");
    let journal = uploader.take_log();
    let text: String = journal.iter().map(|(_, l)| format!("{l}\n")).collect();

    // The journal says something: the proof is not an empty one.
    for expected in [
        "request: POST https://127.0.0.1:",
        "api_key=***",
        "ref=***",
        "authorization: ***",
        "cookie: ***",
        "response: HTTP 200",
        "set-cookie: ***",
        "x-echo: ***",
        "www-authenticate: ***",
        "upload done: HTTP 200",
    ] {
        assert!(text.contains(expected), "{expected:?} missing in:\n{text}");
    }
    // Neither the content of a request nor the body of a success.
    assert!(!text.contains("PNG img"), "{text}");
    assert!(!text.contains("d-1"), "{text}");

    let secrets = SecretSet::loaded();
    let all = format!("{text}{}", everything_of(&outcome, &secrets, "shot.png"));
    assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
}

#[tokio::test]
async fn no_secret_in_the_journal_or_the_error_of_a_failure_that_echoes_it() {
    let secret = token();
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    let standard = base64::engine::general_purpose::STANDARD;
    let bodies = [
        serde_json::json!({"error": {"message": format!("bad credentials: {secret}")}}),
        serde_json::json!({"error": {"message": format!("Authorization: Bearer {secret} rejected")}}),
        serde_json::json!({"hint": standard.encode(format!("user:{secret}"))}),
        serde_json::json!({"hint": secret.replace('/', "%2F").replace('+', "%2B")}),
        serde_json::json!({"hint": secret.replace('/', "\\/")}),
    ];
    for body in bodies {
        server.reset().await;
        Mock::given(any())
            .respond_with(echoing(500, body))
            .mount(&server)
            .await;
        let uploader = server.uploader(&store).with_debug_log(2);
        let err: UploadError = uploader
            .upload(
                &dest,
                &UploadInput::from_bytes("shot.png", b"\x89PNG".to_vec()),
                &[],
            )
            .await
            .unwrap_err();
        let text: String = uploader
            .take_log()
            .iter()
            .map(|(_, l)| format!("{l}\n"))
            .collect();
        assert!(text.contains("response body (error):"), "{text}");
        assert!(text.contains("upload failed (http_status)"), "{text}");
        let all = format!("{text}{err}\n{err:?}\n");
        assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
    }
}

#[tokio::test]
async fn a_secret_that_comes_back_in_the_link_is_withheld_and_never_logged() {
    let secret = token();
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    Mock::given(any())
        .respond_with(echoing(
            200,
            serde_json::json!({"data": {"url": format!("https://f.example.org/{}", secret.replace('/', "%2F").replace('+', "%2B")), "del": "d"}}),
        ))
        .mount(&server)
        .await;
    let uploader = server.uploader(&store).with_debug_log(2);
    let err = uploader
        .upload(
            &dest,
            &UploadInput::from_bytes("a.png", b"\x89PNG".to_vec()),
            &[],
        )
        .await
        .unwrap_err();
    assert!(err.is_secret_in_output());
    let text: String = uploader
        .take_log()
        .iter()
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    let all = format!("{text}{err}\n{err:?}");
    assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
}

#[test]
fn nothing_of_the_store_shows_in_a_debug() {
    let secret = token();
    let value = SecretValue::new(&secret).expect("long enough");
    let entry = SecretEntry {
        value,
        origins: vec!["https://a.example.org".to_owned()],
        placements: vec![Placement::header("Authorization")],
    };
    let text = format!(
        "{entry:?} {:?} {:?}",
        entry.value,
        SecretSet::from_values(&[&secret])
    );
    assert_eq!(leaks(&text, &secret), Vec::<String>::new(), "{text}");
}
