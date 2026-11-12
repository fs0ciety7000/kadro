// SPDX-License-Identifier: GPL-3.0-or-later

//! Activation by a private session bus (`dbus-daemon` with a temporary service directory): the real
//! `DbusActivation` starts the fake application of `kropa-ipc` (decision D1, lot 2.1a).
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use kropa_ipc::dir::socket_path;
use kropa_ipc::{DbusActivation, ensure_running_in};
use serde_json::Value;

const FAKE: &str = env!("CARGO_BIN_EXE_kropa-ipc-fake");
const NAME: &str = "io.github.fs0ciety7000.KropaTest";

/// The private bus is killed with the test.
struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts a private session bus on `socket`, with `services` as its only service directory.
fn private_bus(root: &Path, services: Option<&Path>) -> (Bus, String) {
    let address = format!("unix:path={}", root.join("bus").display());
    let servicedir = services
        .map(|s| format!("<servicedir>{}</servicedir>", s.display()))
        .unwrap_or_default();
    let config = root.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{address}</listen>
  {servicedir}
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#
        ),
    )
    .unwrap();
    let mut daemon = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon is needed for this test");
    let mut first = String::new();
    BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let bus = Bus(daemon);
    assert!(first.starts_with("unix:"), "the bus printed {first:?}");
    (bus, address)
}

#[tokio::test]
async fn dbus_activation_starts_the_application_and_the_client_reaches_it() {
    let tmp = tempfile::tempdir().unwrap();
    let services = tmp.path().join("services");
    std::fs::create_dir(&services).unwrap();
    let runtime = tmp.path().join("kropa");
    let address = format!("unix:path={}", tmp.path().join("bus").display());
    std::fs::write(
        services.join(format!("{NAME}.service")),
        format!(
            "[D-BUS Service]\nName={NAME}\nExec={FAKE} --dir {} --bus-name {NAME} --bus-address {address} --idle 20\n",
            runtime.display()
        ),
    )
    .unwrap();
    let (_bus, address) = private_bus(tmp.path(), Some(&services));

    let launcher = DbusActivation::with_target(NAME, &address);
    let mut client = ensure_running_in(&runtime, &launcher, Duration::from_secs(5))
        .await
        .expect("activation must bring the application up");
    assert_eq!(client.server().server, "kropa-gtk");
    let status: Value = client.call("app.status", ()).await.unwrap();
    assert_eq!(status["owns_clipboard"], false);

    // A second activation finds it running: `AlreadyRunning` is not an error.
    let again = ensure_running_in(&runtime, &launcher, Duration::from_secs(5))
        .await
        .unwrap();
    drop(again);
    let _: Value = client.call("app.quit", ()).await.unwrap();
    for _ in 0..100 {
        if !socket_path(&runtime).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the activated application did not quit");
}

#[tokio::test]
async fn activation_of_an_unknown_service_is_daemon_start_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let (_bus, address) = private_bus(tmp.path(), None);
    let launcher = DbusActivation::with_target(NAME, &address);
    let err = ensure_running_in(&tmp.path().join("kropa"), &launcher, Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(err.reason(), Some("daemon_start_failed"), "{err}");
    assert_eq!(err.exit_code(), 14);
}
