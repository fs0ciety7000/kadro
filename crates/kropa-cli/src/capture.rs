// SPDX-License-Identifier: GPL-3.0-or-later

//! `kropa capture screen|output`: ask the backend, write the file, report.
//!
//! The order is chosen so that the user is never asked for something that cannot work: what the
//! backend can never do (one screen, a format it does not produce) fails first; then the permission
//! the platform asks for up front; only then comes the delay, the portal dialog, the picture and the
//! file. Nothing is written when the capture failed, and the success message is only printed once
//! the file is on disk.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kropa_capture::{
    CaptureBackend, CaptureError, CaptureMode, CaptureRequest, CapturedImage, ImageFormat,
    Unsupported,
};
use kropa_core::{APP_ID, Config, ExitCode};
use kropa_i18n::Localizer;
use serde_json::{Value, json};

use crate::args::{CaptureOpts, CopyTarget};
use crate::errors::CliError;
use crate::exec::Context;
use crate::output::Session;
use crate::target::{Target, TargetSpec, resolve_final};

/// JSON schema of the result.
pub const SCHEMA: &str = "kropa.capture/1";

/// What `capture screen|output` was asked.
pub struct Job<'a> {
    /// All screens or one.
    pub mode: CaptureMode,
    /// Screen name given to `capture output`.
    pub connector: Option<&'a str>,
    /// Options of the command line.
    pub opts: &'a CaptureOpts,
}

/// Error id of Fluent for a capture failure: `error-capture-` and the reason with dashes.
fn error_id(reason: &str) -> String {
    format!("error-capture-{}", reason.replace('_', "-"))
}

/// The command-line error for a capture failure, with its exit code and `error.reason`.
#[must_use]
pub fn capture_cli_error(err: &CaptureError, loc: &Localizer, detail: Option<&str>) -> CliError {
    let detail = detail.or_else(|| err.detail()).unwrap_or_default();
    CliError::new(
        err.exit_code(),
        loc,
        &error_id(err.reason()),
        &[("detail", detail)],
    )
    .with_reason(err.reason())
    .at_step("capture")
}

fn format_of_extension(path: &Path) -> Option<ImageFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

/// The format the file will have: the extension of an explicit file name, else the configured one.
fn wanted_format(target: &Target, config: &Config) -> ImageFormat {
    match target {
        Target::File(path) => format_of_extension(path).unwrap_or(config.image_format),
        Target::Deferred(_) => config.image_format,
    }
}

fn unsupported_format(format: ImageFormat, loc: &Localizer) -> CliError {
    capture_cli_error(
        &CaptureError::Unsupported(Unsupported::ImageFormat),
        loc,
        Some(format.extension()),
    )
}

fn write_error(path: &Path, error: &std::io::Error, loc: &Localizer) -> CliError {
    CliError::new(
        ExitCode::Io,
        loc,
        "error-write",
        &[
            ("path", &path.display().to_string()),
            ("detail", &error.to_string()),
        ],
    )
    .at_step("save")
}

fn exists_error(path: &Path, loc: &Localizer) -> CliError {
    CliError::new(
        ExitCode::Io,
        loc,
        "error-file-exists",
        &[("path", &path.display().to_string())],
    )
    .at_step("save")
}

/// Writes `data` to `path`. Without `force` the file must not exist (checked by the open itself,
/// so a race cannot overwrite it); with `force` the new content replaces the old in one step.
fn write_file(path: &Path, data: &[u8], force: bool, loc: &Localizer) -> Result<(), CliError> {
    let fail = |e: &std::io::Error| write_error(path, e, loc);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| fail(&e))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    if !force {
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(exists_error(path, loc));
            }
            Err(e) => return Err(fail(&e)),
        };
        if let Err(e) = file.write_all(data).and_then(|()| file.flush()) {
            drop(file);
            // A half-written screenshot is worse than none.
            let _ = std::fs::remove_file(path);
            return Err(fail(&e));
        }
        return Ok(());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let temporary = path.with_file_name(format!(
        ".{}.{}.tmp",
        name.unwrap_or_default(),
        std::process::id()
    ));
    let written = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(data).and_then(|()| file.flush()))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(fail(&e));
    }
    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}

/// Fluent id of the note about how the image was obtained.
fn note_id(note: &str) -> String {
    format!("note-{}", note.replace('_', "-"))
}

fn copy_name(copy: CopyTarget) -> &'static str {
    match copy {
        CopyTarget::Url => "url",
        CopyTarget::Markdown => "markdown",
        CopyTarget::Html => "html",
        CopyTarget::Image => "image",
        CopyTarget::None => "none",
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Webp => "webp",
    }
}

fn document(
    job: &Job<'_>,
    image: &CapturedImage,
    path: &Path,
    size: u64,
    captured_at: &str,
) -> Value {
    let send = &job.opts.send;
    let upload = if send.no_upload {
        Value::Null
    } else {
        json!({
            "status": "skipped", "destination": null, "job_id": null, "error": null,
            "url": null, "thumbnail_url": null, "deletion_url": null,
        })
    };
    // `--copy image` never gets here (lot 1.10); a link format has no link to copy without an upload.
    let (requested, explicit, reason) = match send.copy {
        Some(copy) => (
            copy_name(copy),
            true,
            (copy != CopyTarget::None).then_some("nothing_to_copy"),
        ),
        None => ("none", false, None),
    };
    json!({
        "schema": SCHEMA,
        "status": "ok",
        "exit_code": 0,
        "capture": {
            "mode": job.mode.as_str(),
            "output": image.output,
            "scale": image.scale,
            "backend": image.backend.as_str(),
            "region": null,
            "captured_at": captured_at,
        },
        "file": {
            "path": path.display().to_string(),
            "format": format_name(image.format),
            "width": image.width,
            "height": image.height,
            "size": size,
        },
        "upload": upload,
        "link": null,
        "clipboard": {
            "requested": requested, "explicit": explicit, "status": "skipped", "reason": reason,
        },
        "warnings": [],
        "error": null,
    })
}

/// Runs the capture and reports on `session`.
///
/// # Errors
/// The capture, naming or saving failure, with its exit code.
pub fn run(
    job: &Job<'_>,
    backend: &dyn CaptureBackend,
    config: &Config,
    target: &Target,
    ctx: &Context<'_>,
    session: &mut Session<'_, '_>,
) -> Result<ExitCode, CliError> {
    let loc = ctx.loc;
    let opts = job.opts;
    let wanted = wanted_format(target, config);
    if !backend.formats().contains(&wanted) {
        return Err(unsupported_format(wanted, loc));
    }
    let request = CaptureRequest {
        mode: job.mode,
        output: job.connector,
    };
    backend
        .check(&request)
        .map_err(|e| capture_cli_error(&e, loc, None))?;
    // A permission the platform asks for up front (macOS Screen Recording) is settled before the
    // delay, so that a refusal does not come after the wait.
    backend
        .authorize(&request)
        .map_err(|e| capture_cli_error(&e, loc, None))?;
    if let Some(seconds) = opts.delay.filter(|s| *s > 0.0) {
        session.debug(
            1,
            &loc.message_args("note-delay", &[("seconds", &seconds.to_string())]),
        );
        if let Ok(wait) = Duration::try_from_secs_f64(seconds) {
            (ctx.sleep)(wait);
        }
    }
    let image = backend
        .capture(&request)
        .map_err(|e| capture_cli_error(&e, loc, None))?;
    if image.format != wanted {
        return Err(unsupported_format(wanted, loc));
    }
    let path = match target {
        Target::File(path) => path.clone(),
        Target::Deferred(_) => {
            let spec = TargetSpec {
                mode: job.mode.as_str(),
                connector: job.connector,
                output: opts.output.as_deref(),
                name: opts.name.as_deref(),
                force: opts.force,
            };
            resolve_final(
                &spec,
                config,
                ctx.clock,
                ctx.host.as_deref(),
                loc,
                (image.width, image.height),
            )?
        }
    };
    let captured_at = ctx.clock.now();
    write_file(&path, &image.data, opts.force, loc)?;
    let path = absolute(&path);
    for note in &image.notes {
        session.debug(1, &loc.message_args(&note_id(note), &[("app_id", APP_ID)]));
    }
    session.note(&loc.message_args(
        "note-saved",
        &[
            ("path", &path.display().to_string()),
            ("width", &image.width.to_string()),
            ("height", &image.height.to_string()),
        ],
    ));
    if session.json {
        let at = captured_at.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let size = image.data.len() as u64;
        session.document(&document(job, &image, &path, size, &at));
    } else {
        session.result(&path.display().to_string());
    }
    Ok(ExitCode::Ok)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::time::Duration;

    use kropa_capture::{
        ALL_ERROR_REASONS, ALL_NOTES, BackendId, Capabilities, Capability, CaptureBackend,
        CaptureError, CaptureMode, CaptureRequest, CapturedImage, Denial, Diagnosis, Failure,
        ImageFormat, Status, Unsupported,
    };
    use kropa_core::{Config, ExitCode, FixedClock};
    use kropa_i18n::{Lang, Localizer};

    use super::{Job, error_id, note_id, run, write_file};
    use crate::args::CaptureOpts;
    use crate::exec::Context;
    use crate::output::{Io, Session};
    use crate::target::Target;

    /// A backend that answers what the test tells it to, and remembers what it was asked.
    struct Fake {
        answer: Result<CapturedImage, CaptureError>,
        /// What `authorize` answers (the permission asked for up front).
        authorization: Result<(), CaptureError>,
        calls: RefCell<u32>,
    }

    impl Fake {
        fn ok(width: u32, height: u32) -> Self {
            Self::with(Ok(image(width, height)))
        }

        fn with(answer: Result<CapturedImage, CaptureError>) -> Self {
            Self {
                answer,
                authorization: Ok(()),
                calls: RefCell::new(0),
            }
        }

        fn unauthorized(error: CaptureError) -> Self {
            Self {
                authorization: Err(error),
                ..Self::ok(1, 1)
            }
        }
    }

    fn image(width: u32, height: u32) -> CapturedImage {
        CapturedImage {
            data: b"png bytes".to_vec(),
            format: ImageFormat::Png,
            width,
            height,
            backend: BackendId::PortalScreenshot,
            output: None,
            scale: None,
            notes: vec!["app_id_registered"],
        }
    }

    impl CaptureBackend for Fake {
        fn id(&self) -> BackendId {
            BackendId::PortalScreenshot
        }
        fn formats(&self) -> &'static [ImageFormat] {
            &[ImageFormat::Png]
        }
        fn diagnose(&self) -> Diagnosis {
            let cap = |feature| Capability {
                feature,
                status: Status::Available,
                reason: None,
                fallback: None,
            };
            Diagnosis {
                platform: "linux",
                backend: "portal-screenshot",
                capabilities: Capabilities {
                    screen: cap("capture_screen"),
                    output: cap("capture_output"),
                },
                facts: Vec::new(),
            }
        }
        fn check(&self, request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
            if request.mode == CaptureMode::Output {
                return Err(CaptureError::Unsupported(Unsupported::OutputNotIsolable));
            }
            Ok(())
        }
        fn authorize(&self, _request: &CaptureRequest<'_>) -> Result<(), CaptureError> {
            self.authorization.clone()
        }
        fn capture(&self, request: &CaptureRequest<'_>) -> Result<CapturedImage, CaptureError> {
            self.check(request)?;
            *self.calls.borrow_mut() += 1;
            self.answer.clone()
        }
    }

    struct Out {
        result: Result<ExitCode, crate::errors::CliError>,
        stdout: String,
        stderr: String,
        slept: Vec<Duration>,
        calls: u32,
    }

    fn opts(extra: &[&str]) -> CaptureOpts {
        use clap::Parser;
        #[derive(Parser)]
        struct W {
            #[command(flatten)]
            opts: CaptureOpts,
        }
        let mut argv = vec!["w"];
        argv.extend_from_slice(extra);
        W::try_parse_from(argv).unwrap().opts
    }

    fn go(backend: &Fake, mode: CaptureMode, target: &Target, extra: &[&str], json: bool) -> Out {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let slept = RefCell::new(Vec::new());
        let loc = Localizer::new(Lang::En).unwrap();
        let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
        let sleep = |d: Duration| slept.borrow_mut().push(d);
        let result = {
            let mut io = Io {
                out: &mut stdout,
                err: &mut stderr,
                stdin_is_tty: false,
            };
            let mut session = Session::new(&mut io, json, false, 1, false);
            let ctx = Context {
                loc: &loc,
                clock: &clock,
                host: Some("laptop".into()),
                backend: Some(backend),
                sleep: &sleep,
            };
            let opts = opts(extra);
            let job = Job {
                mode,
                connector: None,
                opts: &opts,
            };
            run(
                &job,
                backend,
                &Config::default(),
                target,
                &ctx,
                &mut session,
            )
        };
        Out {
            result,
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
            slept: slept.into_inner(),
            calls: *backend.calls.borrow(),
        }
    }

    fn file_target(dir: &Path, name: &str) -> Target {
        Target::File(dir.join(name))
    }

    #[test]
    fn a_capture_is_written_and_its_path_printed() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "shot.png");
        let out = go(
            &Fake::ok(1920, 1080),
            CaptureMode::Screen,
            &target,
            &[],
            false,
        );
        assert_eq!(out.result.unwrap(), ExitCode::Ok);
        let path = dir.path().join("shot.png");
        assert_eq!(std::fs::read(&path).unwrap(), b"png bytes");
        assert_eq!(out.stdout, format!("{}\n", path.display()));
        assert!(out.stderr.contains("Saved:"), "{}", out.stderr);
        assert!(out.stderr.contains("1920x1080"), "{}", out.stderr);
        assert!(
            out.stderr.contains("Registered with the portal"),
            "{}",
            out.stderr
        );
    }

    #[test]
    fn json_result_describes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "shot.png");
        let out = go(
            &Fake::ok(640, 480),
            CaptureMode::Screen,
            &target,
            &["--no-upload"],
            true,
        );
        assert_eq!(out.result.unwrap(), ExitCode::Ok);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(out.stdout.lines().count(), 1);
        assert_eq!(v["schema"], "kropa.capture/1");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["capture"]["mode"], "screen");
        assert_eq!(v["capture"]["backend"], "portal-screenshot");
        assert_eq!(v["capture"]["captured_at"], "2026-11-11T14:03:22+01:00");
        assert_eq!(v["file"]["format"], "png");
        assert_eq!(v["file"]["width"], 640);
        assert_eq!(v["file"]["size"], 9);
        assert!(v["upload"].is_null(), "--no-upload");
        assert_eq!(v["clipboard"]["status"], "skipped");
        assert_eq!(v["error"], serde_json::Value::Null);
    }

    #[test]
    fn output_is_refused_before_the_delay_and_the_capture() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "o.png");
        let backend = Fake::ok(1, 1);
        let out = go(
            &backend,
            CaptureMode::Output,
            &target,
            &["--delay", "5"],
            false,
        );
        let err = out.result.unwrap_err();
        assert_eq!(err.code, ExitCode::Unsupported);
        assert_eq!(err.reason.as_deref(), Some("output_not_isolable"));
        assert_eq!(err.step, Some("capture"));
        assert!(
            err.message.contains("kropa capture screen"),
            "{}",
            err.message
        );
        assert_eq!(out.slept.len(), 0);
        assert_eq!(out.calls, 0);
        assert!(!dir.path().join("o.png").exists());
    }

    #[test]
    fn refusals_and_failures_map_to_their_exit_codes() {
        for (answer, code, reason) in [
            (
                CaptureError::PermissionDenied(Denial::PortalDeclined),
                ExitCode::PermissionDenied,
                "portal_declined",
            ),
            (
                CaptureError::failed(Failure::NoSessionBus, "no bus"),
                ExitCode::CaptureFailed,
                "no_session_bus",
            ),
            (
                CaptureError::failed(Failure::PortalError, "boom"),
                ExitCode::CaptureFailed,
                "portal_error",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let target = file_target(dir.path(), "x.png");
            let out = go(
                &Fake::with(Err(answer)),
                CaptureMode::Screen,
                &target,
                &[],
                false,
            );
            let err = out.result.unwrap_err();
            assert_eq!(err.code, code, "{reason}");
            assert_eq!(err.reason.as_deref(), Some(reason));
            assert!(!err.message.starts_with("error-capture"), "{}", err.message);
            assert_eq!(out.stdout, "");
            assert!(
                !dir.path().join("x.png").exists(),
                "no file after a failure"
            );
        }
    }

    #[test]
    fn a_missing_permission_is_reported_before_the_delay() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "x.png");
        let backend = Fake::unauthorized(CaptureError::PermissionDenied(
            Denial::ScreenRecordingDenied,
        ));
        let out = go(
            &backend,
            CaptureMode::Screen,
            &target,
            &["--delay", "30"],
            false,
        );
        let err = out.result.unwrap_err();
        assert_eq!(err.code, ExitCode::PermissionDenied);
        assert_eq!(err.reason.as_deref(), Some("screen_recording_denied"));
        assert_eq!(out.slept.len(), 0, "no wait for a capture that cannot happen");
        assert_eq!(out.calls, 0);
        assert!(!dir.path().join("x.png").exists());
    }

    #[test]
    fn the_failure_detail_reaches_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "x.png");
        let answer = CaptureError::failed(Failure::PortalError, "boom 42");
        let out = go(
            &Fake::with(Err(answer)),
            CaptureMode::Screen,
            &target,
            &[],
            false,
        );
        assert!(out.result.unwrap_err().message.contains("boom 42"));
    }

    #[test]
    fn the_delay_is_waited_before_the_capture() {
        let dir = tempfile::tempdir().unwrap();
        let target = file_target(dir.path(), "d.png");
        let out = go(
            &Fake::ok(2, 2),
            CaptureMode::Screen,
            &target,
            &["--delay", "2.5"],
            false,
        );
        assert_eq!(out.result.unwrap(), ExitCode::Ok);
        assert_eq!(out.slept, [Duration::from_millis(2500)]);
        let none = go(
            &Fake::ok(2, 2),
            CaptureMode::Screen,
            &file_target(dir.path(), "e.png"),
            &[],
            false,
        );
        assert_eq!(none.slept.len(), 0);
    }

    #[test]
    fn formats_the_backend_cannot_make_are_refused_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["x.jpg", "x.webp"] {
            let target = file_target(dir.path(), name);
            let backend = Fake::ok(1, 1);
            let out = go(&backend, CaptureMode::Screen, &target, &[], false);
            let err = out.result.unwrap_err();
            assert_eq!(err.code, ExitCode::Unsupported, "{name}");
            assert_eq!(err.reason.as_deref(), Some("image_format_unsupported"));
            assert_eq!(out.calls, 0, "no dialog for a capture that cannot be saved");
        }
    }

    #[test]
    fn a_name_with_the_image_size_is_resolved_after_the_capture() {
        let dir = tempfile::tempdir().unwrap();
        let target = Target::Deferred(dir.path().to_owned());
        let argv = [
            "--output",
            dir.path().to_str().unwrap(),
            "--name",
            "s-{w}x{h}",
        ];
        let out = go(
            &Fake::ok(300, 200),
            CaptureMode::Screen,
            &target,
            &argv,
            false,
        );
        assert_eq!(out.result.unwrap(), ExitCode::Ok);
        assert!(dir.path().join("s-300x200.png").is_file());
    }

    #[test]
    fn an_existing_file_is_kept_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Localizer::new(Lang::En).unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"old").unwrap();
        let err = write_file(&path, b"new", false, &loc).unwrap_err();
        assert_eq!(err.code, ExitCode::Io);
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        write_file(&path, b"new", true, &loc).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file is left");
    }

    #[test]
    fn missing_folders_are_created() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Localizer::new(Lang::En).unwrap();
        let path = dir.path().join("a/b/c.png");
        write_file(&path, b"x", false, &loc).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn an_unwritable_target_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Localizer::new(Lang::En).unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let err = write_file(&blocker.join("child.png"), b"x", false, &loc).unwrap_err();
        assert_eq!(err.code, ExitCode::Io);
        assert_eq!(err.step, Some("save"));
    }

    #[test]
    fn every_reason_and_note_has_a_message_in_both_languages() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for reason in ALL_ERROR_REASONS {
                let id = error_id(reason);
                assert_ne!(loc.message(&id), id, "{lang:?}: {id}");
            }
            for note in ALL_NOTES {
                let id = note_id(note);
                assert_ne!(loc.message(&id), id, "{lang:?}: {id}");
            }
        }
    }
}
