// SPDX-License-Identifier: GPL-3.0-or-later

//! The release wiring of `kropa_cli::hardening` (`docs/security/secrets.md` section 6): `main` lowers
//! `RLIMIT_CORE` to 0 at start-up in a release build only, so the unit tests of the module cannot
//! see a call that was never made.
//!
//! The test runs the binary that `KROPA_RELEASE_BIN` names (CI builds it with
//! `cargo build --release -p kropa --locked`) on `version` and reads `/proc/<pid>/limits` of the
//! child before it is reaped: a process that has ended and not been waited for still has its
//! limits. No command of `kropa` waits on its own, and none is added for a test. Before it starts,
//! the test gives itself (so the child) a core limit of 1 MiB: only the binary itself can bring it
//! to 0. Without the variable the test does nothing (a debug binary does not harden, by design).
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

fn core_line(pid: u32) -> Option<String> {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).ok()?;
    limits
        .lines()
        .find(|l| l.starts_with("Max core file size"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[test]
fn the_release_binary_forbids_core_dumps_at_start_up() {
    let Some(bin) = std::env::var_os("KROPA_RELEASE_BIN").map(PathBuf::from) else {
        eprintln!("SKIPPED: KROPA_RELEASE_BIN is not set (it names a release build of `kropa`)");
        return;
    };
    let own = getrlimit(Resource::Core);
    let mib = 1_u64 << 20;
    if own.maximum.is_some_and(|max| max < mib) {
        eprintln!(
            "SKIPPED: the hard core limit ({:?}) is below 1 MiB already",
            own.maximum
        );
        return;
    }
    setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(mib),
            maximum: Some(mib),
        },
    )
    .unwrap();

    let mut child = Command::new(&bin)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Until `main` has run, the child still shows the 1 MiB it inherited: wait for the 0, with a
    // deadline. The child is not waited for before the reads, so its entry stays in `/proc`.
    let started = Instant::now();
    let mut seen = String::new();
    while started.elapsed() < Duration::from_secs(10) {
        seen = core_line(child.id()).unwrap_or_default();
        if seen == "Max core file size 0 0 bytes" {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = child.wait().unwrap();
    assert_eq!(
        seen, "Max core file size 0 0 bytes",
        "the release binary did not lower RLIMIT_CORE (soft and hard) to 0"
    );
    assert!(status.success(), "{status}");
}
