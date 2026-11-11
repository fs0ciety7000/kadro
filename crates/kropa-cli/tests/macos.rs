// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS capture, as far as a machine without the Screen Recording permission can show it.
//!
//! The CI runner has no TCC permission and may have no login session, so nothing here takes a
//! screenshot or asks for a permission (a dialog would also pop up on the Mac of whoever runs
//! `cargo test`): these tests run the real `kropa` binary on what is answered without either. The
//! real capture is the manual check-list `docs/testing/manual-macos.md`.

#![cfg(target_os = "macos")]

use std::process::{Command, Output};

use serde_json::Value;

fn kropa(args: &[&str]) -> Output {
    let dir = tempfile::tempdir().expect("temporary folder");
    Command::new(env!("CARGO_BIN_EXE_kropa"))
        .args(args)
        .current_dir(dir.path())
        .env_clear()
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join(".config"))
        .output()
        .expect("run kropa")
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not one JSON document ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn doctor_reports_the_macos_backend_whatever_the_permissions() {
    let output = kropa(&["--lang", "en", "--json", "doctor"]);
    assert_eq!(output.status.code(), Some(0), "doctor is a report");
    let doc = json_of(&output);
    assert_eq!(doc["schema"], "kropa.doctor/1");
    assert_eq!(doc["system"]["platform"], "macos");
    assert_eq!(doc["backend"], "screencapturekit");
    assert_eq!(doc["capabilities"].as_array().map(Vec::len), Some(2));
    let facts = doc["facts"].as_array().expect("facts");
    let has = |name: &str| facts.iter().any(|f| f["key"] == name);
    for name in [
        "macos_version",
        "graphical_session",
        "screen_recording",
        "app_bundle",
        "bundle_identifier",
    ] {
        assert!(has(name), "fact {name} is missing: {facts:?}");
    }
}

#[test]
fn doctor_is_not_a_bundle_when_run_from_the_build_folder() {
    let doc = json_of(&kropa(&["--json", "doctor"]));
    let bundle = doc["facts"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["key"] == "app_bundle"))
        .expect("app_bundle fact");
    assert_eq!(bundle["state"], "absent");
}

#[test]
fn doctor_has_no_untranslated_text_in_either_language() {
    for lang in ["en", "fr"] {
        let output = kropa(&["--lang", lang, "doctor"]);
        assert_eq!(output.status.code(), Some(0));
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!text.contains("doctor-"), "{lang}: {text}");
    }
}

#[test]
fn an_unknown_screen_is_a_capture_failure_not_a_permission_question() {
    // `check` runs before the permission step, so no dialog can appear.
    let output = kropa(&[
        "--json",
        "capture",
        "output",
        "no such screen 7d3f",
        "--output",
        "shot.png",
    ]);
    assert_eq!(output.status.code(), Some(10), "{output:?}");
    let doc = json_of(&output);
    let reason = doc["error"]["reason"].as_str().expect("reason");
    // Without a login session (ssh, some runners) the session check comes first.
    assert!(
        ["output_not_found", "no_graphical_session"].contains(&reason),
        "{reason}"
    );
    assert_eq!(doc["error"]["code"], "capture_failed");
    assert_eq!(doc["error"]["step"], "capture");
}

#[test]
fn nothing_is_written_when_the_capture_does_not_happen() {
    let home = tempfile::tempdir().expect("temporary folder");
    let work = home.path().join("work");
    std::fs::create_dir(&work).expect("working folder");
    let output = Command::new(env!("CARGO_BIN_EXE_kropa"))
        .args(["capture", "output", "no such screen 7d3f", "--output", "."])
        .current_dir(&work)
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .output()
        .expect("run kropa");
    assert_eq!(output.status.code(), Some(10));
    let files = std::fs::read_dir(&work).expect("read dir").count();
    assert_eq!(files, 0, "no file after a failure");
}

#[test]
fn secrets_status_reads_the_keychain_and_never_says_not_delivered() {
    // The Keychain of the runner or of the Mac of whoever runs the tests: whatever its state, the
    // report is a report (exit 0) and says it is the Keychain. It never prompts: no terminal here.
    let output = kropa(&["--lang", "en", "--json", "secrets", "status"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let doc = json_of(&output);
    assert_eq!(doc["schema"], "kropa.secrets.status/1");
    assert_eq!(doc["store"], "keychain");
    let state = doc["state"].as_str().expect("state");
    assert!(
        ["available", "locked", "unavailable", "denied", "ambiguous"].contains(&state),
        "{state}"
    );
}

#[test]
fn the_broker_mode_refuses_from_a_shell_before_any_keychain_access() {
    // Started like this, from a test process, the broker has no socket on fd 3: the launch checks
    // refuse it (status 71, `no_channel`), with the reason on stderr and nothing on stdout.
    let output = Command::new(env!("CARGO_BIN_EXE_kropa"))
        .arg("__store-broker")
        .env_clear()
        .output()
        .expect("run kropa");
    assert_eq!(output.status.code(), Some(71), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("keychain broker refused: no_channel"),
        "{stderr}"
    );
}

#[test]
fn the_broker_is_not_a_command_and_takes_no_arguments() {
    // `__store-broker` with anything else is an ordinary unknown command.
    let output = kropa(&["__store-broker", "extra"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn a_caller_that_is_not_kropa_app_gets_nothing_from_the_real_broker() {
    // This test process starts the real `kropa __store-broker` on a real socket pair, as `kropa-app`
    // would. The binary is signed ad hoc (the linker does it), not with a project certificate, so the
    // broker serves nobody: `keychain_broker_unavailable`. Were it signed with the certificate, this
    // test process would still not be `kropa-app`: `keychain_broker_refused`. Either way, no entry.
    use kropa_keychain::client::{BrokerClient, Timeouts};
    use kropa_keychain::item::{FailureKind, ItemStore};

    let client = BrokerClient::new(env!("CARGO_BIN_EXE_kropa"))
        .for_tests(&["__store-broker"], Timeouts::default());
    let error = client.read("zipline", None).unwrap_err();
    assert!(
        matches!(
            error.kind,
            FailureKind::BrokerUnavailable | FailureKind::BrokerRefused
        ),
        "{error:?}"
    );
    assert_eq!(client.list().unwrap_err().kind, error.kind);
}

#[test]
fn the_client_does_not_even_start_a_broker_it_cannot_verify() {
    // Without `for_tests` the client checks the signature of the broker's binary first, and this
    // test binary is not signed with the project certificate to say what it must.
    use kropa_keychain::client::BrokerClient;
    use kropa_keychain::item::{FailureKind, ItemStore};

    let client = BrokerClient::new(env!("CARGO_BIN_EXE_kropa"));
    let error = client.list().unwrap_err();
    assert_eq!(error.kind, FailureKind::BrokerUnavailable);
    assert!(error.detail.contains("disabled"), "{}", error.detail);
}
