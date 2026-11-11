// SPDX-License-Identifier: GPL-3.0-or-later

//! No memory dump of a process that may hold a secret (`docs/security/secrets.md` section 6, Q8).
//!
//! A core file written after a crash, or after Ctrl-\ while a token is being typed, would hand that
//! token to `systemd-coredump` or `apport`. So at start-up, before any secret is read:
//!
//! - Linux: `setrlimit(RLIMIT_CORE, 0)` and `prctl(PR_SET_DUMPABLE, 0)`. The second one also refuses
//!   `ptrace` and `/proc/<pid>/mem` to the same user without root.
//! - macOS: `setrlimit(RLIMIT_CORE, 0)` only. There is no `PR_SET_DUMPABLE`; `PT_DENY_ATTACH` needs
//!   a raw `ptrace` call (`unsafe`, outside the modules where `AGENTS.md` allows it), and the hardened
//!   runtime of the signed `.app` already refuses debuggers (section 6).
//!
//! Both calls are plain system calls through `rustix`: no `unsafe` here. The limit is lowered for the
//! soft and the hard value, so the process cannot raise it back.

use std::io;

#[cfg(unix)]
use rustix::process::{Resource, Rlimit, setrlimit};

/// What [`disable_core_dumps`] could not do.
#[derive(Debug)]
pub struct HardeningError {
    /// What was being set.
    pub step: &'static str,
    /// Why it failed.
    pub source: io::Error,
}

impl std::fmt::Display for HardeningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot set {}: {}", self.step, self.source)
    }
}

/// Forbids core dumps for this process. Every step is tried; the first failure is returned.
///
/// # Errors
/// [`HardeningError`] when a system call is refused (a sandbox that filters `prctl` or
/// `setrlimit`).
pub fn disable_core_dumps() -> Result<(), HardeningError> {
    let mut first: Option<HardeningError> = None;
    let mut record = |step: &'static str, result: io::Result<()>| {
        if let Err(source) = result {
            first.get_or_insert(HardeningError { step, source });
        }
    };
    #[cfg(unix)]
    record(
        "RLIMIT_CORE",
        setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )
        .map_err(io::Error::from),
    );
    #[cfg(target_os = "linux")]
    record(
        "PR_SET_DUMPABLE",
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
            .map_err(io::Error::from),
    );
    #[cfg(not(unix))]
    record("core dumps", Ok(()));
    first.map_or(Ok(()), Err)
}

#[cfg(all(test, unix))]
mod tests {
    use rustix::process::{Resource, getrlimit};

    use super::disable_core_dumps;

    // Each test of `nextest` is a process of its own: lowering the limits here touches nothing else.
    #[test]
    fn core_dumps_are_off_and_cannot_be_raised_back() {
        disable_core_dumps().unwrap();
        let limit = getrlimit(Resource::Core);
        assert_eq!(limit.current, Some(0));
        assert_eq!(limit.maximum, Some(0));
        #[cfg(target_os = "linux")]
        assert_eq!(
            rustix::process::dumpable_behavior().unwrap(),
            rustix::process::DumpableBehavior::NotDumpable
        );
        // Raising the soft limit above the hard one is refused.
        let raised = rustix::process::setrlimit(
            Resource::Core,
            rustix::process::Rlimit {
                current: Some(1 << 20),
                maximum: Some(1 << 20),
            },
        );
        if rustix::process::getuid().is_root() {
            // Root may raise it: nothing to prove here.
            return;
        }
        assert!(raised.is_err());
        assert_eq!(getrlimit(Resource::Core).maximum, Some(0));
    }

    #[test]
    fn it_is_idempotent() {
        disable_core_dumps().unwrap();
        disable_core_dumps().unwrap();
    }
}
