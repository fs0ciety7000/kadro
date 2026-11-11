// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wayland clipboard on a real compositor: `sway --headless` (wlroots, `zwlr_data_control_v1`),
//! the real `kropa` binary as the helper that serves the selection, and `wl-paste` (an independent
//! program) to read what is really in the clipboard. This is the proof of lot 1.10 that a status of
//! `copied` is the effect, not a claim (`docs/cli.md` section 5).
//!
//! `weston --backend=headless` plays the compositor without data-control (what GNOME is for the
//! clipboard): the copy must then be `unavailable`, never `copied`.
//!
//! The compositors run in a private runtime folder: nothing touches the session of whoever runs the
//! test. Without `sway`, `weston` and `wl-paste` the tests say so and pass, unless
//! `KROPA_REQUIRE_WAYLAND_TESTS` is set (CI sets it).
//!
//! What this cannot show: the real GNOME (Mutter), KDE, and `ext-data-control-v1` (the sway of the
//! test machine may only know the wlr protocol). See `docs/testing/manual-linux.md`.

#![cfg(target_os = "linux")]
#![allow(clippy::too_many_lines)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use kropa_capture::clipboard::{CLIPBOARD_SERVE_ARGUMENT, Environment, WaylandClipboard};
use kropa_capture::{Clipboard, ClipboardOutcome, TextKind};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KROPA: &str = env!("CARGO_BIN_EXE_kropa");

fn required() -> bool {
    std::env::var("KROPA_REQUIRE_WAYLAND_TESTS").is_ok_and(|v| !v.is_empty())
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Which compositor to run.
#[derive(Clone, Copy)]
enum Kind {
    /// wlroots: `zwlr_data_control_v1`.
    Sway,
    /// No data-control protocol.
    Weston,
}

/// A headless compositor in a private runtime folder, stopped when dropped.
struct Compositor {
    child: Child,
    runtime: tempfile::TempDir,
    display: String,
}

impl Compositor {
    /// `None` (after saying why) when the tools are missing and not required.
    fn start(kind: Kind) -> Option<Self> {
        let (program, tools) = match kind {
            Kind::Sway => ("sway", ["sway", "wl-paste"]),
            Kind::Weston => ("weston", ["weston", "wl-paste"]),
        };
        for tool in tools {
            if !on_path(tool) {
                assert!(
                    !required(),
                    "{tool} is required (KROPA_REQUIRE_WAYLAND_TESTS is set)"
                );
                eprintln!("SKIPPED: {tool} is not installed");
                return None;
            }
        }
        let runtime = tempfile::tempdir().unwrap();
        // The compositor refuses a runtime folder that others can open.
        std::fs::set_permissions(
            runtime.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let config = runtime.path().join("compositor.conf");
        std::fs::write(&config, "").unwrap();
        let mut command = Command::new(program);
        match kind {
            Kind::Sway => command.arg("--config").arg(&config),
            Kind::Weston => command.args([
                "--backend=headless",
                "--renderer=noop",
                "--socket=wayland-weston",
                "--no-config",
            ]),
        };
        let child = command
            .env_clear()
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env("WLR_BACKENDS", "headless")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut compositor = Self {
            child,
            runtime,
            display: String::new(),
        };
        compositor.display = compositor.wait_for_socket();
        Some(compositor)
    }

    /// The name of the Wayland socket, once the compositor listens on it.
    fn wait_for_socket(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let found = std::fs::read_dir(self.runtime.path())
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .find(|n| {
                    n.starts_with("wayland-")
                        && Path::new(n).extension().is_none_or(|e| e != "lock")
                });
            if let Some(name) = found {
                return name;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "sway exited before it listened"
            );
            assert!(Instant::now() < deadline, "sway never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn environment(&self) -> Environment {
        Environment {
            wayland_display: Some(OsString::from(&self.display)),
            runtime_dir: Some(self.runtime.path().as_os_str().to_owned()),
            x11_display: false,
        }
    }

    /// The clipboard under test: the real binary as helper, in this session.
    fn clipboard(&self) -> WaylandClipboard {
        WaylandClipboard::new(
            PathBuf::from(KROPA),
            vec![OsString::from(CLIPBOARD_SERVE_ARGUMENT)],
            self.environment(),
        )
    }

    /// What `wl-paste` reads: the independent witness.
    fn paste(&self, mime: &str) -> Vec<u8> {
        let out = Command::new("wl-paste")
            .args(["--no-newline", "--type", mime])
            .env_clear()
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env("WAYLAND_DISPLAY", &self.display)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "wl-paste {mime}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    fn offered_types(&self) -> Vec<String> {
        let out = Command::new("wl-paste")
            .arg("--list-types")
            .env_clear()
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env("WAYLAND_DISPLAY", &self.display)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The processes serving a selection in this compositor's session.
    fn helpers(&self) -> Vec<u32> {
        helpers_in(self.runtime.path())
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Pids of the `kropa __clipboard-serve` processes whose environment names `runtime`.
fn helpers_in(runtime: &Path) -> Vec<u32> {
    let wanted = format!("XDG_RUNTIME_DIR={}", runtime.display());
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().filter_map(Result::ok) {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let (Ok(cmdline), Ok(environ)) = (
            std::fs::read(format!("/proc/{pid}/cmdline")),
            std::fs::read(format!("/proc/{pid}/environ")),
        ) else {
            continue;
        };
        let serves = cmdline
            .split(|b| *b == 0)
            .any(|arg| arg == CLIPBOARD_SERVE_ARGUMENT.as_bytes());
        let here = environ
            .split(|b| *b == 0)
            .any(|var| var == wanted.as_bytes());
        // A zombie keeps its command line empty: it is not serving anything.
        if serves && here {
            found.push(pid);
        }
    }
    found
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn text_is_copied_for_real_and_read_back_by_another_program() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    let link = "![shot.png](https://files.example.org/a/shot.png) é €";
    let outcome = sway.clipboard().copy_text(link, TextKind::Plain);
    assert_eq!(outcome, ClipboardOutcome::Copied);
    assert_eq!(sway.paste("text/plain"), link.as_bytes());
    assert!(
        sway.offered_types()
            .iter()
            .any(|t| t.starts_with("text/plain"))
    );
    // The command that asked has returned long ago; the clipboard still holds the text because the
    // helper keeps serving it.
    assert_eq!(sway.helpers().len(), 1);
    assert_eq!(sway.paste("text/plain"), link.as_bytes());
}

#[test]
fn html_is_offered_as_html_and_as_plain_text() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    let snippet = "<a href=\"https://x.example\">x</a>";
    assert_eq!(
        sway.clipboard().copy_text(snippet, TextKind::Html),
        ClipboardOutcome::Copied
    );
    assert_eq!(sway.paste("text/html"), snippet.as_bytes());
    assert_eq!(sway.paste("text/plain"), snippet.as_bytes());
}

#[test]
fn a_large_image_arrives_byte_for_byte() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    // Not a real picture: the clipboard carries bytes. 6 MiB crosses the pipes many times over.
    let png: Vec<u8> = (0..6 * 1024 * 1024_u32)
        .map(|i| u8::try_from((i ^ (i >> 8) ^ (i >> 16)) & 0xff).unwrap())
        .collect();
    assert_eq!(sway.clipboard().copy_image(&png), ClipboardOutcome::Copied);
    assert_eq!(sway.offered_types(), ["image/png"]);
    assert!(
        sway.paste("image/png") == png,
        "the image read back differs"
    );
}

#[test]
fn a_new_copy_replaces_the_old_one_and_its_helper_ends() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    let clipboard = sway.clipboard();
    assert_eq!(
        clipboard.copy_text("first", TextKind::Plain),
        ClipboardOutcome::Copied
    );
    assert_eq!(
        clipboard.copy_text("second", TextKind::Plain),
        ClipboardOutcome::Copied
    );
    assert_eq!(sway.paste("text/plain"), b"second");
    // The first helper lost the selection and has ended; only one process keeps serving.
    wait_until("the first helper to end", || sway.helpers().len() == 1);
}

#[test]
fn no_helper_outlives_the_compositor() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    let runtime = sway.runtime.path().to_owned();
    assert_eq!(
        sway.clipboard().copy_text("x", TextKind::Plain),
        ClipboardOutcome::Copied
    );
    assert_eq!(helpers_in(&runtime).len(), 1);
    // The tempdir goes with `sway`, so the proof is taken on the pids before.
    let pids = sway.helpers();
    drop(sway);
    wait_until("the helper to notice that the compositor is gone", || {
        // Gone, or a zombie (whose command line is empty): either way it serves nothing.
        pids.iter().all(
            |pid| !matches!(std::fs::read(format!("/proc/{pid}/cmdline")), Ok(c) if !c.is_empty()),
        )
    });
}

#[test]
fn a_stale_wayland_socket_is_a_failure_not_a_copy() {
    // WAYLAND_DISPLAY names a socket nobody listens on (a compositor that has gone): the helper
    // cannot connect, and that is `failed`, never `copied`.
    let runtime = tempfile::tempdir().unwrap();
    let env = Environment {
        wayland_display: Some("wayland-gone".into()),
        runtime_dir: Some(runtime.path().as_os_str().to_owned()),
        x11_display: false,
    };
    let clipboard = WaylandClipboard::new(
        PathBuf::from(KROPA),
        vec![OsString::from(CLIPBOARD_SERVE_ARGUMENT)],
        env,
    );
    let outcome = clipboard.copy_text("x", TextKind::Plain);
    assert_eq!(outcome.status(), "failed", "{outcome:?}");
    assert_eq!(
        outcome.reason().map(kropa_capture::ClipboardReason::as_str),
        Some("connection_failed")
    );
}

#[test]
fn the_helper_refuses_a_request_that_is_not_ours() {
    // Garbage on stdin: an answer line that says failed, and a non-zero exit. (On a terminal it would
    // not even wait for input; a test has no terminal to show it.)
    let mut child = Command::new(KROPA)
        .arg(CLIPBOARD_SERVE_ARGUMENT)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"GET / HTTP/1.1\r\n\r\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "failed helper_failed malformed request: magic line\n"
    );
}

/// A loopback server that accepts uploads, a configuration folder that points at it, and a file.
struct Upload {
    _server: MockServer,
    home: tempfile::TempDir,
    config: PathBuf,
    file: PathBuf,
}

impl Upload {
    async fn new() -> Self {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/up"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"url": "https://files.example.org/a/shot.png"}),
                ),
            )
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("config");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(
            config.join("destinations.toml"),
            format!(
                "[[destination]]\nid = \"open\"\ntype = \"http\"\n[destination.request]\nurl = \"{}/up\"\n\
                 [destination.response]\nurl = \"{{json:$.url}}\"\n",
                server.uri()
            ),
        )
        .unwrap();
        std::fs::write(
            config.join("config.toml"),
            "default_destination = \"open\"\n",
        )
        .unwrap();
        let file = home.path().join("shot.png");
        std::fs::write(&file, b"\x89PNG fake screenshot").unwrap();
        Self {
            _server: server,
            home,
            config,
            file,
        }
    }

    /// Runs `kropa upload` (the real binary) in the session of `compositor`.
    fn upload(&self, compositor: &Compositor, flags: &[&str], extra: &[&str]) -> Output {
        let mut command = Command::new(KROPA);
        command
            .args(["--lang", "en"])
            .args(flags)
            .arg("--config")
            .arg(&self.config)
            .arg("upload")
            .arg(&self.file)
            .args(extra)
            .env_clear()
            .env("HOME", self.home.path())
            .env("XDG_RUNTIME_DIR", compositor.runtime.path())
            .env("WAYLAND_DISPLAY", &compositor.display)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent-kropa-bus",
            )
            .stdin(Stdio::null());
        // `output()` reads stdout and stderr until both are closed, like `$(...)` does: if the helper
        // held the pipes it would not return while the helper serves.
        let started = Instant::now();
        let out = command.output().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "the command waited for the helper"
        );
        out
    }
}

fn data_control_fact(compositor: &Compositor, home: &Path) -> serde_json::Value {
    let doctor = Command::new(KROPA)
        .args(["--json", "doctor"])
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", compositor.runtime.path())
        .env("WAYLAND_DISPLAY", &compositor.display)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent-kropa-bus",
        )
        .output()
        .unwrap();
    let d: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert!(
        d["not_available_yet"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["item"] != "clipboard")
    );
    d["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "data_control")
        .unwrap()
        .clone()
}

/// The whole command, as a user runs it: `kropa upload --copy` against a server on the loopback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_command_copies_the_link_and_a_shell_capturing_its_output_is_not_kept_waiting() {
    let Some(sway) = Compositor::start(Kind::Sway) else {
        return;
    };
    let upload = Upload::new().await;
    let link = b"![shot.png](https://files.example.org/a/shot.png)";

    let out = upload.upload(&sway, &["--json"], &["--copy", "markdown"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["upload"]["url"], "https://files.example.org/a/shot.png");
    assert_eq!(v["clipboard"]["status"], "copied");
    assert_eq!(v["clipboard"]["requested"], "markdown");
    assert_eq!(sway.paste("text/plain"), link);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Link copied (markdown)"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Not asked: the clipboard is not touched.
    let out = upload.upload(&sway, &["--json"], &[]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["clipboard"]["status"], "skipped");
    assert_eq!(sway.paste("text/plain"), link);

    // And `kropa doctor` sees the protocol.
    let fact = data_control_fact(&sway, upload.home.path());
    assert_eq!(fact["state"], "present");
    assert_eq!(fact["value"], true);
}

/// The GNOME case: a compositor without data-control. Exit code 0, the raw URL on stdout, the honest
/// warning on stderr, `unavailable` in the JSON, and no process left behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_data_control_the_command_warns_and_exits_zero() {
    let Some(weston) = Compositor::start(Kind::Weston) else {
        return;
    };
    let upload = Upload::new().await;

    let out = upload.upload(&weston, &["--json"], &["--copy", "url"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["upload"]["url"], "https://files.example.org/a/shot.png");
    assert_eq!(v["clipboard"]["status"], "unavailable");
    assert_eq!(v["clipboard"]["reason"], "no_data_control");
    assert_eq!(v["clipboard"]["explicit"], true);
    let codes: Vec<&str> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"clipboard_unavailable"), "{codes:?}");
    let wording = "Link not copied: This Wayland session does not allow clipboard access without \
                   focus (GNOME does not). The link is printed above.";
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(wording),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Without --json: stdout is the URL alone.
    let human = upload.upload(&weston, &[], &["--copy", "url"]);
    assert_eq!(human.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&human.stdout),
        "https://files.example.org/a/shot.png\n"
    );

    // The library says the same, for text and for an image.
    let clipboard = weston.clipboard();
    let expected = ClipboardOutcome::unavailable(kropa_capture::ClipboardReason::NoDataControl);
    assert_eq!(clipboard.copy_text("x", TextKind::Plain), expected);
    assert_eq!(clipboard.copy_image(b"png"), expected);

    // And the doctor does not claim the protocol.
    let fact = data_control_fact(&weston, upload.home.path());
    assert_eq!(fact["state"], "absent");
    wait_until("the helpers to end", || weston.helpers().is_empty());
}
