// SPDX-License-Identifier: GPL-3.0-or-later

//! Inherited descriptors do not reach children (`docs/lots/lot-2.md` decision D2).
//!
//! A process that starts `kropa` can leave descriptors open for it (`--password-fd`, a pipe, a file).
//! Without care they would pass on to every process that `kropa` starts in turn (the editor, the
//! `__clipboard-serve` helper, the application), which would then hold a passphrase pipe open. At the
//! very start of `main`, every descriptor from 3 up is marked close-on-exec. The descriptors stay
//! usable by `kropa` itself: only `exec` closes them.
//!
//! `rustix` 1.1.6 has no `close_range` and cannot build a descriptor from a number without `unsafe`,
//! so this module is the one place of the IPC crate where `unsafe` is allowed (`AGENTS.md`, rule 12).
//! It lives here, and not in the command line, because two programs call it first thing in `main`:
//! `kropa` and `kropa-gtk` (lot 2.1d, review 2.1a m4).
//!
//! Three ways, from the best to the last resort. Linux uses `close_range(3, ~0, CLOSE_RANGE_CLOEXEC)`
//! (kernel 5.11). A kernel or a sandbox that refuses it (and macOS) walks `/proc/self/fd` (`/dev/fd`
//! on macOS). Without that directory either, every number from 3 to the soft `RLIMIT_NOFILE` is tried
//! with `fcntl`, capped at [`SCAN_CAP`]: a descriptor above the cap, in a process whose limit was
//! raised that high and which has no `/proc`, is not marked (a documented limit, not a failure).

#![allow(unsafe_code)]

use std::io;

/// First descriptor that is not standard input, output or error.
const FIRST: libc::c_int = 3;

/// Last resort without `/proc`: the highest descriptor number tried (the soft limit, capped here so
/// that an unlimited `RLIMIT_NOFILE` does not mean billions of system calls).
pub const SCAN_CAP: libc::c_int = 65_536;

/// Marks every open descriptor from 3 up close-on-exec.
///
/// # Errors
/// The system error when neither `close_range` nor the walk of the descriptor directory worked.
pub fn mark_inherited_cloexec() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if close_range_cloexec().is_ok() {
        return Ok(());
    }
    walk_and_mark()
}

/// One system call. `Err` for `ENOSYS` (kernel before 5.11), `EPERM` (seccomp) and the like.
#[cfg(target_os = "linux")]
fn close_range_cloexec() -> io::Result<()> {
    // SAFETY: `close_range` takes three integers and no pointer. With `CLOSE_RANGE_CLOEXEC` it only
    // sets the close-on-exec flag of descriptors in [3, u32::MAX]; it closes nothing and cannot touch
    // memory. The constant is the one of the kernel header.
    let result = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            FIRST as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Lists the descriptor directory, then sets the flag one descriptor at a time. Without the
/// directory, [`scan_and_mark`].
fn walk_and_mark() -> io::Result<()> {
    match list_descriptors() {
        Ok(numbers) => {
            for fd in numbers {
                mark(fd)?;
            }
            Ok(())
        }
        Err(_) => scan_and_mark(),
    }
}

/// Open descriptors from 3 up, from `/proc/self/fd` (`/dev/fd` on macOS). Collected first: the
/// directory handle of the walk is itself in the list, and the flag is set after the listing is
/// complete.
fn list_descriptors() -> io::Result<Vec<libc::c_int>> {
    let directory = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    let mut numbers = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name();
        if let Some(number) = name.to_str().and_then(|n| n.parse::<libc::c_int>().ok())
            && number >= FIRST
        {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

/// The last resort: every number from 3 to the soft limit (at most [`SCAN_CAP`]) is tried; the ones
/// that are not open give `EBADF` and are ignored.
fn scan_and_mark() -> io::Result<()> {
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile).current;
    let last = limit
        .and_then(|l| libc::c_int::try_from(l).ok())
        .map_or(SCAN_CAP, |l| l.min(SCAN_CAP));
    scan_up_to(last)
}

/// [`scan_and_mark`] up to `last` (excluded).
fn scan_up_to(last: libc::c_int) -> io::Result<()> {
    for fd in FIRST..last {
        mark(fd)?;
    }
    Ok(())
}

/// Sets close-on-exec on one descriptor; a number that is not open is not an error.
fn mark(fd: libc::c_int) -> io::Result<()> {
    // SAFETY: `fcntl` with `F_GETFD` and `F_SETFD` takes integers only. A number that is not open
    // (the handle of the walk, closed when `read_dir` ended, or a gap in a scan) gives `EBADF`, which
    // is ignored; a descriptor that is open is only given one more flag, and never closed.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0
            && flags & libc::FD_CLOEXEC == 0
            && libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0
        {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EBADF) {
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::mark_inherited_cloexec;

    /// The tests mark the descriptors of the whole process and look at the descriptors of children:
    /// under `cargo test` (threads of one process) they must not overlap. `nextest` runs each test in
    /// a process of its own, where the lock costs nothing.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn it_is_idempotent_and_leaves_the_standard_streams_alone() {
        let _serial = serial();
        mark_inherited_cloexec().unwrap();
        mark_inherited_cloexec().unwrap();
        for fd in 0..3 {
            // SAFETY: borrowing a standard descriptor for one query; it is open for the whole test.
            let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
            assert!(
                !rustix::io::fcntl_getfd(borrowed)
                    .unwrap()
                    .contains(rustix::io::FdFlags::CLOEXEC)
            );
        }
    }

    /// The proof through a real child: `/proc/<pid>/fd` of a process started after the call.
    #[cfg(target_os = "linux")]
    mod children {
        use std::os::fd::{AsRawFd, OwnedFd};
        use std::process::Command;

        use super::mark_inherited_cloexec;
        use super::serial;
        use crate::fdguard::{scan_up_to, walk_and_mark};

        /// A descriptor that a child would inherit: `dup` does not set close-on-exec.
        fn inheritable() -> OwnedFd {
            rustix::io::dup(std::io::stdin()).unwrap()
        }

        /// Whether a child process started now has this descriptor open.
        fn child_has(fd: i32) -> bool {
            // `test` is a built-in of `sh`, so `/proc/self` is the child itself.
            Command::new("sh")
                .args(["-c", &format!("test -e /proc/self/fd/{fd}")])
                .status()
                .unwrap()
                .success()
        }

        #[test]
        fn inherited_descriptors_do_not_reach_a_child() {
            let _serial = serial();
            let kept = inheritable();
            let fd = kept.as_raw_fd();
            assert!(fd >= 3);
            assert!(
                child_has(fd),
                "the test would prove nothing: the child must see it before"
            );
            mark_inherited_cloexec().unwrap();
            assert!(!child_has(fd), "the descriptor reached the child");
            // Still open and usable here.
            rustix::io::fcntl_getfd(&kept).unwrap();
        }

        #[test]
        fn the_walk_does_the_same_without_close_range() {
            let _serial = serial();
            let kept = inheritable();
            let fd = kept.as_raw_fd();
            assert!(child_has(fd));
            walk_and_mark().unwrap();
            assert!(!child_has(fd));
            assert!(
                rustix::io::fcntl_getfd(&kept)
                    .unwrap()
                    .contains(rustix::io::FdFlags::CLOEXEC)
            );
        }

        /// Without `/proc` (a sandbox) the numbers are tried one by one.
        #[test]
        fn the_scan_does_the_same_without_proc() {
            let _serial = serial();
            let kept = inheritable();
            let fd = kept.as_raw_fd();
            assert!(child_has(fd));
            scan_up_to(fd + 8).unwrap();
            assert!(!child_has(fd));
            assert!(
                rustix::io::fcntl_getfd(&kept)
                    .unwrap()
                    .contains(rustix::io::FdFlags::CLOEXEC)
            );
        }

        #[test]
        fn the_scan_limit_is_the_soft_limit_capped() {
            let _serial = serial();
            // Whatever `RLIMIT_NOFILE` is, the scan ends: it never means billions of calls.
            let started = std::time::Instant::now();
            crate::fdguard::scan_and_mark().unwrap();
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
        }
    }
}
