// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the socket lives, and the checks on that place (`docs/security/secrets.md` section 1.2).
//!
//! Linux: `$XDG_RUNTIME_DIR/kropa/`. macOS: `~/Library/Caches/Kropa/`. There is no fallback: without
//! `XDG_RUNTIME_DIR` (su, cron, SSH without logind) there is no place that is private by construction,
//! and `/tmp` is not one.

use std::fs::DirBuilder;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::error::IpcError;

/// File name of the socket in the runtime directory.
pub const SOCKET_NAME: &str = "app.sock";

/// File name of the lock that decides which process owns the socket.
pub const LOCK_NAME: &str = "app.lock";

/// Longest socket path the platform accepts in `sockaddr_un`, terminating NUL included.
#[cfg(target_os = "macos")]
const SUN_PATH_MAX: usize = 104;
#[cfg(not(target_os = "macos"))]
const SUN_PATH_MAX: usize = 108;

/// UID of this process.
#[must_use]
pub fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// The runtime directory of this user, from the process environment. It is not created and not
/// checked: see [`ensure_dir`] and [`verify_dir`].
///
/// # Errors
/// `no_runtime_dir` when the place cannot be known, `unsafe_runtime_dir` when the variable is not an
/// absolute path.
pub fn runtime_dir() -> Result<PathBuf, IpcError> {
    runtime_dir_with(&|name| std::env::var(name).ok())
}

/// [`runtime_dir`] with the environment given by `lookup`.
///
/// # Errors
/// See [`runtime_dir`].
pub fn runtime_dir_with(lookup: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, IpcError> {
    let (variable, tail) = if cfg!(target_os = "macos") {
        ("HOME", "Library/Caches/Kropa")
    } else {
        ("XDG_RUNTIME_DIR", "kropa")
    };
    let base = match lookup(variable) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            return Err(IpcError::NoRuntimeDir(format!(
                "{variable} is not set, and there is no fallback to /tmp"
            )));
        }
    };
    if !base.is_absolute() {
        return Err(IpcError::UnsafeRuntimeDir {
            path: base,
            why: format!("{variable} is not an absolute path"),
        });
    }
    Ok(base.join(tail))
}

/// Path of the socket in `dir`.
#[must_use]
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(SOCKET_NAME)
}

/// Refuses a socket path that `sockaddr_un` would truncate.
///
/// # Errors
/// `unsafe_runtime_dir` when the path is too long.
pub fn check_socket_len(socket: &Path) -> Result<(), IpcError> {
    use std::os::unix::ffi::OsStrExt;
    if socket.as_os_str().as_bytes().len() >= SUN_PATH_MAX {
        return Err(IpcError::UnsafeRuntimeDir {
            path: socket.to_owned(),
            why: format!("the socket path must be shorter than {SUN_PATH_MAX} bytes"),
        });
    }
    Ok(())
}

fn unsafe_dir(dir: &Path, why: &str) -> IpcError {
    IpcError::UnsafeRuntimeDir {
        path: dir.to_owned(),
        why: why.to_owned(),
    }
}

/// Checks, with `lstat`, that `dir` is a real directory (not a link) owned by `uid` with mode `0700`.
///
/// # Errors
/// `unsafe_runtime_dir` for any difference, and for a directory that cannot be inspected (a missing
/// one included: see [`dir_exists`] to tell it apart first).
pub fn verify_dir(dir: &Path, uid: u32) -> Result<(), IpcError> {
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| unsafe_dir(dir, &format!("cannot inspect it: {e}")))?;
    if meta.file_type().is_symlink() {
        return Err(unsafe_dir(dir, "it is a symbolic link"));
    }
    if !meta.is_dir() {
        return Err(unsafe_dir(dir, "it is not a directory"));
    }
    if meta.uid() != uid {
        return Err(unsafe_dir(dir, "it belongs to another user"));
    }
    if meta.mode() & 0o7777 != 0o700 {
        return Err(unsafe_dir(
            dir,
            &format!("its mode is {:04o}, not 0700", meta.mode() & 0o7777),
        ));
    }
    Ok(())
}

/// Whether `dir` exists as anything at all (a dangling link counts).
#[must_use]
pub fn dir_exists(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok()
}

/// Creates `dir` with mode `0700` when it is missing (its parent must exist), then verifies it.
///
/// # Errors
/// `no_runtime_dir` when the parent is missing, `unsafe_runtime_dir` when the result is not safe.
pub fn ensure_dir(dir: &Path, uid: u32) -> Result<(), IpcError> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(IpcError::NoRuntimeDir(format!(
                "the parent of {} does not exist",
                dir.display()
            )));
        }
        Err(e) => return Err(unsafe_dir(dir, &format!("cannot create it: {e}"))),
    }
    verify_dir(dir, uid)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn reason(e: &IpcError) -> &str {
        e.reason().unwrap()
    }

    #[test]
    fn without_the_variable_there_is_no_runtime_dir() {
        let e = runtime_dir_with(&|_| None).unwrap_err();
        assert_eq!(reason(&e), "no_runtime_dir");
        assert_eq!(e.exit_code(), 14);
        let e = runtime_dir_with(&|_| Some(String::new())).unwrap_err();
        assert_eq!(reason(&e), "no_runtime_dir");
    }

    #[test]
    fn a_relative_variable_is_refused() {
        let e = runtime_dir_with(&|_| Some("run/user".into())).unwrap_err();
        assert_eq!(reason(&e), "unsafe_runtime_dir");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_directory_is_kropa_under_the_runtime_dir() {
        let dir = runtime_dir_with(&|_| Some("/run/user/1000".into())).unwrap();
        assert_eq!(dir, Path::new("/run/user/1000/kropa"));
    }

    #[test]
    fn ensure_creates_a_private_directory_and_accepts_it_again() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("kropa");
        ensure_dir(&dir, current_uid()).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o7777, 0o700);
        ensure_dir(&dir, current_uid()).unwrap();
    }

    #[test]
    fn a_directory_open_to_others_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("kropa");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = ensure_dir(&dir, current_uid()).unwrap_err();
        assert_eq!(reason(&e), "unsafe_runtime_dir");
    }

    #[test]
    fn a_symbolic_link_is_refused_even_to_a_good_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        DirBuilder::new().mode(0o700).create(&real).unwrap();
        let link = tmp.path().join("kropa");
        symlink(&real, &link).unwrap();
        assert_eq!(
            reason(&verify_dir(&link, current_uid()).unwrap_err()),
            "unsafe_runtime_dir"
        );
        assert_eq!(
            reason(&ensure_dir(&link, current_uid()).unwrap_err()),
            "unsafe_runtime_dir"
        );
    }

    #[test]
    fn another_owner_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("kropa");
        ensure_dir(&dir, current_uid()).unwrap();
        // The uid is injected: this directory is ours, so it is "another user's" for uid + 1.
        let e = verify_dir(&dir, current_uid() + 1).unwrap_err();
        assert_eq!(reason(&e), "unsafe_runtime_dir");
    }

    #[test]
    fn a_missing_parent_is_no_runtime_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let e = ensure_dir(&tmp.path().join("a/b"), current_uid()).unwrap_err();
        assert_eq!(reason(&e), "no_runtime_dir");
    }

    #[test]
    fn a_socket_path_that_would_be_truncated_is_refused() {
        let long = PathBuf::from(format!("/{}/app.sock", "a".repeat(120)));
        assert_eq!(
            reason(&check_socket_len(&long).unwrap_err()),
            "unsafe_runtime_dir"
        );
        check_socket_len(Path::new("/run/user/1000/kropa/app.sock")).unwrap();
    }
}
