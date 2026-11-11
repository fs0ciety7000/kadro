// SPDX-License-Identifier: GPL-3.0-or-later

//! No memory dump of a process that may hold a secret (`docs/security/secrets.md` section 6, Q8).
//!
//! A core file written after a crash, or after Ctrl-\ while a token is being typed, would hand that
//! token to `systemd-coredump` or `apport`. So at start-up, before any secret is read:
//!
//! `setrlimit(RLIMIT_CORE, 0)` on Linux and macOS, soft and hard value, so the process cannot raise it
//! back; children inherit it. It is a plain system call through `rustix`: no `unsafe` here.
//!
//! There is deliberately no `prctl(PR_SET_DUMPABLE, 0)` in the CLI: `xdg-desktop-portal` reads
//! `/proc/<pid>/root` of its caller to identify it, and a non-dumpable process makes it unreadable
//! (EACCES), which would break the capture through the portal. Protection against `ptrace` rests on
//! the Yama `ptrace_scope` of the system. On macOS `PT_DENY_ATTACH` needs a raw `ptrace` call
//! (`unsafe`, outside the modules where `AGENTS.md` allows it), and the hardened runtime of the
//! signed `.app` already refuses debuggers (`docs/security/secrets.md` section 6).

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
/// [`HardeningError`] when a system call is refused (a sandbox that filters `setrlimit`).
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
        // The process stays dumpable (see the module documentation): the portal must read it.
        #[cfg(target_os = "linux")]
        assert_eq!(
            rustix::process::dumpable_behavior().unwrap(),
            rustix::process::DumpableBehavior::Dumpable
        );
        // Raising the soft limit above the hard one is refused.
        let raised = rustix::process::setrlimit(
            Resource::Core,
            rustix::process::Rlimit {
                current: Some(1 << 20),
                maximum: Some(1 << 20),
            },
        );
        if has_cap_sys_resource() {
            // Raising a hard limit is allowed to root and to CAP_SYS_RESOURCE: nothing to prove here.
            eprintln!("skipped: the process may raise a hard limit (root or CAP_SYS_RESOURCE)");
            return;
        }
        assert!(raised.is_err());
        assert_eq!(getrlimit(Resource::Core).maximum, Some(0));
    }

    /// Whether the process may raise a hard limit: root, or `CAP_SYS_RESOURCE` (bit 24 of `CapEff`).
    fn has_cap_sys_resource() -> bool {
        if rustix::process::geteuid().is_root() {
            return true;
        }
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                let line = status.lines().find_map(|l| l.strip_prefix("CapEff:"))?;
                u64::from_str_radix(line.trim(), 16).ok()
            })
            .is_some_and(|caps| caps & (1 << 24) != 0)
    }

    #[test]
    fn it_is_idempotent() {
        disable_core_dumps().unwrap();
        disable_core_dumps().unwrap();
    }
}
