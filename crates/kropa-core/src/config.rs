// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration (`config.toml`).
//!
//! The file lives in the standard configuration folder (`directories`), or in the folder given with
//! `--config`. A missing file means the defaults. It never holds a secret: destinations are referred
//! to by profile name only.

use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};

use crate::{CoreError, NameTemplate};

/// Name of the configuration file inside the configuration folder.
pub const CONFIG_FILE: &str = "config.toml";
/// Application folder name, under the standard configuration folder and in the pictures folder.
const APP_DIR: &str = "kropa";
/// Sub-folder of the pictures folder used by default.
const PICTURES_SUBDIR: &str = "Kropa";
/// Default name template.
pub const DEFAULT_NAME_TEMPLATE: &str = "Kropa_{date}_{time}";

/// Image format of saved captures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// PNG (default).
    #[default]
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    Webp,
}

impl ImageFormat {
    /// File extension, without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

/// User configuration. Every key is optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Folder where captures are saved. `None`: the default pictures folder (see [`default_output_dir`]).
    /// A leading `~/` is expanded to the home directory.
    pub output_dir: Option<PathBuf>,
    /// Interface language (`fr`, `en`); `None`: negotiated from the environment.
    pub language: Option<String>,
    /// File-name template (see [`NameTemplate`]).
    pub name_template: String,
    /// Image format of saved captures.
    pub image_format: ImageFormat,
    /// Name of the default destination profile; `None`: no upload by default. Never a secret.
    pub default_destination: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_dir: None,
            language: None,
            name_template: DEFAULT_NAME_TEMPLATE.to_owned(),
            image_format: ImageFormat::default(),
            default_destination: None,
        }
    }
}

impl Config {
    /// Parses a configuration from TOML text; `origin` is only used in error messages.
    ///
    /// # Errors
    /// [`CoreError::ConfigParse`] (with line and column) on a syntax error, an unknown key or a wrong
    /// type; [`CoreError::ConfigValue`] on a value that is not acceptable.
    pub fn from_toml_str(text: &str, origin: &Path) -> Result<Self, CoreError> {
        let config: Self = toml::from_str(text).map_err(|e| {
            let (line, column) = e.span().map_or((1, 1), |s| line_column(text, s.start));
            CoreError::ConfigParse {
                path: origin.to_owned(),
                line,
                column,
                message: e.message().to_owned(),
            }
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Loads `path`. A missing file gives the defaults.
    ///
    /// # Errors
    /// [`CoreError::Io`] if the file exists but cannot be read, plus the errors of
    /// [`Config::from_toml_str`].
    pub fn load(path: &Path) -> Result<Self, CoreError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml_str(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(CoreError::Io {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Loads `config.toml` from `dir`, or from the standard configuration folder when `dir` is `None`.
    ///
    /// # Errors
    /// [`CoreError::NoHome`] if there is no standard folder, plus the errors of [`Config::load`].
    pub fn load_from(dir: Option<&Path>) -> Result<Self, CoreError> {
        let dir = match dir {
            Some(d) => d.to_owned(),
            None => config_dir()?,
        };
        Self::load(&dir.join(CONFIG_FILE))
    }

    /// Checks the values that serde cannot.
    ///
    /// # Errors
    /// [`CoreError::ConfigValue`] naming the key.
    pub fn validate(&self) -> Result<(), CoreError> {
        NameTemplate::parse(&self.name_template).map_err(|e| CoreError::ConfigValue {
            key: "name_template",
            reason: e.to_string(),
        })?;
        if let Some(lang) = &self.language {
            let ok = (2..=35).contains(&lang.len())
                && lang
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !ok {
                return Err(CoreError::ConfigValue {
                    key: "language",
                    reason: format!(
                        "{lang:?} is not a language tag (expected for example \"fr\" or \"en\")"
                    ),
                });
            }
        }
        if let Some(profile) = &self.default_destination {
            let ok = (1..=64).contains(&profile.len())
                && profile
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !ok {
                return Err(CoreError::ConfigValue {
                    key: "default_destination",
                    reason: "a profile name uses 1 to 64 letters, digits, '-' or '_'".into(),
                });
            }
        }
        Ok(())
    }

    /// Folder where captures are saved: `output_dir` (with `~/` expanded) or the default.
    ///
    /// # Errors
    /// [`CoreError::NoHome`] if the folder depends on a home directory that does not exist.
    pub fn resolved_output_dir(&self) -> Result<PathBuf, CoreError> {
        match &self.output_dir {
            None => default_output_dir(),
            Some(dir) => match dir.strip_prefix("~") {
                Ok(rest) => {
                    let home = BaseDirs::new().ok_or(CoreError::NoHome("home"))?;
                    Ok(home.home_dir().join(rest))
                }
                Err(_) => Ok(dir.clone()),
            },
        }
    }
}

/// Standard configuration folder of Kropa (`~/.config/kropa` on Linux, XDG aware).
///
/// # Errors
/// [`CoreError::NoHome`] if there is no home directory.
pub fn config_dir() -> Result<PathBuf, CoreError> {
    ProjectDirs::from("", "", APP_DIR)
        .map(|d| d.config_dir().to_owned())
        .ok_or(CoreError::NoHome("config"))
}

/// Default output folder: `Kropa` inside the user's pictures folder (localised through XDG
/// `user-dirs` on Linux, `~/Pictures` on macOS), or `~/Images/Kropa` when the system names none.
///
/// # Errors
/// [`CoreError::NoHome`] if there is no home directory.
pub fn default_output_dir() -> Result<PathBuf, CoreError> {
    let user = UserDirs::new().ok_or(CoreError::NoHome("pictures"))?;
    let pictures = user
        .picture_dir()
        .map_or_else(|| user.home_dir().join("Images"), Path::to_owned);
    Ok(pictures.join(PICTURES_SUBDIR))
}

/// Writes `text` to `path` in one step: a temporary file next to it (created new, flushed to disk),
/// then a rename, then a flush of the directory. A reader sees the old file or the new one, never
/// half of it, and nothing is left behind when a step fails.
///
/// The mode of a file that exists is kept (`0600` for a new one: a configuration may name secrets and
/// hosts). A symbolic link is written **through**: the file it points to is replaced, in its own
/// directory, and the link stays (a dotfile manager keeps its link). A link that points nowhere is an
/// error: nothing is created behind it. There is no lock between two writers: the last rename wins.
///
/// # Errors
/// The I/O error of the step that failed.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;

    let resolved;
    let path = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        resolved = std::fs::canonicalize(path)?;
        resolved.as_path()
    } else {
        path
    };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
        let mode = std::fs::metadata(path).map_or(0o600, |m| m.mode() & 0o7777);
        options.mode(mode);
        mode
    };
    let written = options
        .open(&temporary)
        .and_then(|mut f| {
            // The umask may have taken bits off at creation: the mode is set exactly.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                f.set_permissions(std::fs::Permissions::from_mode(mode))?;
            }
            f.write_all(text.as_bytes()).and_then(|()| f.sync_all())
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return written;
    }
    // The rename itself reaches the disk only once the directory does.
    #[cfg(unix)]
    std::fs::File::open(dir).and_then(|d| d.sync_all())?;
    written
}

/// Sets (`Some`) or clears (`None`) `default_destination` in the `config.toml` of `dir`, keeping the
/// comments and the order of the file. The result is read back with [`Config::from_toml_str`] before it
/// replaces the file, which is written in one step ([`write_atomic`]). A missing file is created.
///
/// # Errors
/// [`CoreError::ConfigParse`] or [`CoreError::ConfigValue`] when the file, or the profile name, is not
/// valid (nothing is written); [`CoreError::Io`] when the file cannot be read or written.
pub fn set_default_destination(dir: &Path, profile: Option<&str>) -> Result<(), CoreError> {
    let path = dir.join(CONFIG_FILE);
    let io = |source| CoreError::Io {
        path: path.clone(),
        source,
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io(e)),
    };
    // The file must be valid as it is, so that nothing but the key changes.
    Config::from_toml_str(&text, &path)?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        let (line, column) = e.span().map_or((1, 1), |s| line_column(&text, s.start));
        CoreError::ConfigParse {
            path: path.clone(),
            line,
            column,
            message: e.message().to_owned(),
        }
    })?;
    match profile {
        Some(profile) => doc["default_destination"] = toml_edit::value(profile),
        None => {
            doc.remove("default_destination");
        }
    }
    let result = doc.to_string();
    Config::from_toml_str(&result, &path)?;
    write_atomic(&path, &result).map_err(io)
}

/// 1-based line and column (in characters) of byte offset `offset` in `text`.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        Config, DEFAULT_NAME_TEMPLATE, ImageFormat, line_column, set_default_destination,
        write_atomic,
    };
    use crate::{CoreError, ExitCode};

    fn parse(text: &str) -> Result<Config, CoreError> {
        Config::from_toml_str(text, Path::new("config.toml"))
    }

    #[test]
    fn empty_file_gives_defaults() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.name_template, DEFAULT_NAME_TEMPLATE);
        assert_eq!(c.image_format, ImageFormat::Png);
    }

    #[test]
    fn full_file() {
        let c = parse(
            "output_dir = \"/srv/shots\"\nlanguage = \"fr\"\nname_template = \"{date}-{n:3}\"\n\
             image_format = \"webp\"\ndefault_destination = \"zipline\"\n",
        )
        .unwrap();
        assert_eq!(c.output_dir.as_deref(), Some(Path::new("/srv/shots")));
        assert_eq!(c.language.as_deref(), Some("fr"));
        assert_eq!(c.image_format, ImageFormat::Webp);
        assert_eq!(c.image_format.extension(), "webp");
        assert_eq!(c.default_destination.as_deref(), Some("zipline"));
        assert_eq!(c.resolved_output_dir().unwrap(), Path::new("/srv/shots"));
    }

    #[test]
    fn roundtrip() {
        let c = Config {
            default_destination: Some("s3-prod".into()),
            ..Config::default()
        };
        let text = toml::to_string(&c).unwrap();
        assert_eq!(parse(&text).unwrap(), c);
    }

    #[test]
    fn parse_error_has_line_and_column() {
        let err = parse("language = \"fr\"\nimage_format = = \"png\"\n").unwrap_err();
        let CoreError::ConfigParse {
            line, column, path, ..
        } = &err
        else {
            panic!("{err:?}")
        };
        assert_eq!((*line, *column), (2, 16));
        assert_eq!(path, Path::new("config.toml"));
        assert_eq!(err.exit_code(), ExitCode::Config);
        assert!(err.to_string().starts_with("config.toml:2:16: "));
    }

    #[test]
    fn unknown_key_and_wrong_type_are_errors() {
        let err = parse("a = 1\n").unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::ConfigParse {
                    line: 1,
                    column: 1,
                    ..
                }
            ),
            "{err:?}"
        );
        let err = parse("image_format = \"bmp\"\n").unwrap_err();
        assert!(
            matches!(err, CoreError::ConfigParse { line: 1, .. }),
            "{err:?}"
        );
        let err = parse("name_template = 3\n").unwrap_err();
        assert!(matches!(err, CoreError::ConfigParse { .. }), "{err:?}");
    }

    #[test]
    fn invalid_values_name_the_key() {
        for (text, key) in [
            ("name_template = \"{nope}\"", "name_template"),
            ("name_template = \"../x\"", "name_template"),
            ("language = \"fr fr\"", "language"),
            ("default_destination = \"a/b\"", "default_destination"),
            ("default_destination = \"\"", "default_destination"),
        ] {
            match parse(text) {
                Err(CoreError::ConfigValue { key: k, .. }) if k == key => {}
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn missing_file_gives_defaults_and_directory_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load_from(Some(dir.path())).unwrap(),
            Config::default()
        );
        std::fs::write(dir.path().join("config.toml"), "image_format = \"jpeg\"\n").unwrap();
        assert_eq!(
            Config::load_from(Some(dir.path())).unwrap().image_format,
            ImageFormat::Jpeg
        );
        let sub = tempfile::tempdir().unwrap();
        std::fs::create_dir(sub.path().join("config.toml")).unwrap();
        let err = Config::load_from(Some(sub.path())).unwrap_err();
        assert!(matches!(err, CoreError::Io { .. }));
        assert_eq!(err.exit_code(), ExitCode::Io);
    }

    #[test]
    fn line_column_counts_characters() {
        assert_eq!(line_column("ab\ncé=x", 6), (2, 3));
        assert_eq!(line_column("abc", 99), (1, 4));
    }
    #[test]
    fn default_destination_is_set_and_cleared_without_touching_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // A missing file is created.
        set_default_destination(dir.path(), Some("zip")).unwrap();
        assert_eq!(
            Config::load_from(Some(dir.path()))
                .unwrap()
                .default_destination
                .as_deref(),
            Some("zip")
        );
        std::fs::write(
            &path,
            "# my settings\nlanguage = \"fr\"  # keep\ndefault_destination = \"zip\"\nimage_format = \"webp\"\n",
        )
        .unwrap();
        set_default_destination(dir.path(), Some("s3-prod")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my settings\n"), "{text}");
        assert!(text.contains("language = \"fr\"  # keep"), "{text}");
        assert!(text.contains("default_destination = \"s3-prod\""), "{text}");
        set_default_destination(dir.path(), None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("default_destination"), "{text}");
        assert!(text.contains("image_format = \"webp\""), "{text}");
        assert_eq!(
            Config::load_from(Some(dir.path()))
                .unwrap()
                .default_destination,
            None
        );
        // Clearing what is not there is fine.
        set_default_destination(dir.path(), None).unwrap();
    }

    #[test]
    fn default_destination_refuses_a_bad_name_or_a_bad_file_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "language = \"fr\"\n").unwrap();
        let err = set_default_destination(dir.path(), Some("a/b")).unwrap_err();
        assert!(matches!(err, CoreError::ConfigValue { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "language = \"fr\"\n"
        );
        std::fs::write(&path, "unknown = 1\n").unwrap();
        let err = set_default_destination(dir.path(), Some("zip")).unwrap_err();
        assert!(matches!(err, CoreError::ConfigParse { .. }), "{err:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "unknown = 1\n");
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file is left behind");
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("f.toml");
        write_atomic(&path, "a = 1\n").unwrap();
        write_atomic(&path, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_the_mode_and_creates_new_files_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let fresh = dir.path().join("fresh.toml");
        write_atomic(&fresh, "a = 1\n").unwrap();
        assert_eq!(mode(&fresh), 0o600);
        let shared = dir.path().join("shared.toml");
        std::fs::write(&shared, "a = 0\n").unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&shared, "a = 1\n").unwrap();
        assert_eq!(mode(&shared), 0o640);
        let private = dir.path().join("private.toml");
        std::fs::write(&private, "a = 0\n").unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&private, "a = 1\n").unwrap();
        assert_eq!(mode(&private), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_writes_through_a_link_and_refuses_a_dangling_one() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles");
        std::fs::create_dir(&real).unwrap();
        let target = real.join("destinations.toml");
        std::fs::write(&target, "a = 0\n").unwrap();
        let link = dir.path().join("destinations.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_atomic(&link, "a = 1\n").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "a = 1\n");
        assert_eq!(std::fs::read_dir(&real).unwrap().count(), 1);
        // A link to nothing: an error, and nothing is created behind it.
        let dangling = dir.path().join("dangling.toml");
        std::os::unix::fs::symlink(dir.path().join("nowhere.toml"), &dangling).unwrap();
        assert!(write_atomic(&dangling, "a = 1\n").is_err());
        assert!(!dir.path().join("nowhere.toml").exists());
    }
}
