// SPDX-License-Identifier: GPL-3.0-or-later

//! `upload`, `dest list|show|test|import` and `capture --upload` end to end, in process, against
//! `wiremock` servers on the loopback and an in-memory secret store. Nothing leaves the machine.

#![allow(
    clippy::many_single_char_names,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::format_push_string
)]

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use kropa_capture::{
    BackendId, Capabilities, Capability, CaptureBackend, CaptureError, CaptureRequest,
    CapturedImage, Diagnosis, ImageFormat, Status,
};
use kropa_cli::secret_input::{InputError, InputFrom, SecretInput};
use kropa_cli::{Io, Services, run_with_services};
use kropa_core::FixedClock;
use kropa_upload::origin::canonical_origin;
use kropa_upload::secret::{
    MemorySecretSource, Placement, SecretSource, SecretStore, UnavailableSecretSource,
};
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zeroize::Zeroizing;

const TOKEN: &str = concat!("cli-", "Zq7kLm9", "Xt2vRw-abcdef");

struct Out {
    code: u8,
    out: String,
    err: String,
}

/// These tests give no secret by hand: asking for one is a bug.
struct NoInput;

impl SecretInput for NoInput {
    fn read(&self, _from: InputFrom, _prompt: &str) -> Result<Zeroizing<String>, InputError> {
        Err(InputError::Unreadable)
    }
}

struct Fake;

impl CaptureBackend for Fake {
    fn id(&self) -> BackendId {
        BackendId::PortalScreenshot
    }
    fn formats(&self) -> &'static [ImageFormat] {
        &[ImageFormat::Png]
    }
    fn diagnose(&self) -> Diagnosis {
        let cap = |feature| Capability {
            feature,
            status: Status::Available,
            reason: None,
            fallback: None,
        };
        Diagnosis {
            platform: "linux",
            backend: "portal-screenshot",
            capabilities: Capabilities {
                screen: cap("capture_screen"),
                output: cap("capture_output"),
            },
            facts: Vec::new(),
        }
    }
    fn capture(&self, _request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError> {
        Ok(CapturedImage {
            data: b"\x89PNG fake screenshot".to_vec(),
            format: ImageFormat::Png,
            width: 4,
            height: 3,
            backend: BackendId::PortalScreenshot,
            output: None,
            scale: None,
            notes: Vec::new(),
        })
    }
}

/// Runs the command line on a blocking thread (it owns its runtime) and collects what it printed.
async fn cli(
    args: &[&str],
    secrets: Arc<dyn SecretStore>,
    tty: bool,
    answer: bool,
    with_backend: bool,
) -> Out {
    let args: Vec<OsString> = std::iter::once("kropa")
        .chain(args.iter().copied())
        .map(OsString::from)
        .collect();
    tokio::task::spawn_blocking(move || {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
        let code = {
            let mut io = Io {
                out: &mut out,
                err: &mut err,
                stdin_is_tty: tty,
            };
            let services = Services {
                secrets: secrets.as_ref(),
                input: &NoInput,
                confirm: &move |_| answer,
            };
            let backend = Fake;
            run_with_services(
                &args,
                &|_| None,
                &mut io,
                &clock,
                Some("laptop".into()),
                with_backend.then_some(&backend as &dyn CaptureBackend),
                &|_| {},
                &services,
            )
        };
        Out {
            code: code.code(),
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    })
    .await
    .unwrap()
}

fn none() -> Arc<dyn SecretStore> {
    Arc::new(UnavailableSecretSource)
}

fn config_with(dir: &Path, destinations: &str, default: Option<&str>) {
    std::fs::write(dir.join("destinations.toml"), destinations).unwrap();
    let config = default.map_or_else(String::new, |d| format!("default_destination = \"{d}\"\n"));
    std::fs::write(dir.join("config.toml"), config).unwrap();
}

fn open_destination(server: &MockServer) -> String {
    format!(
        r#"
[[destination]]
id = "open"
type = "http"
[destination.request]
url = "{}/up"
[destination.response]
url = "{{json:$.url}}"
error = "{{json:$.error}}"
"#,
        server.uri()
    )
}

fn shot(dir: &Path) -> String {
    let file = dir.join("shot.png");
    std::fs::write(&file, b"\x89PNG fake screenshot").unwrap();
    file.display().to_string()
}

fn cfg(dir: &Path) -> String {
    dir.display().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_prints_the_url_and_the_json_document() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/up"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"url": "https://files.example.org/a/shot.png"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let file = shot(dir.path());
    let c = cfg(dir.path());

    let r = cli(
        &["--lang", "en", "--config", &c, "upload", &file],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "https://files.example.org/a/shot.png\n");
    assert!(r.err.contains("Uploaded to open"), "{}", r.err);
    assert!(r.err.contains("Metadata is not removed"), "{}", r.err);

    let r = cli(
        &[
            "--json", "--config", &c, "upload", &file, "--to", "open", "--copy", "markdown",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out.lines().count(), 1);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["schema"], "kropa.upload/1");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["upload"]["status"], "done");
    assert_eq!(v["upload"]["destination"], "open");
    assert_eq!(v["upload"]["url"], "https://files.example.org/a/shot.png");
    assert_eq!(v["link"]["format"], "markdown");
    assert_eq!(
        v["link"]["text"],
        "![shot.png](https://files.example.org/a/shot.png)"
    );
    // The clipboard is lot 1.10: the status says so.
    assert_eq!(v["clipboard"]["status"], "unavailable");
    assert_eq!(v["clipboard"]["explicit"], true);
    let codes: Vec<&str> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"clipboard_unavailable") && codes.contains(&"metadata_not_stripped"),
        "{codes:?}"
    );
    assert!(v["error"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_errors_use_codes_8_9_and_the_reason() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(503).set_body_json(serde_json::json!({"error": "overloaded"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let file = shot(dir.path());
    let c = cfg(dir.path());
    let r = cli(
        &["--lang", "en", "--json", "--config", &c, "upload", &file],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 9);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["schema"], "kropa.upload/1");
    assert_eq!(v["status"], "error");
    assert_eq!(v["exit_code"], 9);
    assert_eq!(v["error"]["code"], "upload_failed");
    assert_eq!(v["error"]["reason"], "http_status");
    assert_eq!(v["error"]["step"], "upload");
    assert_eq!(
        v["error"]["message"],
        "The server answered with status 503: overloaded"
    );
    assert_eq!(v["upload"]["status"], "failed");
    assert!(v["upload"]["url"].is_null());
    assert!(r.err.contains("status 503: overloaded"), "{}", r.err);

    // Nobody listens: code 8.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let down =
        open_destination(&server).replace(&server.uri(), &format!("http://127.0.0.1:{port}"));
    config_with(dir.path(), &down, Some("open"));
    let r = cli(
        &["--json", "--config", &c, "upload", &file],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 8);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "connect");
    // The local file is untouched.
    assert!(Path::new(&file).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_with_a_secret_needs_the_store_of_lot_1_8_and_https() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"url": "https://z.example.org/u/abc.png"}]
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let destination = |url: &str| {
        format!(
            "[[destination]]\nid = \"zipline\"\ntype = \"zipline\"\nurl = \"{url}\"\n[destination.secrets]\ntoken = {{}}\n"
        )
    };
    // The address is https here: the transport is checked first, then the store is asked.
    config_with(
        dir.path(),
        &destination("https://z.example.org"),
        Some("zipline"),
    );
    let file = shot(dir.path());
    let c = cfg(dir.path());

    // A build without a store (macOS until lot 1.8m): a clear refusal, nothing sent.
    let r = cli(
        &["--lang", "en", "--json", "--config", &c, "upload", &file],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 5);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "secret_store_not_delivered");
    assert_eq!(v["error"]["lots"][0], "1.8m");
    assert!(r.err.contains("lot 1.8m"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);

    config_with(dir.path(), &destination(&server.uri()), Some("zipline"));
    // With a store holding the token for this origin and place, but an `http://` address: no
    // exception, not even on the loopback (decision U3). Refused with code 11 before any connection;
    // the upload with a secret over HTTPS is tested in `kropa-upload` (a real TLS server).
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
    let r = cli(
        &["--lang", "en", "--json", "--config", &c, "upload", &file],
        Arc::new(store),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 11, "{}", r.err);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "https_required");
    assert!(!r.err.contains(TOKEN) && !r.out.contains(TOKEN));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_echoed_in_the_link_gives_code_11_and_the_local_path() {
    // The detector knows every secret loaded in the run (N5). One is registered here, and a server
    // without any secret echoes it in the link: the CLI must withhold the link. (A secret never
    // travels over `http://`, so this is the way to reach that path from the CLI tests.)
    kropa_upload::masked::register_secret(TOKEN);
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": format!("https://z.example.org/u/{TOKEN}.png")
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let file = shot(dir.path());
    let c = cfg(dir.path());
    let make = none;
    // Human: stdout is the local file, never the link.
    let r = cli(
        &["--lang", "en", "--config", &c, "upload", &file],
        make(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 11);
    assert_eq!(r.out, format!("{file}\n"));
    assert!(
        !r.out.contains(TOKEN) && !r.err.contains(TOKEN),
        "{}",
        r.err
    );
    assert!(r.err.contains("withheld"), "{}", r.err);
    // JSON: failed, no URL, the warning.
    let r = cli(
        &["--json", "--config", &c, "upload", &file],
        make(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 11);
    assert!(!r.out.contains(TOKEN));
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "secret_in_output");
    assert_eq!(v["upload"]["status"], "failed");
    assert!(v["upload"]["url"].is_null());
    assert!(
        v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "uploaded_link_withheld")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_upload_chains_and_keeps_the_file_on_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"url": "https://files.example.org/c.png"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), None);
    let c = cfg(dir.path());
    let out = dir.path().join("shots");
    let o = out.display().to_string();
    let r = cli(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "capture",
            "screen",
            "--output",
            &o,
            "--upload=open",
        ],
        none(),
        false,
        false,
        true,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "https://files.example.org/c.png\n");
    assert!(
        r.err.contains("Saved:") && r.err.contains("Uploaded to open"),
        "{}",
        r.err
    );
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);

    // JSON.
    let out2 = dir.path().join("shots2").display().to_string();
    let r = cli(
        &[
            "--json",
            "--config",
            &c,
            "capture",
            "screen",
            "--output",
            &out2,
            "--upload=open",
        ],
        none(),
        false,
        false,
        true,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["schema"], "kropa.capture/1");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["upload"]["url"], "https://files.example.org/c.png");
    assert_eq!(v["link"]["format"], "url");

    // The upload fails: the file stays, stdout is its path, the code is the upload's, status partial.
    server.reset().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let out3 = dir.path().join("shots3").display().to_string();
    let r = cli(
        &[
            "--json",
            "--config",
            &c,
            "capture",
            "screen",
            "--output",
            &out3,
            "--upload=open",
        ],
        none(),
        false,
        false,
        true,
    )
    .await;
    assert_eq!(r.code, 9);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["status"], "partial");
    assert_eq!(v["exit_code"], 9);
    assert_eq!(v["upload"]["status"], "failed");
    assert_eq!(v["error"]["step"], "upload");
    let saved = v["file"]["path"].as_str().unwrap();
    assert!(Path::new(saved).exists());
    let r = cli(
        &[
            "--config",
            &c,
            "capture",
            "screen",
            "--output",
            &dir.path().join("shots4").display().to_string(),
            "--upload=open",
        ],
        none(),
        false,
        false,
        true,
    )
    .await;
    assert_eq!(r.code, 9);
    assert!(
        r.out.trim_end().ends_with(".png") && Path::new(r.out.trim_end()).exists(),
        "{}",
        r.out
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dest_list_show_and_test() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(|req: &wiremock::Request| req.body.windows(8).any(|w| w == b"\x89PNG\r\n\x1a\n"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"url": "https://files.example.org/t.png"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let c = cfg(dir.path());

    let r = cli(
        &["--lang", "en", "--config", &c, "dest", "list"],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0);
    assert!(
        r.out.starts_with("open\thttp\thttp://127.0.0.1:"),
        "{}",
        r.out
    );
    assert!(r.out.contains("(default)"));
    let r = cli(
        &["--json", "--config", &c, "dest", "list"],
        none(),
        false,
        false,
        false,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["schema"], "kropa.dest.list/1");
    assert_eq!(v["destinations"][0]["id"], "open");
    assert_eq!(v["destinations"][0]["default"], true);

    let r = cli(
        &["--lang", "en", "--config", &c, "dest", "show", "open"],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out.contains("open (http)") && r.out.contains("Default destination: yes"),
        "{}",
        r.out
    );
    let r = cli(
        &["--config", &c, "dest", "show", "nope"],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 6);

    // `test` needs a confirmation: refused without a terminal and without --yes (12).
    let r = cli(
        &["--config", &c, "dest", "test", "open"],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 12);
    // Declined on a terminal: 3.
    let r = cli(
        &["--config", &c, "dest", "test", "open"],
        none(),
        true,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 3);
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    let r = cli(
        &[
            "--lang", "en", "--config", &c, "dest", "test", "open", "--yes",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out.contains("Status: 200") && r.out.contains("URL: https://files.example.org/t.png"),
        "{}",
        r.out
    );
}

const PUBLIC_SXCU: &str = r#"{"Version":"16.0.1","Name":"Open Gallery","DestinationType":"ImageUploader","RequestURL":"https://open.example.org/up","Body":"MultipartFormData","URL":"{json:link}"}"#;

#[tokio::test(flavor = "multi_thread")]
async fn dest_import_rules() {
    let dir = tempfile::tempdir().unwrap();
    let c = cfg(dir.path());
    let public = dir.path().join("open.sxcu");
    std::fs::write(&public, PUBLIC_SXCU).unwrap();
    let p = public.display().to_string();
    let secret = dir.path().join("zip.sxcu");
    std::fs::write(
        &secret,
        include_str!("../../kropa-upload/tests/fixtures/zipline.sxcu"),
    )
    .unwrap();
    let s = secret.display().to_string();

    // Without a terminal: --yes AND --expect-host are required (12).
    for args in [
        vec!["--config", &c, "dest", "import", &p],
        vec!["--config", &c, "dest", "import", &p, "--yes"],
        vec![
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--expect-host",
            "open.example.org",
        ],
    ] {
        let r = cli(&args, none(), false, false, false).await;
        assert_eq!(r.code, 12, "{args:?}: {}", r.err);
    }
    assert!(!dir.path().join("destinations.toml").exists());

    // A host that is not the expected one: 11, nothing written.
    let r = cli(
        &[
            "--json",
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--yes",
            "--expect-host",
            "evil.example.com",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 11);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "import_host_mismatch");
    assert!(!dir.path().join("destinations.toml").exists());

    // The right host: the profile is written.
    let r = cli(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--yes",
            "--expect-host",
            "open.example.org",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "open-gallery\n");
    assert!(r.err.contains("open.example.org"), "{}", r.err);
    let written = std::fs::read_to_string(dir.path().join("destinations.toml")).unwrap();
    assert!(written.contains("id = \"open-gallery\""), "{written}");

    // Same id again: refused (6), pointing at --id.
    let r = cli(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--yes",
            "--expect-host",
            "open.example.org",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 6);
    assert!(r.err.contains("--id"), "{}", r.err);
    let r = cli(
        &[
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--id",
            "second",
            "--yes",
            "--expect-host",
            "open.example.org",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);

    // A file with a real secret and no store yet: a clear refusal, nothing written.
    let before = std::fs::read_to_string(dir.path().join("destinations.toml")).unwrap();
    let r = cli(
        &[
            "--lang",
            "en",
            "--json",
            "--config",
            &c,
            "dest",
            "import",
            &s,
            "--yes",
            "--expect-host",
            "zipline.example.org",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 5);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "secret_store_not_delivered");
    assert_eq!(v["error"]["lots"][0], "1.8m");
    assert!(!r.out.contains("ZIPLINE-FAKE") && !r.err.contains("ZIPLINE-FAKE"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("destinations.toml")).unwrap(),
        before
    );

    // With a store, the secret is handed over tied to its origin and place.
    let store = Arc::new(MemorySecretSource::new());
    let r = cli(
        &[
            "--config",
            &c,
            "dest",
            "import",
            &s,
            "--yes",
            "--expect-host",
            "zipline.example.org",
        ],
        store.clone(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);
    let entry = store
        .load("zipline-team", "authorization")
        .unwrap()
        .unwrap();
    assert_eq!(entry.origins, ["https://zipline.example.org:443"]);
    assert_eq!(entry.placements, [Placement::header("authorization")]);
    // The import never reuses a store entry: the profile id is now taken in the store too.
    let again = cli(
        &[
            "--config",
            &c,
            "dest",
            "import",
            &s,
            "--id",
            "zipline-team",
            "--yes",
            "--expect-host",
            "zipline.example.org",
        ],
        store.clone(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(again.code, 6);

    // Interactive: declined (3), accepted (0).
    let r = cli(
        &[
            "--config",
            &c,
            "dest",
            "import",
            &p,
            "--id",
            "third",
            "--expect-host",
            "open.example.org",
        ],
        none(),
        true,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 3);
    let r = cli(
        &["--config", &c, "dest", "import", &p, "--id", "third"],
        none(),
        true,
        true,
        false,
    )
    .await;
    assert_eq!(r.code, 0, "{}", r.err);

    // Unknown file type.
    let odd = dir.path().join("x.json");
    std::fs::write(&odd, "{}").unwrap();
    let r = cli(
        &[
            "--config",
            &c,
            "dest",
            "import",
            &odd.display().to_string(),
            "--yes",
            "--expect-host",
            "a.example.org",
        ],
        none(),
        false,
        false,
        false,
    )
    .await;
    assert_eq!(r.code, 6);
}

// ---------------------------------------------------------------------------------------------
// Task 1.9: no secret on stdout, stderr or in the JSON, at `-vv`, even when the server echoes it
// ---------------------------------------------------------------------------------------------

/// Built at run time: nothing here looks like a token in the source.
fn journal_token() -> String {
    ["cli9-", "Zq7kLm9", "Xt2+vRw", "/abcdef-QW"].concat()
}

/// The parts of the secret that must not come out: every window of 8 characters of each form
/// (raw, percent-encoded whole or in part, hex, JSON escapes, HTML entities, and the part of the
/// base64 in both alphabets that does not depend on the neighbours, at the three alignments).
fn leaked_forms(text: &str, secret: &str) -> Vec<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
    let special = |b: u8| !b.is_ascii_alphanumeric() && !matches!(b, b'-' | b'.' | b'_' | b'~');
    let mut forms = vec![secret.to_owned()];
    for lower in [false, true] {
        let enc = |b: u8| {
            if lower {
                format!("{b:02x}")
            } else {
                format!("{b:02X}")
            }
        };
        let (mut pct, mut hex, mut json, mut uni, mut dec, mut hexent) = (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
        let (mut only_plus, mut only_slash) = (String::new(), String::new());
        for b in secret.bytes() {
            let c = char::from(b);
            hex.push_str(&enc(b));
            if special(b) {
                pct.push_str(&format!("%{}", enc(b)));
                uni.push_str(&format!("\\u00{}", enc(b)));
                json.push_str(&if b == b'/' {
                    "\\/".to_owned()
                } else {
                    format!("\\u00{}", enc(b))
                });
                dec.push_str(&format!("&#{b};"));
                hexent.push_str(&format!("&#x{};", enc(b).trim_start_matches('0')));
            } else {
                for out in [&mut pct, &mut uni, &mut json, &mut dec, &mut hexent] {
                    out.push(c);
                }
            }
            for (only, target) in [(&mut only_plus, b'+'), (&mut only_slash, b'/')] {
                if b == target {
                    only.push_str(&format!("%{}", enc(b)));
                } else {
                    only.push(c);
                }
            }
        }
        forms.extend([pct, hex, json, uni, dec, hexent, only_plus, only_slash]);
    }
    for pad in 0..3 {
        let mut bytes = vec![b'#'; pad];
        bytes.extend_from_slice(secret.as_bytes());
        for e in [
            STANDARD_NO_PAD.encode(&bytes),
            URL_SAFE_NO_PAD.encode(&bytes),
        ] {
            forms.push(e[4..e.len() - 4].to_owned());
        }
    }
    let mut found = Vec::new();
    for form in forms {
        let chars: Vec<char> = form.chars().collect();
        for w in chars.windows(8) {
            let w: String = w.iter().collect();
            if text.contains(w.as_str()) && !found.contains(&w) {
                found.push(w);
            }
        }
    }
    found
}

fn echoing(status: u16, body: serde_json::Value, secret: &str) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("x-echo", secret)
        .insert_header("set-cookie", format!("sid={secret}").as_str())
        .insert_header("www-authenticate", format!("Bearer {secret}").as_str())
        .set_body_json(body)
}

#[tokio::test(flavor = "multi_thread")]
async fn no_secret_reaches_stdout_stderr_or_the_json_at_vv() {
    use base64::Engine as _;
    let secret = journal_token();
    // A secret loaded in the process since its start (N5): the detector knows it whatever the
    // request. The TLS path with a real store is covered in `kropa-upload` (`log_tests`).
    kropa_upload::masked::register_secret(&secret);
    let standard = base64::engine::general_purpose::STANDARD;
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let file = shot(dir.path());
    let c = cfg(dir.path());
    let mut seen = String::new();
    let mut journal_lines = 0;

    // Success: the secret comes back in headers and in fields of the body that are not read.
    server.reset().await;
    Mock::given(any())
        .respond_with(echoing(
            200,
            serde_json::json!({
                "url": "https://files.example.org/a/shot.png?name=a%20b",
                "debug": [secret, standard.encode(format!("user:{secret}"))],
            }),
            &secret,
        ))
        .mount(&server)
        .await;
    for args in [
        vec!["-vv", "--lang", "en", "upload", &file],
        vec!["-vv", "--json", "upload", &file, "--copy", "markdown"],
        vec!["-vv", "--json", "upload", &file, "--copy", "html"],
        vec!["-vv", "--lang", "fr", "dest", "test", "open", "--yes"],
        vec!["-vv", "--json", "dest", "test", "open", "--yes"],
    ] {
        let mut full = vec!["--config", c.as_str()];
        full.extend(args);
        let r = cli(&full, none(), false, true, false).await;
        assert_eq!(r.code, 0, "{full:?}: {}", r.err);
        journal_lines += r
            .err
            .lines()
            .filter(|l| l.starts_with("kropa-upload: "))
            .count();
        seen.push_str(&format!("{}\n{}\n", r.out, r.err));
    }

    // Failure: the answer holds the secret in several forms.
    for body in [
        serde_json::json!({"error": format!("bad credentials {secret}")}),
        serde_json::json!({"error": standard.encode(format!("Bearer {secret}"))}),
        serde_json::json!({"error": secret.replace('/', "%2F").replace('+', "%2B")}),
    ] {
        server.reset().await;
        Mock::given(any())
            .respond_with(echoing(500, body, &secret))
            .mount(&server)
            .await;
        for json in [false, true] {
            let mut full = vec!["--config", c.as_str(), "-vv", "--lang", "en"];
            if json {
                full.push("--json");
            }
            full.extend(["upload", file.as_str()]);
            let r = cli(&full, none(), false, false, false).await;
            assert_eq!(r.code, 9, "{}", r.err);
            assert!(
                r.err.contains("kropa-upload: response body (error):"),
                "{}",
                r.err
            );
            journal_lines += r
                .err
                .lines()
                .filter(|l| l.starts_with("kropa-upload: "))
                .count();
            seen.push_str(&format!("{}\n{}\n", r.out, r.err));
        }
    }
    // The proof is not an empty one: the journal spoke, and it was redacted.
    assert!(journal_lines >= 30, "{journal_lines}");
    assert!(
        seen.contains("kropa-upload: request: POST http://127.0.0.1:"),
        "{seen}"
    );
    assert!(seen.contains("other header(s): ***"), "{seen}");
    assert!(seen.contains("set-cookie: ***"), "{seen}");
    assert_eq!(leaked_forms(&seen, &secret), Vec::<String>::new(), "{seen}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hostile_link_never_reaches_stdout() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    config_with(dir.path(), &open_destination(&server), Some("open"));
    let file = shot(dir.path());
    let c = cfg(dir.path());
    for url in ["javascript:alert(1)", "ftp://files.example.org/a.png"] {
        server.reset().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"url": url})))
            .mount(&server)
            .await;
        let r = cli(
            &["--config", &c, "--json", "upload", &file, "--copy", "html"],
            none(),
            false,
            false,
            false,
        )
        .await;
        assert_eq!(r.code, 9, "{url}: {}", r.err);
        assert!(!r.out.contains(url), "{}", r.out);
    }
}

/// The CLI side of the HTTPS paths without the CA of the tests (decision U3: the CA exists in
/// `kropa-upload` only). The secret is read from the store and the destination is `https://`; the
/// server behind it speaks plain HTTP, so the handshake fails, but only after the lines `request:`
/// and `request headers:` were written. They must show `api_key=***`, `ref=***` (not named like a
/// secret: the detector alone hides it) and `authorization: ***` on stderr, and nothing of the
/// secret on stdout, stderr or in the JSON.
#[tokio::test(flavor = "multi_thread")]
async fn a_secret_read_from_the_store_is_hidden_in_the_request_lines_when_the_handshake_fails() {
    let secret = journal_token();
    let server = MockServer::start().await;
    let https = server.uri().replacen("http://", "https://", 1);
    let dir = tempfile::tempdir().unwrap();
    let destination = format!(
        r#"
[[destination]]
id = "journal"
type = "http"
[destination.request]
url = "{https}/up"
query = {{ api_key = "{{secret:token}}", ref = "{{secret:token}}", title = "{{filename}}" }}
headers = {{ Authorization = "Bearer {{secret:token}}" }}
[destination.secrets]
token = {{}}
[destination.response]
url = "{{json:$.url}}"
"#
    );
    config_with(dir.path(), &destination, Some("journal"));
    let store = MemorySecretSource::new();
    store
        .insert(
            "journal",
            "token",
            &secret,
            &[canonical_origin(&https).unwrap().as_str()],
            vec![
                Placement::header("Authorization"),
                Placement::Query("api_key".into()),
                Placement::Query("ref".into()),
            ],
        )
        .unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(store);
    let file = shot(dir.path());
    let c = cfg(dir.path());
    let mut seen = String::new();
    for json in [false, true] {
        let mut args = vec!["--config", c.as_str(), "-vv", "--lang", "en"];
        if json {
            args.push("--json");
        }
        args.extend(["upload", file.as_str()]);
        let r = cli(&args, store.clone(), false, false, false).await;
        assert_ne!(r.code, 0, "{}", r.err);
        for expected in [
            "kropa-upload: request: POST https://127.0.0.1:",
            "api_key=***",
            "ref=***",
            "kropa-upload: request headers: authorization: ***",
        ] {
            assert!(
                r.err.contains(expected),
                "{expected:?} missing in:\n{}",
                r.err
            );
        }
        seen.push_str(&format!("{}\n{}\n", r.out, r.err));
    }
    assert!(seen.contains("title=shot.png"), "{seen}");
    assert_eq!(leaked_forms(&seen, &secret), Vec::<String>::new(), "{seen}");
}

/// The oracle above is not an empty one: it sees a part of the secret in several forms and nothing
/// in a harmless text.
#[test]
fn the_cli_oracle_sees_partial_leaks() {
    let secret = journal_token();
    let pct = secret.replace('+', "%2B");
    for part in [
        &secret[3..15],
        &pct[2..18],
        &secret.replace('/', "\\/")[8..24],
        &secret.replace('+', "\\u002b")[4..22],
    ] {
        assert!(
            !leaked_forms(&format!("x {part} y"), &secret).is_empty(),
            "{part}"
        );
    }
    assert_eq!(
        leaked_forms("kropa: nothing to see here", &secret),
        Vec::<String>::new()
    );
}
