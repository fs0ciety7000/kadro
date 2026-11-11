// SPDX-License-Identifier: GPL-3.0-or-later

//! The broker as a real process: `BrokerClient` starts a program with `spawn_with_channel` and talks
//! to it over the inherited socket pair, as `kropa-app` does with `kropa __store-broker`.
//!
//! The program is this very test binary, run on one of the `child_*` tests below (they are
//! `#[ignore]`d, so nothing runs them but the client). What the real broker does with a Mac
//! (`LOCAL_PEERTOKEN`, `SecCode`) cannot be tested here and is not: the child admits the code of its
//! caller by decree, and checks the rest for real (the parent, fd 3, the environment, the standard
//! descriptors). The real broker's refusals on a Mac are in `crates/kropa-cli/tests/macos.rs`.
//!
//! A child that finds something wrong with how it was started writes it in `problems` of its folder
//! and exits 99: its stderr is `/dev/null`, by design.

#![cfg(unix)]
#![allow(clippy::assert_is_empty)]

use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use kropa_keychain::client::{BrokerClient, Timeouts};
use kropa_keychain::guard::{ChannelState, LaunchFacts, PeerToken, ProjectCertificate, Unverified};
use kropa_keychain::item::{Failure, FailureKind, Item, ItemName, ItemStore};
use kropa_keychain::server;
use kropa_keychain::system::{SystemFacts, take_channel};
use zeroize::Zeroizing;

/// An item store in a folder, so that two launches of the broker see the same items.
struct DirStore(PathBuf);

impl ItemStore for DirStore {
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure> {
        let mut out = Vec::new();
        for n in self.list()? {
            if n.profile == profile && name.is_none_or(|w| w == n.name) {
                let bytes = fs::read(self.0.join(format!("{}__{}", n.profile, n.name))).unwrap();
                out.push(Item {
                    name: n.name,
                    document: Zeroizing::new(bytes),
                });
            }
        }
        Ok(out)
    }

    fn list(&self) -> Result<Vec<ItemName>, Failure> {
        let mut out = Vec::new();
        for e in fs::read_dir(&self.0).unwrap() {
            let file = e.unwrap().file_name().into_string().unwrap();
            if let Some((p, n)) = file.split_once("__") {
                out.push(ItemName {
                    profile: p.into(),
                    name: n.into(),
                });
            }
        }
        Ok(out)
    }

    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        let path = self.0.join(format!("{profile}__{name}"));
        if path.exists() {
            return Err(Failure::new(FailureKind::Exists, "exists"));
        }
        fs::write(path, document).unwrap();
        Ok(())
    }

    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        let path = self.0.join(format!("{profile}__{name}"));
        if !path.exists() {
            return Err(Failure::new(FailureKind::Missing, name));
        }
        fs::write(path, document).unwrap();
        Ok(())
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure> {
        Ok(fs::remove_file(self.0.join(format!("{profile}__{name}"))).is_ok())
    }
}

/// Real parent and channel facts, decreed code facts.
struct Facts {
    code_ok: bool,
}

impl LaunchFacts for Facts {
    fn parent_pid(&self) -> u32 {
        SystemFacts.parent_pid()
    }

    fn channel(&self) -> ChannelState {
        SystemFacts.channel()
    }

    fn peer_token(&self) -> Result<PeerToken, Unverified> {
        Ok(PeerToken {
            pid: self.parent_pid(),
            raw: [0; 8],
        })
    }

    fn project_certificate(&self) -> Result<ProjectCertificate, Unverified> {
        Ok(ProjectCertificate::from_hex(&"ab".repeat(20)).unwrap())
    }

    fn check_peer_code(&self, _: &PeerToken, _: &ProjectCertificate) -> Result<(), Unverified> {
        if self.code_ok {
            Ok(())
        } else {
            Err(Unverified("decreed refusal"))
        }
    }
}

fn folder_of_the_parent() -> PathBuf {
    PathBuf::from(format!(
        "/tmp/kropa-broker-test-{}",
        std::os::unix::process::parent_id()
    ))
}

/// How this process was started, as `spawn_with_channel` promises.
fn problems_with_the_start() -> Vec<String> {
    let mut problems = Vec::new();
    if std::env::vars_os().count() != 0 {
        problems.push("the environment is not empty".to_owned());
    }
    if std::env::current_dir().ok() != Some(PathBuf::from("/")) {
        problems.push("the working directory is not /".to_owned());
    }
    let null = fs::metadata("/dev/null").unwrap().rdev();
    let stdin = std::io::stdin().as_fd().try_clone_to_owned().unwrap();
    let stdin_rdev = fs::File::from(stdin).metadata().unwrap().rdev();
    if stdin_rdev != null {
        problems.push("fd 0 is not /dev/null".to_owned());
    }
    if SystemFacts.channel() != ChannelState::Connected {
        problems.push("fd 3 is not a connected socket".to_owned());
    }
    problems
}

fn run_as_broker(code_ok: bool) -> ! {
    let dir = folder_of_the_parent();
    let problems = problems_with_the_start();
    if !problems.is_empty() {
        fs::write(dir.join("problems"), problems.join("\n")).unwrap();
        std::process::exit(99);
    }
    let code = server::run(&Facts { code_ok }, &DirStore(dir), take_channel);
    std::process::exit(i32::from(code));
}

#[test]
#[ignore = "run as the broker by the tests below"]
fn child_broker() {
    run_as_broker(true);
}

#[test]
#[ignore = "run as the broker by the tests below"]
fn child_refusing() {
    run_as_broker(false);
}

#[test]
#[ignore = "run as the broker by the tests below"]
fn child_silent() {
    std::thread::sleep(Duration::from_secs(60));
}

fn client_of(child: &str, call: Duration) -> BrokerClient {
    let me = std::env::current_exe().unwrap();
    BrokerClient::new(me).for_tests(
        &[
            child,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        Timeouts { call, prompt: call },
    )
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let dir = PathBuf::from(format!("/tmp/kropa-broker-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_broker_started_by_the_client_is_started_as_the_specification_says_and_serves_a_round_trip() {
    let folder = Folder::new();
    let client = client_of("child_broker", Duration::from_secs(30));
    client.create("zipline", "token", b"{\"v\":2}").unwrap();
    let items = client.read("zipline", None).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(&**items[0].document, b"{\"v\":2}");
    assert_eq!(client.list().unwrap().len(), 1);
    assert_eq!(
        client.create("zipline", "token", b"{}").unwrap_err().kind,
        FailureKind::Exists
    );
    client
        .replace("zipline", "token", b"{\"v\":2,\"x\":1}")
        .unwrap();
    assert!(client.remove("zipline", "token").unwrap());
    assert!(client.list().unwrap().is_empty());
    let problems = fs::read_to_string(folder.0.join("problems")).unwrap_or_default();
    assert_eq!(problems, "", "the broker was started wrongly");
}

#[test]
fn a_broker_that_refuses_its_caller_gives_keychain_broker_refused_and_serves_nothing() {
    let folder = Folder::new();
    fs::write(folder.0.join("p__n"), b"{}").unwrap();
    let client = client_of("child_refusing", Duration::from_secs(30));
    let error = client.read("p", None).unwrap_err();
    assert_eq!(error.kind, FailureKind::BrokerRefused);
    assert!(
        error.detail.contains("peer_code_rejected"),
        "{}",
        error.detail
    );
    // Nothing was written, nothing leaked.
    assert!(!error.detail.contains("{}"));
}

#[test]
fn a_program_that_ends_without_an_answer_is_unavailable_with_its_status() {
    let client = client_of("no_such_test_at_all", Duration::from_secs(30));
    let error = client.list().unwrap_err();
    assert_eq!(error.kind, FailureKind::BrokerUnavailable);
    assert!(
        error.detail.contains("without an answer"),
        "{}",
        error.detail
    );
}

#[test]
fn a_broker_that_never_answers_is_stopped_at_the_deadline() {
    let client = client_of("child_silent", Duration::from_millis(500));
    let started = std::time::Instant::now();
    let error = client.list().unwrap_err();
    assert_eq!(error.kind, FailureKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(20));
}
