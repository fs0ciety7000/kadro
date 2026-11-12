// SPDX-License-Identifier: GPL-3.0-or-later

//! The advisory lock that makes read-modify-write of the single Keychain item atomic between
//! processes (`docs/security/secrets.md` section 3.4): `flock` on a **stable** file, `store.lock`,
//! in the data folder of Kropa (never on the item, never on a file that is renamed).
//!
//! The folder is created `0700` and the file `0600`; the file is opened `O_NOFOLLOW` and its owner
//! and mode are checked on the open descriptor, not on the path.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

/// Takes the exclusive lock on `path` (the file is created if need be), waiting for whoever holds
/// it. The lock is released when the returned file is dropped.
///
/// # Errors
/// An I/O error, or `PermissionDenied` when the file is a link, belongs to someone else or can be
/// read by others.
pub fn acquire(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != crate::system::effective_uid() || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the lock file is not ours or can be read by others",
        ));
    }
    file.lock()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::sync::{Arc, Mutex};

    use super::acquire;

    #[test]
    fn the_lock_file_and_its_folder_are_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Kropa").join("store.lock");
        let _lock = acquire(&path).unwrap();
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(file_mode & 0o777, 0o600);
        assert_eq!(dir_mode & 0o777, 0o700);
    }

    #[test]
    fn a_link_or_a_file_open_to_others_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"").unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(acquire(&link).is_err());
        let open = dir.path().join("open.lock");
        std::fs::write(&open, b"").unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(acquire(&open).is_err());
    }

    #[test]
    fn two_writers_never_hold_the_lock_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("store.lock"));
        let inside = Arc::new(Mutex::new(0_u32));
        let overlap = Arc::new(Mutex::new(false));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (path, inside, overlap) = (path.clone(), inside.clone(), overlap.clone());
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        let _lock = acquire(&path).unwrap();
                        *inside.lock().unwrap() += 1;
                        if *inside.lock().unwrap() > 1 {
                            *overlap.lock().unwrap() = true;
                        }
                        std::thread::yield_now();
                        *inside.lock().unwrap() -= 1;
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert!(!*overlap.lock().unwrap());
    }
}
