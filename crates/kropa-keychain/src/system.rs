// SPDX-License-Identifier: GPL-3.0-or-later

//! The real facts of a launch ([`SystemFacts`]) and the descriptor of the channel.
//!
//! `unsafe` is allowed in this module only, for the calls of libc that have no safe wrapper in the
//! standard library (`fstat`, `getsockopt`, `getsockname` on a descriptor number that is not ours to
//! own yet), and for taking fd 3 as a socket once the checks passed. Each block says why it holds.
//! Everything above (the order of the checks, the refusals) is in [`crate::guard`] and is tested
//! without these calls.
#![allow(unsafe_code)]

use std::io;
use std::mem::{MaybeUninit, size_of};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream;

use crate::guard::{ChannelState, LaunchFacts, PeerToken, ProjectCertificate, Unverified};

/// `AF_UNIX` as the type of `ss_family` (1 on every system we run on, so the cast cannot truncate).
#[allow(clippy::cast_possible_truncation)]
const AF_UNIX: libc::sa_family_t = libc::AF_UNIX as libc::sa_family_t;

/// The descriptor the client hands the broker its end of the socket pair on.
pub const CHANNEL_FD: RawFd = 3;

/// The facts of this process, from the system. Under macOS the peer token and the code signature
/// come from Security.framework ([`crate::apple`]); elsewhere they cannot be established, so a
/// broker built there refuses at the code check: **there is no broker outside macOS**.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFacts;

/// The effective user ID of this process.
#[must_use]
pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid` takes no argument, cannot fail and touches no memory.
    unsafe { libc::geteuid() }
}

/// `getsockopt` of a plain integer option.
pub(crate) fn int_option(
    fd: RawFd,
    level: libc::c_int,
    name: libc::c_int,
) -> io::Result<libc::c_int> {
    let mut value: libc::c_int = 0;
    let mut len = libc::socklen_t::try_from(size_of::<libc::c_int>())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `value` and `len` are valid for writes for the duration of the call and `len` is the
    // size of `value`; `fd` is only a number, a bad one makes the call fail with EBADF.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            level,
            name,
            (&raw mut value).cast::<libc::c_void>(),
            &raw mut len,
        )
    };
    if rc == 0 {
        Ok(value)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// What the descriptor `fd` is.
#[must_use]
pub fn channel_state(fd: RawFd) -> ChannelState {
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `stat` is valid for writes; a closed or invalid `fd` makes the call fail (EBADF),
    // which is the "missing" answer.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return ChannelState::Missing;
    }
    // SAFETY: `fstat` succeeded, so the structure is initialised.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFSOCK {
        return ChannelState::NotAConnectedUnixStream;
    }
    let is_stream =
        int_option(fd, libc::SOL_SOCKET, libc::SO_TYPE).is_ok_and(|t| t == libc::SOCK_STREAM);
    if !is_stream {
        return ChannelState::NotAConnectedUnixStream;
    }
    let mut address = MaybeUninit::<libc::sockaddr_storage>::zeroed();
    let Ok(mut len) = libc::socklen_t::try_from(size_of::<libc::sockaddr_storage>()) else {
        return ChannelState::NotAConnectedUnixStream;
    };
    // SAFETY: `address` is valid for `len` bytes of writes, `len` is its size.
    let named = unsafe { libc::getsockname(fd, address.as_mut_ptr().cast(), &raw mut len) };
    // SAFETY: only read after the call succeeded (`named == 0`), and the structure was zeroed.
    let family = (named == 0).then(|| unsafe { address.assume_init() }.ss_family);
    if family != Some(AF_UNIX) {
        return ChannelState::NotAConnectedUnixStream;
    }
    // Connected: the other end is there (an unconnected stream socket has no peer).
    let mut peer = MaybeUninit::<libc::sockaddr_storage>::zeroed();
    let Ok(mut peer_len) = libc::socklen_t::try_from(size_of::<libc::sockaddr_storage>()) else {
        return ChannelState::NotAConnectedUnixStream;
    };
    // SAFETY: as above, for `peer`.
    let connected = unsafe { libc::getpeername(fd, peer.as_mut_ptr().cast(), &raw mut peer_len) };
    if connected == 0 {
        ChannelState::Connected
    } else {
        ChannelState::NotAConnectedUnixStream
    }
}

/// Takes fd 3 as the socket it is. Call it once, and only after [`channel_state`] said
/// [`ChannelState::Connected`]: the descriptor is then owned by the returned stream.
///
/// # Errors
/// The error of `fcntl` when the descriptor cannot be marked close-on-exec (it is not open).
pub fn take_channel() -> io::Result<UnixStream> {
    // SAFETY: fd 3 was handed to this process by its parent for this purpose, nothing else in the
    // process owns or closes it (the standard library opens none), and `channel_state` found a
    // connected socket there. If it is not open, `fcntl` fails and nothing is built from it.
    if unsafe { libc::fcntl(CHANNEL_FD, libc::F_GETFD) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above; the `UnixStream` becomes the only owner of the descriptor.
    let stream = unsafe { UnixStream::from_raw_fd(CHANNEL_FD) };
    // A broker never starts another program, but a descriptor that cannot leak does not leak.
    // SAFETY: `fcntl` on a descriptor the stream owns, with a plain flag.
    unsafe { libc::fcntl(CHANNEL_FD, libc::F_SETFD, libc::FD_CLOEXEC) };
    Ok(stream)
}

impl LaunchFacts for SystemFacts {
    fn parent_pid(&self) -> u32 {
        std::os::unix::process::parent_id()
    }

    fn channel(&self) -> ChannelState {
        channel_state(CHANNEL_FD)
    }

    #[cfg(target_os = "macos")]
    fn peer_token(&self) -> Result<PeerToken, Unverified> {
        crate::apple::peer_token(CHANNEL_FD)
    }

    #[cfg(not(target_os = "macos"))]
    fn peer_token(&self) -> Result<PeerToken, Unverified> {
        Err(Unverified("LOCAL_PEERTOKEN exists on macOS only"))
    }

    #[cfg(target_os = "macos")]
    fn project_certificate(&self) -> Result<ProjectCertificate, Unverified> {
        crate::apple::own_certificate()
    }

    #[cfg(not(target_os = "macos"))]
    fn project_certificate(&self) -> Result<ProjectCertificate, Unverified> {
        Err(Unverified("code signatures are checked on macOS only"))
    }

    #[cfg(target_os = "macos")]
    fn check_peer_code(
        &self,
        token: &PeerToken,
        certificate: &ProjectCertificate,
    ) -> Result<(), Unverified> {
        crate::apple::check_peer(token, certificate)
    }

    #[cfg(not(target_os = "macos"))]
    fn check_peer_code(
        &self,
        _token: &PeerToken,
        _certificate: &ProjectCertificate,
    ) -> Result<(), Unverified> {
        Err(Unverified("code signatures are checked on macOS only"))
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::channel_state;
    use crate::guard::ChannelState;

    #[test]
    fn a_connected_unix_socket_is_a_channel() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(channel_state(a.as_raw_fd()), ChannelState::Connected);
    }

    #[test]
    fn a_descriptor_that_is_not_open_is_missing() {
        // Far above what a test opens (and above the default limit of macOS): never open.
        assert_eq!(channel_state(4000), ChannelState::Missing);
    }

    #[test]
    fn a_file_a_pipe_and_a_listener_are_not_channels() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert_eq!(
            channel_state(file.as_raw_fd()),
            ChannelState::NotAConnectedUnixStream
        );
        let (read, _write) = std::io::pipe().unwrap();
        assert_eq!(
            channel_state(read.as_raw_fd()),
            ChannelState::NotAConnectedUnixStream
        );
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(dir.path().join("s")).unwrap();
        assert_eq!(
            channel_state(listener.as_raw_fd()),
            ChannelState::NotAConnectedUnixStream
        );
    }

    #[test]
    fn a_datagram_socket_is_not_a_stream_channel() {
        let (a, _b) = std::os::unix::net::UnixDatagram::pair().unwrap();
        assert_eq!(
            channel_state(a.as_raw_fd()),
            ChannelState::NotAConnectedUnixStream
        );
    }
}
