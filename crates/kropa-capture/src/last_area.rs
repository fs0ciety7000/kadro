// SPDX-License-Identifier: GPL-3.0-or-later

//! The last area the user selected, kept so that `R` can take it again (`DESIGN.md` section 8.1).
//!
//! It is a small JSON file in the state folder of the user: `$XDG_STATE_HOME/kropa/last-area.json`
//! on Linux (`~/.local/state` when the variable is unset or relative), and
//! `~/Library/Application Support/<application id>/last-area.json` on macOS. The folder is created
//! `0700`, the file `0600`, opened `O_NOFOLLOW` (a link in its place is refused), and written to a
//! temporary file that is then renamed, so that a reader never sees half a file.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::region::Region;

/// Name of the file.
pub const FILE_NAME: &str = "last-area.json";
/// Most bytes read from the file: the real content is under 100.
const MAX_BYTES: u64 = 4096;

/// The operating system, for the location of the file (a parameter so that both are tested anywhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Linux and the other XDG systems.
    Linux,
    /// macOS.
    MacOs,
}

impl Os {
    /// The system this program was built for.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Where the file lives: `None` when no home folder is known.
#[must_use]
pub fn last_area_path(
    os: Os,
    xdg_state_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    match os {
        Os::Linux => {
            let base = match xdg_state_home.filter(|p| p.is_absolute()) {
                Some(state) => state.to_path_buf(),
                None => home.filter(|p| p.is_absolute())?.join(".local/state"),
            };
            Some(base.join("kropa").join(FILE_NAME))
        }
        Os::MacOs => Some(
            home.filter(|p| p.is_absolute())?
                .join("Library/Application Support")
                .join(kropa_core::APP_ID)
                .join(FILE_NAME),
        ),
    }
}

/// [`last_area_path`] from the environment of this process.
#[must_use]
pub fn default_last_area_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    last_area_path(Os::current(), state.as_deref(), home.as_deref())
}

/// Why the file could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LastAreaError {
    /// The file system refused (a link in place of the file, no room, no access...).
    #[error("last-area file: {0}")]
    Io(#[from] io::Error),
    /// The file is not a version 1 record, or its area leaves its image.
    #[error("last-area file is not a valid record")]
    Invalid,
    /// The file is not ours alone: not a regular file, too big, or readable by others.
    #[error("last-area file has unsafe permissions or size")]
    Unsafe,
}

/// The remembered area, with the size of the image it was taken on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredArea {
    /// Format version, always 1.
    pub v: u32,
    /// Size of the image the area was selected on, in pixels.
    pub image: (u32, u32),
    /// The area.
    pub region: Region,
}

impl StoredArea {
    /// A version 1 record.
    #[must_use]
    pub fn new(image: (u32, u32), region: Region) -> Self {
        Self {
            v: 1,
            image,
            region,
        }
    }

    /// Whether the record is version 1 and its area lies inside its image.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.v == 1 && self.region.fits(self.image)
    }

    /// Reads the record at `path`; `Ok(None)` when there is none yet.
    ///
    /// # Errors
    /// [`LastAreaError`]: unreadable, unsafe, or not a valid record.
    pub fn load(path: &Path) -> Result<Option<Self>, LastAreaError> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.mode() & 0o077 != 0 || meta.len() > MAX_BYTES {
            return Err(LastAreaError::Unsafe);
        }
        let mut text = String::new();
        Read::take(&mut file, MAX_BYTES).read_to_string(&mut text)?;
        let area: Self = serde_json::from_str(&text).map_err(|_| LastAreaError::Invalid)?;
        if area.is_valid() {
            Ok(Some(area))
        } else {
            Err(LastAreaError::Invalid)
        }
    }

    /// Writes the record at `path`, replacing the previous one.
    ///
    /// # Errors
    /// [`LastAreaError`]: the record is not valid, or the file system refused.
    pub fn save(&self, path: &Path) -> Result<(), LastAreaError> {
        if !self.is_valid() {
            return Err(LastAreaError::Invalid);
        }
        let dir = path.parent().ok_or(LastAreaError::Invalid)?;
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let tmp = dir.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&tmp)?;
            let text = serde_json::to_string(self).map_err(|_| LastAreaError::Invalid)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            // Best effort: the temporary file may not exist.
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn area() -> StoredArea {
        StoredArea::new(
            (2560, 1440),
            Region {
                monitor: 0,
                x: 424,
                y: 372,
                width: 1280,
                height: 720,
            },
        )
    }

    #[test]
    fn paths_follow_xdg_and_macos_conventions() {
        let home = Path::new("/home/u");
        let xdg = Path::new("/state");
        assert_eq!(
            last_area_path(Os::Linux, Some(xdg), Some(home)),
            Some(PathBuf::from("/state/kropa/last-area.json"))
        );
        assert_eq!(
            last_area_path(Os::Linux, None, Some(home)),
            Some(PathBuf::from("/home/u/.local/state/kropa/last-area.json"))
        );
        // A relative XDG_STATE_HOME is ignored, as the specification says.
        assert_eq!(
            last_area_path(Os::Linux, Some(Path::new("rel")), Some(home)),
            Some(PathBuf::from("/home/u/.local/state/kropa/last-area.json"))
        );
        assert_eq!(
            last_area_path(Os::MacOs, Some(xdg), Some(home)),
            Some(PathBuf::from(
                "/home/u/Library/Application Support/io.github.fs0ciety7000.Kropa/last-area.json"
            ))
        );
        assert_eq!(last_area_path(Os::Linux, None, None), None);
        assert_eq!(last_area_path(Os::MacOs, Some(xdg), None), None);
    }

    #[test]
    fn saves_with_private_modes_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kropa/last-area.json");
        assert!(StoredArea::load(&path).unwrap().is_none());
        area().save(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(StoredArea::load(&path).unwrap(), Some(area()));
        // Saving again replaces, and leaves no temporary file behind.
        let mut other = area();
        other.region.x = 0;
        other.save(&path).unwrap();
        assert_eq!(StoredArea::load(&path).unwrap(), Some(other));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn refuses_a_link_in_place_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.json");
        fs::write(&target, "{}").unwrap();
        let path = dir.path().join("last-area.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(matches!(StoredArea::load(&path), Err(LastAreaError::Io(_))));
    }

    #[test]
    fn refuses_a_file_readable_by_others_or_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-area.json");
        area().save(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            StoredArea::load(&path),
            Err(LastAreaError::Unsafe)
        ));

        for text in [
            "not json",
            r#"{"v":2,"image":[10,10],"region":{"monitor":0,"x":0,"y":0,"width":1,"height":1}}"#,
            r#"{"v":1,"image":[10,10],"region":{"monitor":0,"x":9,"y":0,"width":2,"height":1}}"#,
            r#"{"v":1,"image":[10,10],"region":{"monitor":0,"x":0,"y":0,"width":0,"height":1}}"#,
        ] {
            fs::remove_file(&path).unwrap();
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(
                matches!(StoredArea::load(&path), Err(LastAreaError::Invalid)),
                "{text}"
            );
        }
    }

    #[test]
    fn refuses_to_save_an_area_outside_its_image() {
        let dir = tempfile::tempdir().unwrap();
        let mut bad = area();
        bad.image = (100, 100);
        assert!(matches!(
            bad.save(&dir.path().join("a.json")),
            Err(LastAreaError::Invalid)
        ));
    }
}
