// SPDX-License-Identifier: GPL-3.0-or-later

//! `destinations.toml`: reading the profiles and adding one. The file only holds names of secrets.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::destination::{DESTINATIONS_FILE, Destination, DestinationFile};
use crate::error::UploadError;

/// Path of the destinations file inside the configuration folder `dir`.
#[must_use]
pub fn destinations_path(dir: &Path) -> PathBuf {
    dir.join(DESTINATIONS_FILE)
}

fn io(path: &Path, source: std::io::Error) -> UploadError {
    UploadError::Io {
        path: path.to_owned(),
        source,
    }
}

fn read(path: &Path) -> Result<Option<String>, UploadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

/// The destinations of the configuration folder `dir`. A missing file is no destination.
///
/// # Errors
/// [`UploadError::Io`] if the file cannot be read, [`UploadError::Config`] if it is not valid.
pub fn load_destinations(dir: &Path) -> Result<Vec<Destination>, UploadError> {
    let path = destinations_path(dir);
    match read(&path)? {
        None => Ok(Vec::new()),
        Some(text) => {
            DestinationFile::parse(&text, &path.display().to_string()).map(|file| file.destination)
        }
    }
}

/// The destination `id`.
///
/// # Errors
/// [`UploadError::Config`] (`unknown_destination`) when there is none.
pub fn find<'a>(destinations: &'a [Destination], id: &str) -> Result<&'a Destination, UploadError> {
    destinations
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| UploadError::Config {
            reason: "unknown_destination",
            detail: format!("no destination named {id:?}"),
        })
}

/// Adds `destination` at the end of the file, keeping what is there (comments included). The result
/// is parsed again before it replaces the file, and the file is replaced in one step.
///
/// # Errors
/// [`UploadError::Config`] if the `id` is taken or the result is not valid, [`UploadError::Io`] if the
/// file cannot be written.
pub fn append_destination(dir: &Path, destination: &Destination) -> Result<(), UploadError> {
    let path = destinations_path(dir);
    let mut text = read(&path)?.unwrap_or_default();
    let shown = path.display().to_string();
    if DestinationFile::parse(&text, &shown)?
        .destination
        .iter()
        .any(|d| d.id == destination.id)
    {
        return Err(UploadError::config(format!(
            "the destination {:?} already exists",
            destination.id
        )));
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&destination.to_toml()?);
    DestinationFile::parse(&text, &shown)?;
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let temporary = dir.join(format!(".{DESTINATIONS_FILE}.{}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut f| f.write_all(text.as_bytes()).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&temporary, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(io(&path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append_destination, destinations_path, find, load_destinations};
    use crate::destination::Destination;

    #[test]
    fn missing_file_is_empty_and_append_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_destinations(dir.path()).unwrap().is_empty());
        append_destination(
            dir.path(),
            &Destination::zipline("one", "https://one.example.org"),
        )
        .unwrap();
        let path = destinations_path(dir.path());
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.insert_str(0, "# my destinations\n");
        std::fs::write(&path, text).unwrap();
        append_destination(
            dir.path(),
            &Destination::zipline("two", "https://two.example.org"),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my destinations\n"));
        let all = load_destinations(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            find(&all, "two").unwrap().url.as_deref(),
            Some("https://two.example.org")
        );
        assert!(find(&all, "three").is_err());
        assert!(append_destination(dir.path(), &all[0]).is_err());
    }

    #[test]
    fn an_invalid_file_is_a_config_error_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(destinations_path(dir.path()), "[[destination]]\nid = 3\n").unwrap();
        let err = load_destinations(dir.path()).unwrap_err();
        assert_eq!(err.exit_code().code(), 6);
        assert!(err.to_string().contains("destinations.toml:"), "{err}");
    }
}
