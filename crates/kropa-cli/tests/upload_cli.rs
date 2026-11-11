// SPDX-License-Identifier: GPL-3.0-or-later

//! `upload`, `dest list|show|test|import` and `capture --upload` end to end, in process, against
//! `wiremock` servers on the loopback and an in-memory secret store. Nothing leaves the machine.

#![allow(
    clippy::many_single_char_names,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use kropa_capture::{
    BackendId, Capabilities, Capability, CaptureBackend, CaptureError, CaptureRequest,
    CapturedImage, Diagnosis, ImageFormat, Status,
};
use kropa_cli::{Io, Services, run_with_services};
use kropa_core::FixedClock;
use kropa_upload::origin::canonical_origin;
use kropa_upload::secret::{MemorySecretSource, Placement, SecretSource, UnavailableSecretSource};
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = concat!("cli-", "Zq7kLm9", "Xt2vRw-abcdef");

struct Out {
    code: u8,
    out: String,
    err: String,
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
    secrets: Arc<dyn SecretSource>,
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

fn none() -> Arc<dyn SecretSource> {
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

    // No store yet: a clear refusal that points at lot 1.8, nothing sent.
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
    assert_eq!(v["error"]["lots"][0], "1.8");
    assert!(r.err.contains("lot 1.8"));
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
    assert_eq!(v["error"]["lots"][0], "1.8");
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
