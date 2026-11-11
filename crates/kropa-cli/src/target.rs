// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a capture will be written: `--output`, `--name` and the configured name template
//! (`docs/cli.md` section 2.1).

use std::path::{Path, PathBuf};

use kropa_core::{
    Clock, Config, CoreError, ExitCode, NameContext, NameError, NameTemplate, path_taken,
};
use kropa_i18n::Localizer;

use crate::errors::CliError;

/// Extensions that make `--output` a file path rather than a folder.
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// What the user asked for.
#[derive(Debug, Clone, Copy)]
pub struct TargetSpec<'a> {
    /// `screen`, `output` or `area`.
    pub mode: &'static str,
    /// Screen name given to `capture output`.
    pub connector: Option<&'a str>,
    /// `--output`.
    pub output: Option<&'a Path>,
    /// `--name`.
    pub name: Option<&'a str>,
    /// `--force`.
    pub force: bool,
}

/// The outcome of the resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The file that will be written.
    File(PathBuf),
    /// The name needs a value only the capture knows (`{w}`, `{h}`, or `{output}` for the primary
    /// screen): only the folder is known yet.
    Deferred(PathBuf),
}

fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn name_error(e: NameError, loc: &Localizer) -> CliError {
    CliError::from_core(&CoreError::Name(e), loc)
}

fn io_error(loc: &Localizer, id: &str, path: &Path) -> CliError {
    CliError::new(
        ExitCode::Io,
        loc,
        id,
        &[("path", &path.display().to_string())],
    )
}

/// Resolves the target of a capture without touching the disk beyond existence checks.
///
/// # Errors
/// Usage error for a refused name or `--name` with a file `--output`; configuration error for a bad
/// configured template or no home folder; I/O error when `--output` names a non-folder or the file
/// exists without `--force`.
pub fn resolve_target(
    spec: &TargetSpec<'_>,
    config: &Config,
    clock: &dyn Clock,
    host: Option<&str>,
    loc: &Localizer,
) -> Result<Target, CliError> {
    resolve(spec, config, clock, host, loc, None)
}

/// Like [`resolve_target`], once the size of the image is known: `{w}` and `{h}` have a value, so
/// the result is always a file.
///
/// # Errors
/// The errors of [`resolve_target`]; a name that still lacks a value is a usage error.
pub fn resolve_final(
    spec: &TargetSpec<'_>,
    config: &Config,
    clock: &dyn Clock,
    host: Option<&str>,
    loc: &Localizer,
    size: (u32, u32),
) -> Result<PathBuf, CliError> {
    match resolve(spec, config, clock, host, loc, Some(size))? {
        Target::File(path) => Ok(path),
        Target::Deferred(_) => Err(name_error(NameError::MissingVariable("output"), loc)),
    }
}

fn resolve(
    spec: &TargetSpec<'_>,
    config: &Config,
    clock: &dyn Clock,
    host: Option<&str>,
    loc: &Localizer,
    size: Option<(u32, u32)>,
) -> Result<Target, CliError> {
    let ext = config.image_format.extension();
    let dir = match spec.output {
        Some(path) if has_image_extension(path) => {
            if spec.name.is_some() {
                return Err(CliError::new(
                    ExitCode::Usage,
                    loc,
                    "error-name-with-file",
                    &[],
                ));
            }
            check_explicit_file(path).map_err(|e| name_error(e, loc))?;
            return existing_check(path.to_owned(), spec.force, loc);
        }
        Some(path) => path.to_owned(),
        None => config
            .resolved_output_dir()
            .map_err(|e| CliError::from_core(&e, loc))?,
    };
    if dir.exists() && !dir.is_dir() {
        return Err(io_error(loc, "error-output-not-dir", &dir));
    }
    let source = spec.name.unwrap_or(&config.name_template);
    let template = NameTemplate::parse(source).map_err(|e| name_error(e, loc))?;
    let ctx = NameContext {
        kind: Some(spec.mode.to_owned()),
        output: spec.connector.map(str::to_owned),
        host: host.map(str::to_owned),
        width: size.map(|(w, _)| w),
        height: size.map(|(_, h)| h),
        ..NameContext::default()
    };
    match template.render_unique(&dir, &ctx, clock, ext) {
        Ok(path) => existing_check(path, spec.force, loc),
        Err(CoreError::Name(NameError::MissingVariable(v)))
            if matches!(v, "w" | "h")
                || (v == "output" && spec.mode == "output" && spec.connector.is_none()) =>
        {
            Ok(Target::Deferred(dir))
        }
        Err(e) => Err(CliError::from_core(&e, loc)),
    }
}

/// A file name the user gave in full with `--output` is theirs: it is refused only when it cannot
/// name a file at all (nothing, `/`, `..`, a NUL byte). Unlike a generated name it may start with a
/// dot, hold a colon or any text; being valid UTF-8 is not required either.
fn check_explicit_file(path: &Path) -> Result<(), NameError> {
    let Some(name) = path.file_name() else {
        return Err(NameError::UnsafeName("the path has no file name"));
    };
    if name.is_empty() {
        return Err(NameError::UnsafeName("empty name"));
    }
    if name.as_encoded_bytes().contains(&0) {
        return Err(NameError::UnsafeName("NUL byte in the file name"));
    }
    Ok(())
}

/// Refuses an existing target unless `--force`. This check is only there to answer early and
/// clearly: what really protects a file is the write, which opens it with `create_new` (`O_EXCL`)
/// when there is no `--force`, and replaces it atomically (temporary file, then rename) with it.
/// A path that cannot be inspected is an error, not a free path.
fn existing_check(path: PathBuf, force: bool, loc: &Localizer) -> Result<Target, CliError> {
    if !force && path_taken(&path).map_err(|e| CliError::from_core(&e, loc))? {
        return Err(io_error(loc, "error-file-exists", &path));
    }
    Ok(Target::File(path))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use kropa_core::{Config, ExitCode, FixedClock};
    use kropa_i18n::{Lang, Localizer};

    use super::{Target, TargetSpec, resolve_target};

    fn clock() -> FixedClock {
        FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap())
    }

    fn loc() -> Localizer {
        Localizer::new(Lang::En).unwrap()
    }

    fn spec<'a>(output: Option<&'a Path>, name: Option<&'a str>) -> TargetSpec<'a> {
        TargetSpec {
            mode: "screen",
            connector: None,
            output,
            name,
            force: false,
        }
    }

    fn resolve(spec: &TargetSpec<'_>) -> Result<Target, crate::errors::CliError> {
        resolve_target(spec, &Config::default(), &clock(), Some("laptop"), &loc())
    }

    #[test]
    fn default_template_in_the_given_folder() {
        let dir = tempfile::tempdir().unwrap();
        let got = resolve(&spec(Some(dir.path()), None)).unwrap();
        assert_eq!(
            got,
            Target::File(dir.path().join("Kropa_2026-11-11_14-03-22.png"))
        );
    }

    #[test]
    fn name_option_with_variables() {
        let dir = tempfile::tempdir().unwrap();
        let got = resolve(&spec(Some(dir.path()), Some("{date}-{host}-{mode}"))).unwrap();
        assert_eq!(
            got,
            Target::File(dir.path().join("2026-11-11-laptop-screen.png"))
        );
    }

    #[test]
    fn output_with_extension_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("shot.PNG");
        assert_eq!(
            resolve(&spec(Some(&file), None)).unwrap(),
            Target::File(file.clone())
        );
        let e = resolve(&spec(Some(&file), Some("x"))).unwrap_err();
        assert_eq!(e.code, ExitCode::Usage);
    }

    #[test]
    fn an_explicit_file_name_is_only_refused_when_it_cannot_name_a_file() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".hidden.png", "a:b.png", "x y .png", "é\u{202E}.png", "..png"] {
            let file = dir.path().join(name);
            assert_eq!(
                resolve(&spec(Some(&file), None)).unwrap(),
                Target::File(file.clone()),
                "{name:?}"
            );
        }
        let nul = dir.path().join("a\0b.png");
        assert_eq!(resolve(&spec(Some(&nul), None)).unwrap_err().code, ExitCode::Usage);
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_accepted_for_an_explicit_file() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let mut raw = dir.path().as_os_str().to_owned().into_vec();
        raw.extend_from_slice(b"/caf\xe9.png");
        let file = PathBuf::from(std::ffi::OsString::from_vec(raw));
        assert_eq!(
            resolve(&spec(Some(&file), None)).unwrap(),
            Target::File(file.clone())
        );
    }

    #[test]
    fn a_path_that_cannot_be_inspected_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain");
        std::fs::write(&plain, b"x").unwrap();
        // `plain/shot.png`: a file in the middle of the path, neither free nor taken.
        let e = resolve(&spec(Some(&plain.join("shot.png")), None)).unwrap_err();
        assert_eq!(e.code, ExitCode::Io);
        assert!(!e.message.contains("already exists"), "{}", e.message);
    }

    #[test]
    fn existing_file_needs_force() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"x").unwrap();
        let e = resolve(&spec(Some(dir.path()), Some("a"))).unwrap_err();
        assert_eq!(e.code, ExitCode::Io);
        let forced = TargetSpec {
            force: true,
            ..spec(Some(dir.path()), Some("a"))
        };
        assert_eq!(
            resolve(&forced).unwrap(),
            Target::File(dir.path().join("a.png"))
        );
    }

    #[test]
    fn counter_skips_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s-1.png"), b"x").unwrap();
        let got = resolve(&spec(Some(dir.path()), Some("s-{n}"))).unwrap();
        assert_eq!(got, Target::File(dir.path().join("s-2.png")));
    }

    #[test]
    fn values_known_only_after_the_capture_are_deferred() {
        let dir = tempfile::tempdir().unwrap();
        let got = resolve(&spec(Some(dir.path()), Some("shot-{w}x{h}"))).unwrap();
        assert_eq!(got, Target::Deferred(dir.path().to_owned()));
        let primary = TargetSpec {
            mode: "output",
            ..spec(Some(dir.path()), Some("{output}"))
        };
        assert!(matches!(resolve(&primary).unwrap(), Target::Deferred(_)));
        // A named screen is known now; `screen` has no output name at all.
        let named = TargetSpec {
            connector: Some("DP-1"),
            ..primary
        };
        assert_eq!(
            resolve(&named).unwrap(),
            Target::File(dir.path().join("DP-1.png"))
        );
        let e = resolve(&spec(Some(dir.path()), Some("{output}"))).unwrap_err();
        assert_eq!(e.code, ExitCode::Usage);
    }

    #[test]
    fn deferred_names_get_their_value_from_the_image_size() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec(Some(dir.path()), Some("shot-{w}x{h}"));
        let got = super::resolve_final(
            &spec,
            &Config::default(),
            &clock(),
            Some("laptop"),
            &loc(),
            (1920, 1080),
        )
        .unwrap();
        assert_eq!(got, dir.path().join("shot-1920x1080.png"));
    }

    #[test]
    fn bad_names_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../x", "{nope}", "a/b", "{date"] {
            let e = resolve(&spec(Some(dir.path()), Some(name))).unwrap_err();
            assert_eq!(e.code, ExitCode::Usage, "{name}");
        }
        let file = dir.path().join("plain");
        std::fs::write(&file, b"x").unwrap();
        let e = resolve(&spec(Some(&file), None)).unwrap_err();
        assert_eq!(e.code, ExitCode::Io);
        let _: PathBuf = file;
    }

    #[test]
    fn config_template_and_folder_are_used() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            output_dir: Some(dir.path().to_owned()),
            name_template: "cfg-{mode}".into(),
            ..Config::default()
        };
        let got = resolve_target(&spec(None, None), &config, &clock(), None, &loc()).unwrap();
        assert_eq!(got, Target::File(dir.path().join("cfg-screen.png")));
    }
}
