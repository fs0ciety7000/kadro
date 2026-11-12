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
//! so this module is the one place of the command line where `unsafe` is allowed (`AGENTS.md`, rule
//! 12). Linux uses `close_range(3, ~0, CLOSE_RANGE_CLOEXEC)` (kernel 5.11); everything else, and a
//! kernel or a sandbox that refuses it, walks `/proc/self/fd` (`/dev/fd` on macOS).

#![allow(unsafe_code)]

use std::io;

/// First descriptor that is not standard input, output or error.
const FIRST: libc::c_int = 3;

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

/// Lists the descriptor directory, then sets the flag one descriptor at a time.
fn walk_and_mark() -> io::Result<()> {
    let directory = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    // Collected first: the directory handle of the walk is itself in the list, and the flag is set
    // after the listing is complete.
    let mut numbers = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name();
        if let Some(number) = name.to_str().and_then(|n| n.parse::<libc::c_int>().ok())
            && number >= FIRST
        {
            numbers.push(number);
        }
    }
    for fd in numbers {
        // SAFETY: `fcntl` with `F_GETFD` and `F_SETFD` takes integers only. A number that is not
        // open any more (the handle of the walk, closed when `read_dir` ended) gives `EBADF`, which is
        // ignored; a descriptor that is open is only given one more flag, and never closed.
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
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::process::Command;

    use super::{mark_inherited_cloexec, walk_and_mark};

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

    // Each test of `nextest` is a process of its own.
    #[test]
    #[cfg(target_os = "linux")]
    fn inherited_descriptors_do_not_reach_a_child() {
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
    #[cfg(target_os = "linux")]
    fn the_walk_does_the_same_without_close_range() {
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

    #[test]
    fn it_is_idempotent_and_leaves_the_standard_streams_alone() {
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
}
