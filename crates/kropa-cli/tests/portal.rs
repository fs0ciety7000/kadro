// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa capture` and `kropa doctor` against a fake `org.freedesktop.portal.Desktop`.
//!
//! Each test starts its own private `dbus-daemon` (no service directory, so nothing can be
//! activated: the real portal of the machine that runs the test is never reached), serves a fake
//! Screenshot portal on it from this process, and runs the real `kropa` binary with
//! `DBUS_SESSION_BUS_ADDRESS` pointing at it. No display, no compositor and no session are needed;
//! `dbus-daemon` is. What this proves: the D-Bus conversation (Registry, Screenshot request,
//! `Response` signal), the mapping of every answer to an exit code and an `error.reason`, the
//! reading of the returned `file://` URI, the metadata stripping and the saved file. What it cannot
//! prove: that GNOME or KDE answer the same way (see `docs/testing/manual-linux.md`).
#![cfg(target_os = "linux")]
#![expect(
    clippy::needless_pass_by_value,
    reason = "the argument types of the fake D-Bus methods are fixed by the interface macro"
)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kropa_capture::sample_png;
use serde_json::Value;
use zbus::{
    Connection, DBusError, OwnedObjectPath, OwnedVariant as OwnedValue, Variant as DbusValue,
    connection, interface, message::Header,
};

const DAEMON_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

const REQUEST_PREFIX: &str = "/org/freedesktop/portal/desktop/request";

/// A private message bus, killed when dropped.
struct Bus {
    daemon: Child,
    address: String,
    _dir: tempfile::TempDir,
}

impl Bus {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("bus.conf");
        std::fs::write(&config, DAEMON_CONFIG).unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required by this test (package `dbus` / `dbus-daemon`)");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_owned();
        assert!(address.starts_with("unix:"), "bus address: {address:?}");
        Self {
            daemon,
            address,
            _dir: dir,
        }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// What the fake portal does when asked for a screenshot.
#[derive(Clone)]
enum Answer {
    /// Writes this file in the temporary folder and answers with its `file://` URI.
    Png(Vec<u8>),
    /// Answers with this URI, whatever it is.
    Uri(String),
    /// Ends the request with this response code (1 dismissed, 2 other) and no result.
    Response(u32),
    /// Replies with the D-Bus error `org.freedesktop.portal.Error.NotAllowed`.
    NotAllowed,
    /// Replies with the D-Bus error `org.freedesktop.portal.Error.Failed`.
    Failed,
}

struct State {
    answer: Answer,
    version: u32,
    targets: u32,
    /// `interactive` option of each `Screenshot` call.
    calls: Mutex<Vec<Option<bool>>>,
    /// Application identifiers given to `Registry.Register`.
    registered: Mutex<Vec<String>>,
    /// Files written for the screenshots.
    written: Mutex<Vec<PathBuf>>,
}

#[derive(DBusError, Debug)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    NotAllowed(String),
    NotFound(String),
}

struct Screenshot(Arc<State>);

#[interface(name = "org.freedesktop.portal.Screenshot")]
impl Screenshot {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        self.0.version
    }

    #[zbus(property, name = "AvailableTargets")]
    fn available_targets(&self) -> u32 {
        self.0.targets
    }

    fn screenshot(
        &self,
        parent_window: &str,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        let _ = parent_window;
        let state = &self.0;
        let interactive = options
            .get("interactive")
            .and_then(|v| bool::try_from(v).ok());
        state.calls.lock().unwrap().push(interactive);
        let token = options
            .get("handle_token")
            .and_then(|v| String::try_from(v.clone()).ok())
            .unwrap_or_default();
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        let path = format!(
            "{REQUEST_PREFIX}/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        let (code, uri) = match &state.answer {
            Answer::Png(bytes) => {
                let file = std::env::temp_dir().join(format!(
                    "kropa-fake-portal-{}-{token}.png",
                    std::process::id()
                ));
                std::fs::write(&file, bytes).unwrap();
                state.written.lock().unwrap().push(file.clone());
                (0, Some(format!("file://{}", file.display())))
            }
            Answer::Uri(uri) => (0, Some(uri.clone())),
            Answer::Response(code) => (*code, None),
            Answer::NotAllowed => return Err(PortalError::NotAllowed("denied".into())),
            Answer::Failed => return Err(PortalError::Failed("the compositor said no".into())),
        };
        // A real portal answers later, after the user did something: the signal follows the reply.
        let connection = connection.clone();
        let signal_path = path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut results: HashMap<String, DbusValue<'_>> = HashMap::new();
            if let Some(uri) = uri {
                results.insert("uri".to_owned(), DbusValue::from(uri));
            }
            zbus::block_on(connection.emit_signal(
                Some(sender.as_str()),
                signal_path.as_str(),
                "org.freedesktop.portal.Request",
                "Response",
                &(code, results),
            ))
            .unwrap();
        });
        Ok(OwnedObjectPath::try_from(path).unwrap())
    }
}

struct Registry(Arc<State>);

#[interface(name = "org.freedesktop.host.portal.Registry")]
impl Registry {
    fn register(&self, app_id: &str, options: HashMap<String, OwnedValue>) {
        let _ = options;
        self.0.registered.lock().unwrap().push(app_id.to_owned());
    }
}

/// The fake portal on the bus, alive while this value is.
struct Portal {
    state: Arc<State>,
    _connection: Connection,
}

impl Portal {
    fn serve(bus: &Bus, answer: Answer) -> Self {
        Self::serve_with(bus, answer, true)
    }

    /// `registry: false` imitates a portal older than the `Registry` interface.
    fn serve_with(bus: &Bus, answer: Answer, registry: bool) -> Self {
        let state = Arc::new(State {
            answer,
            version: 3,
            targets: 1 | 2 | 4,
            calls: Mutex::default(),
            registered: Mutex::default(),
            written: Mutex::default(),
        });
        let path = "/org/freedesktop/portal/desktop";
        let mut builder = connection::Builder::address(bus.address.as_str())
            .name("org.freedesktop.portal.Desktop")
            .serve_at(path, Screenshot(state.clone()));
        if registry {
            builder = builder.serve_at(path, Registry(state.clone()));
        }
        let connection = zbus::block_on(builder.build()).expect("the fake portal starts");
        Self {
            state,
            _connection: connection,
        }
    }

    fn calls(&self) -> usize {
        self.state.calls.lock().unwrap().len()
    }
}

/// What the fake permission store holds, and what it was asked.
struct StoreState {
    /// `(table, id, app)` of the stored answers.
    entries: Mutex<Vec<(String, String, String)>>,
    /// When set, every call fails with the D-Bus error `org.freedesktop.portal.Error.Failed`.
    broken: bool,
    /// Arguments of each `DeletePermission` call.
    calls: Mutex<Vec<(String, String, String)>>,
}

struct PermissionStore(Arc<StoreState>);

#[interface(name = "org.freedesktop.impl.portal.PermissionStore")]
impl PermissionStore {
    fn delete_permission(&self, table: &str, id: &str, app: &str) -> Result<(), PortalError> {
        let key = (table.to_owned(), id.to_owned(), app.to_owned());
        self.0.calls.lock().unwrap().push(key.clone());
        if self.0.broken {
            return Err(PortalError::Failed("the store is broken".into()));
        }
        let mut entries = self.0.entries.lock().unwrap();
        match entries.iter().position(|e| *e == key) {
            Some(i) => {
                entries.remove(i);
                Ok(())
            }
            None => Err(PortalError::NotFound(format!("No entry for {id}"))),
        }
    }
}

/// The fake permission store on the bus, alive while this value is.
struct Store {
    state: Arc<StoreState>,
    _connection: Connection,
}

impl Store {
    fn serve(bus: &Bus, entries: &[(&str, &str, &str)], broken: bool) -> Self {
        let state = Arc::new(StoreState {
            entries: Mutex::new(
                entries
                    .iter()
                    .map(|(t, i, a)| ((*t).to_owned(), (*i).to_owned(), (*a).to_owned()))
                    .collect(),
            ),
            broken,
            calls: Mutex::default(),
        });
        let connection = zbus::block_on(
            connection::Builder::address(bus.address.as_str())
                .name("org.freedesktop.impl.portal.PermissionStore")
                .serve_at(
                    "/org/freedesktop/impl/portal/PermissionStore",
                    PermissionStore(state.clone()),
                )
                .build(),
        )
        .expect("the fake permission store starts");
        Self {
            state,
            _connection: connection,
        }
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not one JSON object ({e}): {:?} / stderr {:?}",
                self.stdout, self.stderr
            )
        })
    }
}

/// Runs `kropa` against `bus` in an otherwise empty environment.
fn kropa(bus: &Bus, home: &Path, args: &[&str]) -> Run {
    kropa_at(&bus.address, home, args)
}

/// Like [`kropa`], on the session bus at `address`.
fn kropa_at(address: &str, home: &Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_kropa"))
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_DIRS", home.join("no-data"))
        // The fake portal writes its files under this process's temporary folder.
        .env("TMPDIR", std::env::temp_dir())
        .env("XDG_CURRENT_DESKTOP", "FakeDesktop")
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .args(if args.contains(&"--lang") {
            &[][..]
        } else {
            &["--lang", "en"][..]
        })
        .args(args)
        .output()
        .expect("run kropa");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn png_with_metadata() -> Vec<u8> {
    sample_png(
        800,
        600,
        &[
            (b"tEXt", b"Software\0fake-portal"),
            (b"tIME", &[7, 234, 11, 11, 14, 3, 22]),
            (b"gAMA", &[0, 1, 0x86, 0xa0]),
        ],
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_screen_capture_is_saved_without_metadata() {
    let bus = Bus::start();
    let portal = Portal::serve(&bus, Answer::Png(png_with_metadata()));
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("out/shot.png");
    let run = kropa(
        &bus,
        home.path(),
        &[
            "--json",
            "capture",
            "screen",
            "--output",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["schema"], "kropa.capture/1");
    assert_eq!(doc["status"], "ok");
    assert_eq!(doc["capture"]["mode"], "screen");
    assert_eq!(doc["capture"]["backend"], "portal-screenshot");
    assert_eq!(doc["file"]["width"], 800);
    assert_eq!(doc["file"]["height"], 600);
    assert_eq!(doc["file"]["path"], target.to_str().unwrap());
    let saved = std::fs::read(&target).unwrap();
    assert_eq!(doc["file"]["size"], saved.len());
    assert!(saved.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(!contains(&saved, b"tEXt"), "text chunk kept");
    assert!(!contains(&saved, b"tIME"), "time chunk kept");
    assert!(contains(&saved, b"gAMA"), "colour chunk lost");
    assert!(run.stdout.lines().count() == 1, "one JSON line");
    // The portal was asked for a non-interactive screenshot, under the application identifier.
    assert_eq!(*portal.state.calls.lock().unwrap(), [Some(false)]);
    assert_eq!(
        *portal.state.registered.lock().unwrap(),
        ["io.github.fs0ciety7000.Kropa"]
    );
    // The portal's own copy of the whole desktop does not stay in the temporary folder.
    for file in portal.state.written.lock().unwrap().iter() {
        assert!(!file.exists(), "{} left behind", file.display());
    }
}

#[test]
fn human_output_is_the_absolute_path_on_stdout() {
    let bus = Bus::start();
    let _portal = Portal::serve(&bus, Answer::Png(png_with_metadata()));
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("pics");
    let run = kropa(
        &bus,
        home.path(),
        &[
            "capture",
            "screen",
            "--output",
            dir.to_str().unwrap(),
            "--name",
            "s-{w}x{h}",
        ],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    let expected = dir.join("s-800x600.png");
    assert_eq!(run.stdout, format!("{}\n", expected.display()));
    assert!(expected.is_file());
    assert!(run.stderr.contains("Saved:"), "{}", run.stderr);
}

#[test]
fn an_older_portal_without_registry_still_captures() {
    let bus = Bus::start();
    let portal = Portal::serve_with(&bus, Answer::Png(png_with_metadata()), false);
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("shot.png");
    let run = kropa(
        &bus,
        home.path(),
        &[
            "-v",
            "capture",
            "screen",
            "--output",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(target.is_file());
    assert!(portal.state.registered.lock().unwrap().is_empty());
    assert!(
        run.stderr.contains("no application registry"),
        "{}",
        run.stderr
    );
}

fn refused(answer: Answer, code: i32, reason: &str) {
    let bus = Bus::start();
    let portal = Portal::serve(&bus, answer);
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("shot.png");
    let run = kropa(
        &bus,
        home.path(),
        &[
            "--json",
            "capture",
            "screen",
            "--output",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, code, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["status"], "error");
    assert_eq!(doc["exit_code"], code);
    assert_eq!(doc["error"]["reason"], reason);
    assert_eq!(doc["error"]["step"], "capture");
    assert_eq!(portal.calls(), 1);
    assert!(!target.exists(), "no file after a failed capture");
}

#[test]
fn a_refusal_by_the_portal_is_permission_denied() {
    // Response 2 is what GNOME and KDE answer when the permission was denied.
    refused(Answer::Response(2), 4, "portal_declined");
}

#[test]
fn a_dismissed_request_is_permission_denied() {
    refused(Answer::Response(1), 4, "portal_dismissed");
}

#[test]
fn a_not_allowed_error_is_permission_denied() {
    refused(Answer::NotAllowed, 4, "portal_not_allowed");
}

#[test]
fn a_portal_error_is_a_capture_failure() {
    refused(Answer::Failed, 10, "portal_error");
}

#[test]
fn a_garbage_image_is_a_capture_failure() {
    refused(
        Answer::Png(b"definitely not a png".to_vec()),
        10,
        "invalid_image",
    );
}

#[test]
fn a_remote_uri_is_a_capture_failure() {
    refused(
        Answer::Uri("https://example.org/shot.png".into()),
        10,
        "portal_invalid_response",
    );
}

#[test]
fn a_missing_file_is_a_capture_failure() {
    refused(
        Answer::Uri("file:///nonexistent-kropa-dir/shot.png".into()),
        10,
        "portal_file_unreadable",
    );
}

#[test]
fn no_portal_on_the_bus_is_a_capture_failure_with_the_fallback_in_the_message() {
    let bus = Bus::start();
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("shot.png");
    let run = kropa(
        &bus,
        home.path(),
        &["capture", "screen", "--output", target.to_str().unwrap()],
    );
    assert_eq!(run.code, 10, "{}", run.stderr);
    assert!(run.stderr.contains("xdg-desktop-portal"), "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(!target.exists());
}

#[test]
fn one_screen_cannot_be_isolated_and_nothing_is_asked() {
    let bus = Bus::start();
    let portal = Portal::serve(&bus, Answer::Png(png_with_metadata()));
    let home = tempfile::tempdir().unwrap();
    let run = kropa(
        &bus,
        home.path(),
        &[
            "--json",
            "capture",
            "output",
            "DP-1",
            "--output",
            home.path().to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, 5, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["error"]["code"], "unsupported");
    assert_eq!(doc["error"]["reason"], "output_not_isolable");
    assert_eq!(portal.calls(), 0, "the portal must not be bothered");
}

#[test]
fn an_existing_file_is_refused_before_the_portal_is_asked() {
    let bus = Bus::start();
    let portal = Portal::serve(&bus, Answer::Png(png_with_metadata()));
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("shot.png");
    std::fs::write(&target, b"precious").unwrap();
    let run = kropa(
        &bus,
        home.path(),
        &["capture", "screen", "--output", target.to_str().unwrap()],
    );
    assert_eq!(run.code, 15, "{}", run.stderr);
    assert_eq!(portal.calls(), 0);
    assert_eq!(std::fs::read(&target).unwrap(), b"precious");
    let forced = kropa(
        &bus,
        home.path(),
        &[
            "capture",
            "screen",
            "--force",
            "--output",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(forced.code, 0, "{}", forced.stderr);
    assert!(std::fs::read(&target).unwrap().starts_with(&[0x89, b'P']));
}

fn fact<'a>(doc: &'a Value, key: &str) -> &'a Value {
    doc["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap_or_else(|| panic!("no fact {key}"))
}

#[test]
fn doctor_reports_the_portal_it_finds() {
    let bus = Bus::start();
    let _portal = Portal::serve(&bus, Answer::Response(2));
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["schema"], "kropa.doctor/1");
    assert_eq!(doc["backend"], "portal-screenshot");
    assert_eq!(doc["capabilities"][0]["feature"], "capture_screen");
    assert_eq!(doc["capabilities"][0]["status"], "available");
    assert_eq!(doc["capabilities"][1]["status"], "unavailable");
    assert_eq!(doc["capabilities"][1]["reason"], "output_not_isolable");
    assert_eq!(doc["capabilities"][1]["fallback"], "capture_screen_instead");
    assert_eq!(doc["fallbacks"][0], "capture_screen_instead");
    assert_eq!(fact(&doc, "session_bus")["value"], true);
    assert_eq!(fact(&doc, "portal_screenshot")["value"], 3);
    assert_eq!(
        fact(&doc, "portal_screenshot_targets")["value"],
        "screen,window,area"
    );
    // Only the interfaces the fake serves are found; Registry has no version property there.
    assert_eq!(fact(&doc, "portal_screencast")["state"], "absent");
    // The desktop name is information, nothing more.
    assert_eq!(fact(&doc, "desktop")["value"], "FakeDesktop");
}

#[test]
fn doctor_without_portal_says_so_and_still_succeeds() {
    let bus = Bus::start();
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["capabilities"][0]["status"], "unavailable");
    assert_eq!(doc["capabilities"][0]["reason"], "portal_unavailable");
    assert_eq!(fact(&doc, "session_bus")["value"], true);
    assert_eq!(fact(&doc, "portal_screenshot")["state"], "absent");
}

#[test]
fn doctor_in_french_is_translated() {
    let bus = Bus::start();
    let _portal = Portal::serve(&bus, Answer::Response(2));
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--lang", "fr", "doctor"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("Diagnostic de Kropa"), "{}", run.stdout);
    assert!(run.stdout.contains("disponible"), "{}", run.stdout);
    assert!(
        run.stdout
            .contains("Portail Screenshot, version de l'interface : 3"),
        "{}",
        run.stdout
    );
}

const APP: &str = "io.github.fs0ciety7000.Kropa";
const RESET: &str = "--reset-screenshot-permission";

fn entry(app: &str) -> (String, String, String) {
    (
        "screenshot".to_owned(),
        "screenshot".to_owned(),
        app.to_owned(),
    )
}

#[test]
fn resetting_deletes_the_stored_answer_and_says_so() {
    let bus = Bus::start();
    let store = Store::serve(
        &bus,
        &[
            ("screenshot", "screenshot", APP),
            ("screenshot", "screenshot", "org.example.Other"),
        ],
        false,
    );
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor", RESET]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["schema"], "kropa.doctor.reset/1");
    assert_eq!(doc["status"], "ok");
    assert_eq!(doc["exit_code"], 0);
    assert_eq!(doc["permission"], "screenshot");
    assert_eq!(doc["result"], "removed");
    assert_eq!(run.stdout.lines().count(), 1, "one JSON line");
    // Exactly the call of the manual `busctl` command, and only Kropa's own entry goes.
    assert_eq!(*store.state.calls.lock().unwrap(), [entry(APP)]);
    assert_eq!(
        *store.state.entries.lock().unwrap(),
        [entry("org.example.Other")]
    );
}

#[test]
fn resetting_twice_says_there_was_nothing_the_second_time() {
    let bus = Bus::start();
    let _store = Store::serve(&bus, &[("screenshot", "screenshot", APP)], false);
    let home = tempfile::tempdir().unwrap();
    let first = kropa(&bus, home.path(), &["doctor", RESET]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert!(first.stdout.contains("forgotten"), "{}", first.stdout);
    let second = kropa(&bus, home.path(), &["doctor", RESET]);
    assert_eq!(second.code, 0, "{}", second.stderr);
    assert!(
        second.stdout.contains("Nothing to reset"),
        "{}",
        second.stdout
    );
    let json = kropa(&bus, home.path(), &["--json", "doctor", RESET]);
    assert_eq!(json.json()["result"], "absent");
}

#[test]
fn resetting_in_french_is_translated() {
    let bus = Bus::start();
    let _store = Store::serve(&bus, &[("screenshot", "screenshot", APP)], false);
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--lang", "fr", "doctor", RESET]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stdout.contains("Permission de capture oubliée"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_missing_permission_store_is_unsupported_with_its_reason() {
    let bus = Bus::start();
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor", RESET]);
    assert_eq!(run.code, 5, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["schema"], "kropa.doctor.reset/1");
    assert_eq!(doc["status"], "error");
    assert_eq!(doc["error"]["code"], "unsupported");
    assert_eq!(doc["error"]["reason"], "permission_store_unreachable");
    assert_eq!(doc["error"]["step"], "reset");
    assert!(run.stderr.contains("permission store"), "{}", run.stderr);
}

#[test]
fn no_session_bus_is_unsupported_with_the_same_reason() {
    let home = tempfile::tempdir().unwrap();
    let run = kropa_at(
        "unix:path=/nonexistent-kropa-dir/bus",
        home.path(),
        &["--json", "doctor", RESET],
    );
    assert_eq!(run.code, 5, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["error"]["reason"], "permission_store_unreachable");
}

#[test]
fn a_store_that_fails_is_an_internal_error_with_the_detail() {
    let bus = Bus::start();
    let _store = Store::serve(&bus, &[("screenshot", "screenshot", APP)], true);
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor", RESET]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    let doc = run.json();
    assert_eq!(doc["error"]["code"], "internal");
    assert_eq!(doc["error"]["reason"], "permission_store_error");
    assert!(
        doc["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the store is broken"),
        "{doc}"
    );
}

#[test]
fn the_refusal_message_points_to_the_reset_command_and_not_to_flatpak() {
    for (answer, reason) in [
        (Answer::Response(2), "portal_declined"),
        (Answer::NotAllowed, "portal_not_allowed"),
    ] {
        for lang in ["en", "fr"] {
            let bus = Bus::start();
            let _portal = Portal::serve(&bus, answer.clone());
            let home = tempfile::tempdir().unwrap();
            let target = home.path().join("shot.png");
            let run = kropa(
                &bus,
                home.path(),
                &[
                    "--lang",
                    lang,
                    "capture",
                    "screen",
                    "--output",
                    target.to_str().unwrap(),
                ],
            );
            assert_eq!(run.code, 4, "{reason}: {}", run.stderr);
            assert!(
                run.stderr
                    .contains("kropa doctor --reset-screenshot-permission"),
                "{reason}/{lang}: {}",
                run.stderr
            );
            assert!(
                !run.stderr.contains("flatpak"),
                "{reason}/{lang}: {}",
                run.stderr
            );
        }
    }
}

#[test]
fn doctor_without_the_option_is_still_the_report() {
    let bus = Bus::start();
    let store = Store::serve(&bus, &[("screenshot", "screenshot", APP)], false);
    let home = tempfile::tempdir().unwrap();
    let run = kropa(&bus, home.path(), &["--json", "doctor"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.json()["schema"], "kropa.doctor/1");
    assert!(
        store.state.calls.lock().unwrap().is_empty(),
        "the report must not reset anything"
    );
}
