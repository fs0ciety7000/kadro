// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 1.9, end of task: a complete upload over a real HTTPS listener (`testing::Front`, `rcgen`),
//! with a known secret, at the most talkative level of the journal. Everything the engine lets
//! out is collected (the journal, the outcome, its `Debug`, the links in the three formats, the
//! errors) and the secret must appear in none of it, in any form: raw, base64 (both alphabets,
//! three alignments), percent-encoded, hex. The server echoes the secret in its answer, in headers
//! and in the body, to make that hard.
//!
//! The oracle looks for every window of 8 characters of the secret in each of its forms (raw,
//! percent, hex, JSON, HTML, base64), and is itself tested against real partial leaks.
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
use crate::masked::{MaskedText, SecretSet};
use crate::origin::canonical_origin;
use crate::secret::{MemorySecretSource, Placement, SecretEntry, SecretValue};
use crate::testing::Front;

/// Built at run time: nothing here looks like a token in the source.
fn token() -> String {
    ["jrnl-", "Zq7kLm9", "Xt2+vRw", "/abcdef-QW"].concat()
}

/// Windows of this many characters of a form of the secret are what the oracle looks for: a leak
/// of a part of the secret is a leak (N1), whatever the form it was written in.
const WINDOW: usize = 8;

/// Every form of `secret` that must not come out, whole: raw, percent-encoded (all of it, or only
/// some of the special characters), hex, JSON escapes (`\/`, `\uXXXX`), HTML entities, and for
/// base64 the part of the encoding that does not depend on what surrounds the secret, at each of
/// the three alignments.
fn forms(secret: &str) -> Vec<String> {
    let mut forms = vec![secret.to_owned()];
    let mut specials: Vec<u8> = secret
        .bytes()
        .filter(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'-' | b'.' | b'_' | b'~'))
        .collect();
    specials.sort_unstable();
    specials.dedup();
    for lower in [false, true] {
        // Percent: every subset of the special characters (the whole encoding is the full one).
        for mask in 1u32..(1 << specials.len()) {
            let chosen = |b: u8| {
                specials
                    .iter()
                    .position(|s| *s == b)
                    .is_some_and(|i| mask & (1 << i) != 0)
            };
            let mut out = String::new();
            for b in secret.bytes() {
                if chosen(b) {
                    out.push_str(&if lower {
                        format!("%{b:02x}")
                    } else {
                        format!("%{b:02X}")
                    });
                } else {
                    out.push(char::from(b));
                }
            }
            forms.push(out);
        }
        let hex: String = secret
            .bytes()
            .map(|b| {
                if lower {
                    format!("{b:02x}")
                } else {
                    format!("{b:02X}")
                }
            })
            .collect();
        forms.push(hex);
        // JSON: `\/` for the slash, `\uXXXX` for the specials (both cases of the digits).
        let mut json = String::new();
        let mut unicode = String::new();
        let mut html_dec = String::new();
        let mut html_hex = String::new();
        for c in secret.chars() {
            if c == '/' {
                json.push_str("\\/");
            } else if specials.contains(&(c as u8)) {
                json.push_str(&if lower {
                    format!("\\u{:04x}", c as u32)
                } else {
                    format!("\\u{:04X}", c as u32)
                });
            } else {
                json.push(c);
            }
            if specials.contains(&(c as u8)) {
                unicode.push_str(&if lower {
                    format!("\\u{:04x}", c as u32)
                } else {
                    format!("\\u{:04X}", c as u32)
                });
                html_dec.push_str(&format!("&#{};", c as u32));
                html_hex.push_str(&if lower {
                    format!("&#x{:x};", c as u32)
                } else {
                    format!("&#x{:X};", c as u32)
                });
            } else {
                unicode.push(c);
                html_dec.push(c);
                html_hex.push(c);
            }
        }
        forms.extend([json, unicode, html_dec, html_hex]);
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

/// Every window of [`WINDOW`] characters of every form of the secret.
fn fragments(secret: &str) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for form in forms(secret) {
        let chars: Vec<char> = form.chars().collect();
        if chars.len() < WINDOW {
            all.push(form);
            continue;
        }
        for w in chars.windows(WINDOW) {
            all.push(w.iter().collect());
        }
    }
    all.sort_unstable();
    all.dedup();
    all
}

/// The fragments of the secret that `haystack` holds.
fn leaks(haystack: &str, secret: &str) -> Vec<String> {
    fragments(secret)
        .into_iter()
        .filter(|f| haystack.contains(f.as_str()))
        .collect()
}

/// The oracle is not an empty one (I2 of the review of 1.9): it sees a part of the secret in each
/// form, which the whole-form search of 1.9 did not, and it sees the leak that a wrong order of
/// operations in the real code would make (cut first, mask after).
#[test]
fn the_fragment_oracle_catches_a_partial_leak() {
    let secret = token();
    // Parts of the secret, in several forms. None holds the whole of it.
    let json_partial = secret.replace('/', "\\/").replace('+', "\\u002b");
    let pct_partial = secret.replace('+', "%2B");
    let b64 = STANDARD_NO_PAD.encode(secret.as_bytes());
    let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
    let parts = [
        ("raw", secret[2..14].to_owned()),
        ("raw tail", secret[secret.len() - 10..].to_owned()),
        ("json", json_partial[6..24].to_owned()),
        ("percent", pct_partial[3..20].to_owned()),
        ("base64", b64[8..24].to_owned()),
        ("hex", hex[4..24].to_owned()),
    ];
    for (what, part) in &parts {
        let hay = format!("error: {part} (end)");
        assert!(!leaks(&hay, &secret).is_empty(), "{what}: {part}");
        // The search of 1.9 (whole forms only) saw none of them.
        assert!(
            !forms(&secret).iter().any(|f| hay.contains(f.as_str())),
            "{what}: the whole-form search would have seen it"
        );
    }
    assert!(leaks("nothing here, a plain text", &secret).is_empty());
    for form in forms(&secret) {
        assert!(!leaks(&format!("a {form} b"), &secret).is_empty(), "{form}");
    }

    // A real leak of the kind the code could have: the body is cut, then masked. The secret that
    // straddles the cut is no longer whole, so nothing is masked and 15 characters of it remain.
    let set = SecretSet::from_values(&[secret.as_str()]);
    let body = format!("{}{secret}{}", "x".repeat(185), "y".repeat(100));
    let cut_first: String = body.chars().take(200).collect();
    let wrong = MaskedText::from_response(cut_first.as_bytes(), &set, 1000);
    assert!(
        !leaks(wrong.as_str(), &secret).is_empty(),
        "the oracle must see a secret cut before it is masked"
    );
    // What the code does: mask the whole body, then cut.
    let right = MaskedText::from_response(body.as_bytes(), &set, 200);
    assert_eq!(leaks(right.as_str(), &secret), Vec::<String>::new());
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
        "www-authenticate: ***",
        "other header(s): ***",
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

fn journal_text(uploader: &crate::engine::Uploader<'_>) -> String {
    uploader
        .take_log()
        .iter()
        .map(|(_, l)| format!("{l}\n"))
        .collect()
}

/// The secret comes back in headers that are on the white list: the value of `location` and of
/// `content-type`. Neither the journal nor the headers kept in the outcome may carry any part of it.
#[tokio::test]
async fn no_secret_in_a_header_of_the_white_list() {
    let secret = token();
    let pct = secret.replace('+', "%2B").replace('/', "%2F");
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    let public = format!("{}/u/abc123.png", server.uri());
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": {"url": public, "del": "d-1"}}))
                .insert_header(
                    "location",
                    format!("{}/next?ref={pct}#{secret}", server.uri()).as_str(),
                )
                .insert_header(
                    "content-type",
                    format!("application/json; x={secret}").as_str(),
                ),
        )
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
    let text = journal_text(&uploader);
    assert!(text.contains("location: https://127.0.0.1:"), "{text}");
    assert!(text.contains("content-type: "), "{text}");
    assert!(
        outcome.headers.iter().any(|h| h.name == "location"),
        "{outcome:?}"
    );
    let all = format!(
        "{text}{}",
        everything_of(&outcome, &SecretSet::loaded(), "shot.png")
    );
    assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
}

/// The `BODY_LIMIT` of the journal (200) and the `MESSAGE_LIMIT` of the error (500) are met by a
/// body of 1 000 characters, with the secret astride each of the cuts.
#[tokio::test]
async fn a_secret_astride_the_cut_of_a_long_error_body_leaves_no_fragment() {
    let secret = token();
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    for offset in [0usize, 100, 172, 185, 195, 199, 200, 471, 485, 499] {
        let message = format!(
            "{}{secret}{}",
            "x ".repeat(offset / 2) + &"x".repeat(offset % 2),
            " tail".repeat(100)
        );
        server.reset().await;
        Mock::given(any())
            .respond_with(echoing(
                500,
                serde_json::json!({"error": {"message": message}}),
            ))
            .mount(&server)
            .await;
        let uploader = server.uploader(&store).with_debug_log(2);
        let err = uploader
            .upload(
                &dest,
                &UploadInput::from_bytes("shot.png", b"\x89PNG".to_vec()),
                &[],
            )
            .await
            .unwrap_err();
        let text = journal_text(&uploader);
        let all = format!("{text}{err}\n{err:?}\n");
        assert!(all.contains('…'), "the cut was not met at {offset}: {all}");
        assert_eq!(
            leaks(&all, &secret),
            Vec::<String>::new(),
            "offset {offset}: {all}"
        );
    }
}

/// A redirection to the same origin whose address carries the secret in a parameter that is not
/// named like one: the detector alone hides it, in the journal and in the error.
#[tokio::test]
async fn no_secret_in_the_address_of_a_redirection() {
    use wiremock::matchers::path;
    let secret = token();
    let pct = secret.replace('+', "%2B").replace('/', "%2F");
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    Mock::given(path("/up"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("/moved?ref={pct}&a=b").as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(path("/moved"))
        .respond_with(echoing(
            500,
            serde_json::json!({"error": {"message": format!("see {secret}")}}),
        ))
        .mount(&server)
        .await;
    let uploader = server.uploader(&store).with_debug_log(2);
    let err = uploader
        .upload(
            &dest,
            &UploadInput::from_bytes("shot.png", b"\x89PNG".to_vec()),
            &[],
        )
        .await
        .unwrap_err();
    let text = journal_text(&uploader);
    assert!(text.contains("response: HTTP 302"), "{text}");
    assert!(text.contains("location: https://127.0.0.1:"), "{text}");
    assert!(text.contains("/moved?ref=***"), "{text}");
    let all = format!("{text}{err}\n{err:?}\n");
    assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
}

/// A client that does not trust the CA of the server: the handshake fails, after the lines of the
/// request were written. The secret is in the query (`api_key`, and `ref` which is not named like
/// one) and is in none of them, nor in the error.
#[tokio::test]
async fn no_secret_when_the_tls_handshake_fails() {
    let secret = token();
    let server = Front::start().await;
    let (dest, store) = setup(&server);
    let uploader = crate::engine::Uploader::new(&store)
        .expect("default client")
        .with_debug_log(2);
    let err = uploader
        .upload(
            &dest,
            &UploadInput::from_bytes("shot.png", b"\x89PNG".to_vec()),
            &[],
        )
        .await
        .unwrap_err();
    assert_eq!(err.reason(), "tls", "{err:?}");
    let text = journal_text(&uploader);
    for expected in [
        "request: POST https://127.0.0.1:",
        "api_key=***",
        "ref=***",
        "authorization: ***",
        "cookie: ***",
    ] {
        assert!(text.contains(expected), "{expected:?} missing in:\n{text}");
    }
    let all = format!("{text}{err}\n{err:?}\n");
    assert_eq!(leaks(&all, &secret), Vec::<String>::new(), "{all}");
}
