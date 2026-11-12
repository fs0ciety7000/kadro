// SPDX-License-Identifier: GPL-3.0-or-later

//! Starts the broker with the socket pair it is served on (`docs/security/secrets.md` section 2.2.1,
//! point 1): an empty environment, `/dev/null` on fd 0, 1 and 2, one end of an `AF_UNIX` stream
//! pair on fd 3 and nothing else of ours open (every descriptor Rust opens is close-on-exec).
//!
//! `unsafe` is allowed in this module only, for the one thing the standard library cannot do on
//! stable: placing a descriptor at fd 3 of the child. The specification says `posix_spawn`; this is
//! `fork` and `exec` through [`std::process::Command`], with the same effect for the child and the
//! standard library's own handling of an `exec` that fails (a missing broker is an error here, not
//! a child that dies).
#![allow(unsafe_code)]

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::system::CHANNEL_FD;

/// A broker that was started, and the end of the channel the client keeps.
pub struct Spawned {
    /// The process.
    pub child: Child,
    /// The client's end of the socket pair.
    pub channel: UnixStream,
}

/// Starts `program` with `args` and the other end of a new socket pair on fd 3.
///
/// # Errors
/// The error of `socketpair` or of starting the program (not found, not executable).
pub fn spawn_with_channel(program: &Path, args: &[&str]) -> io::Result<Spawned> {
    let (mine, theirs) = UnixStream::pair()?;
    let theirs_fd = theirs.as_raw_fd();
    if theirs_fd < CHANNEL_FD {
        // The standard descriptors are closed in this process: the pair landed on 0, 1 or 2, where
        // the child's `/dev/null` would replace it. Nothing sensible to do but say so.
        return Err(io::Error::other(
            "a standard descriptor is closed: cannot place the channel",
        ));
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs in the child between `fork` and `exec`: it only calls `dup2` and
    // `fcntl`, which are async-signal-safe, allocates nothing and takes no lock. It captures a plain
    // integer.
    unsafe {
        command.pre_exec(move || place_channel(theirs_fd));
    }
    let child = command.spawn()?;
    // The parent's copy of the child's end: closing it is what lets the client see the end of
    // file when the broker ends.
    drop(theirs);
    Ok(Spawned {
        child,
        channel: mine,
    })
}

/// In the child: puts `fd` at [`CHANNEL_FD`], not close-on-exec.
fn place_channel(fd: i32) -> io::Result<()> {
    if fd == CHANNEL_FD {
        // `dup2(3, 3)` does nothing and would keep close-on-exec: clear it.
        // SAFETY: `fcntl` on an open descriptor with a plain flag.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }
    // SAFETY: `dup2` of an open descriptor onto fd 3; it clears close-on-exec on the copy.
    if unsafe { libc::dup2(fd, CHANNEL_FD) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
