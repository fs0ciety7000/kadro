// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wayland clipboard, through `ext-data-control-v1` or `zwlr_data_control_v1` (KDE, wlroots).
//!
//! A Wayland selection is a promise to serve the data on request: it disappears with the process
//! that made it. The `kropa` command exits right after the copy, so [`WaylandClipboard`] starts a
//! **helper**, a second `kropa` run with the hidden argument [`CLIPBOARD_SERVE_ARGUMENT`] (the
//! approach of `wl-copy`, with no daemon and no `fork`, which needs `unsafe`). The helper gets the
//! content on its stdin, sets the selection, **reads it back** through a second Wayland connection to
//! prove the compositor holds it, and only then answers `ok` on its stdout. It then keeps serving
//! until another client takes the selection, and exits.
//!
//! What the helper inherits is on purpose small: its own process group, `/` as working directory,
//! `/dev/null` as stderr, our pipes as stdin and stdout (so a shell that captures `$(kropa ...)` is
//! not kept waiting by the helper), and only `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` from the
//! environment. Descriptors above 2 that the caller left open without `CLOEXEC` are inherited (closing
//! them needs `unsafe`); `kropa` itself creates none.
//!
//! Without `WAYLAND_DISPLAY` nothing is started and the outcome is `unavailable`. A compositor with
//! no data-control protocol (GNOME) is told apart by the helper, which gets `MissingProtocol` from the
//! library: the outcome is `unavailable` as well, never `copied`.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use wl_clipboard_rs::copy::{self, MimeSource, Options, Source};
use wl_clipboard_rs::paste;

use super::protocol::{
    Kind, MAX_PAYLOAD, Request, RequestError, format_answer, parse_answer, read_request,
    write_request,
};
use super::{Clipboard, ClipboardOutcome, ClipboardReason, TextKind};
use crate::FactValue;

/// The hidden argument of `kropa` that makes it the helper (like `__store-broker` on macOS). It is
/// not a command: it reads a request on stdin and refuses to run on a terminal.
pub const CLIPBOARD_SERVE_ARGUMENT: &str = "__clipboard-serve";

/// How long the helper has to set and confirm the selection.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// MIME type of an image.
const PNG_MIME: &str = "image/png";

/// What the session says about where the clipboard is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// `WAYLAND_DISPLAY`, `None` when unset or empty.
    pub wayland_display: Option<OsString>,
    /// `XDG_RUNTIME_DIR`, where the Wayland socket lives.
    pub runtime_dir: Option<OsString>,
    /// Whether `DISPLAY` (X11) is set.
    pub x11_display: bool,
}

impl Environment {
    /// Reads the three variables from the process.
    #[must_use]
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        Self {
            wayland_display: var("WAYLAND_DISPLAY"),
            runtime_dir: var("XDG_RUNTIME_DIR"),
            x11_display: var("DISPLAY").is_some(),
        }
    }
}

/// The program that serves the selection.
#[derive(Debug, Clone)]
struct Helper {
    program: PathBuf,
    args: Vec<OsString>,
}

/// The clipboard of a Wayland session with data-control.
#[derive(Debug, Clone)]
pub struct WaylandClipboard {
    /// `None` when the path of the running program is unknown.
    helper: Option<Helper>,
    env: Environment,
    timeout: Duration,
}

impl WaylandClipboard {
    /// The clipboard of this process: the helper is this very program, the session is read from the
    /// environment.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            helper: std::env::current_exe().ok().map(|program| Helper {
                program,
                args: vec![OsString::from(CLIPBOARD_SERVE_ARGUMENT)],
            }),
            env: Environment::from_process(),
            timeout: ANSWER_TIMEOUT,
        }
    }

    /// A clipboard that starts `program` with `args` as its helper, in the session `env`.
    #[must_use]
    pub fn new(program: PathBuf, args: Vec<OsString>, env: Environment) -> Self {
        Self {
            helper: Some(Helper { program, args }),
            env,
            timeout: ANSWER_TIMEOUT,
        }
    }

    /// Waits `timeout` for the helper's answer instead of the default.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn copy(&self, kind: Kind, data: &[u8]) -> ClipboardOutcome {
        if let Some(outcome) = precheck(&self.env) {
            return outcome;
        }
        if data.len() > MAX_PAYLOAD {
            return ClipboardOutcome::failed(
                ClipboardReason::TooLarge,
                format!("{} bytes", data.len()),
            );
        }
        let Some(helper) = &self.helper else {
            return ClipboardOutcome::failed(
                ClipboardReason::HelperSpawnFailed,
                "the path of the running program is unknown",
            );
        };
        let mut child = match self.spawn(helper) {
            Ok(child) => child,
            Err(e) => {
                return ClipboardOutcome::failed(ClipboardReason::HelperSpawnFailed, e.to_string());
            }
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            stop(child);
            return ClipboardOutcome::failed(ClipboardReason::HelperSpawnFailed, "no pipes");
        };
        // A helper that has already refused closes its stdin: the answer it wrote says why.
        let sent = write_request(&mut stdin, kind, data);
        drop(stdin);
        match read_answer(stdout, self.timeout) {
            Ok(line) if !line.is_empty() => {
                let outcome = parse_answer(&line);
                settle(child, &outcome);
                outcome
            }
            Ok(_) => {
                let how = sent.err().map_or_else(String::new, |e| format!(" ({e})"));
                let status = child
                    .wait()
                    .map_or_else(|e| e.to_string(), |s| s.to_string());
                ClipboardOutcome::failed(
                    ClipboardReason::HelperFailed,
                    format!("the helper ended without answering: {status}{how}"),
                )
            }
            Err(()) => {
                stop(child);
                ClipboardOutcome::failed(
                    ClipboardReason::Timeout,
                    format!("no answer in {} s", self.timeout.as_secs_f32()),
                )
            }
        }
    }

    fn spawn(&self, helper: &Helper) -> io::Result<Child> {
        let mut command = Command::new(&helper.program);
        command
            .args(&helper.args)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // Its own process group: a Ctrl-C aimed at the command does not kill the helper.
            .process_group(0);
        if let Some(display) = &self.env.wayland_display {
            command.env("WAYLAND_DISPLAY", display);
        }
        if let Some(dir) = &self.env.runtime_dir {
            command.env("XDG_RUNTIME_DIR", dir);
        }
        command.spawn()
    }
}

/// What can be told without asking the compositor: no Wayland display means no data-control.
fn precheck(env: &Environment) -> Option<ClipboardOutcome> {
    if env.wayland_display.is_some() {
        return None;
    }
    let reason = if env.x11_display {
        ClipboardReason::X11NotSupported
    } else {
        ClipboardReason::NoGraphicalSession
    };
    Some(ClipboardOutcome::unavailable(reason))
}

/// Reads the helper's answer line; `Err` when it did not come in time.
fn read_answer(stdout: impl Read + Send + 'static, timeout: Duration) -> Result<String, ()> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        // One short line; a helper that writes more is not read to the end.
        let _ = BufReader::new(stdout).take(1024).read_line(&mut line);
        let _ = tx.send(line);
    });
    match rx.recv_timeout(timeout) {
        Ok(line) => Ok(line),
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => Err(()),
    }
}

/// Ends a helper that did not do its job, and reaps it.
fn stop(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// After an answer: a helper that serves the selection is left running and reaped when it ends (a
/// thread waits for it, so that a long-lived caller leaves no zombie); any other is stopped.
fn settle(mut child: Child, outcome: &ClipboardOutcome) {
    if *outcome == ClipboardOutcome::Copied {
        thread::spawn(move || {
            let _ = child.wait();
        });
    } else {
        stop(child);
    }
}

impl Clipboard for WaylandClipboard {
    fn copy_text(&self, text: &str, kind: TextKind) -> ClipboardOutcome {
        let kind = match kind {
            TextKind::Plain => Kind::Text,
            TextKind::Html => Kind::Html,
        };
        self.copy(kind, text.as_bytes())
    }

    fn copy_image(&self, png: &[u8]) -> ClipboardOutcome {
        self.copy(Kind::Png, png)
    }
}

/// The outcome for an error of the copy of `wl-clipboard-rs`.
fn classify(error: &copy::Error) -> ClipboardOutcome {
    use copy::Error;
    match error {
        Error::MissingProtocol { .. } => {
            ClipboardOutcome::unavailable(ClipboardReason::NoDataControl)
        }
        Error::NoSeats | Error::SeatNotFound => {
            ClipboardOutcome::failed(ClipboardReason::NoSeat, error.to_string())
        }
        Error::SocketOpenError(_) | Error::WaylandConnection(_) => {
            ClipboardOutcome::failed(ClipboardReason::ConnectionFailed, error.to_string())
        }
        _ => ClipboardOutcome::failed(ClipboardReason::CompositorError, error.to_string()),
    }
}

/// Answers once on stdout, whichever thread gets there first.
#[derive(Default)]
struct Reporter {
    done: Mutex<bool>,
}

impl Reporter {
    fn report(&self, outcome: &ClipboardOutcome) {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        if !*done {
            *done = true;
            let mut out = io::stdout().lock();
            // The caller may already be gone: nothing to do about it.
            let _ = writeln!(out, "{}", format_answer(outcome));
            let _ = out.flush();
        }
    }
}

/// The sources to offer for a request, how to read the main one back, and the bytes to compare.
fn sources(request: Request) -> (Vec<MimeSource>, paste::MimeType<'static>, Vec<u8>) {
    let Request { kind, data } = request;
    let offer = |data: &[u8], mime_type| MimeSource {
        source: Source::Bytes(data.into()),
        mime_type,
    };
    match kind {
        Kind::Text => (
            vec![offer(&data, copy::MimeType::Text)],
            paste::MimeType::Text,
            data,
        ),
        Kind::Html => (
            vec![
                offer(&data, copy::MimeType::Specific("text/html".to_owned())),
                offer(&data, copy::MimeType::Text),
            ],
            paste::MimeType::Specific("text/html"),
            data,
        ),
        Kind::Png => (
            vec![offer(&data, copy::MimeType::Specific(PNG_MIME.to_owned()))],
            paste::MimeType::Specific(PNG_MIME),
            data,
        ),
    }
}

/// Reads the selection back and compares it with what was offered.
fn verify(mime: paste::MimeType<'_>, expected: &[u8]) -> ClipboardOutcome {
    let mismatch =
        |detail: String| ClipboardOutcome::failed(ClipboardReason::SelectionMismatch, detail);
    match paste::get_contents(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        mime,
    ) {
        Ok((reader, _)) => {
            let mut got = Vec::with_capacity(expected.len());
            let limit = u64::try_from(expected.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            match reader.take(limit).read_to_end(&mut got) {
                Ok(_) if got == expected => ClipboardOutcome::Copied,
                Ok(_) => mismatch(format!(
                    "read back {} bytes, expected {}",
                    got.len(),
                    expected.len()
                )),
                Err(e) => mismatch(format!("read back failed: {e}")),
            }
        }
        Err(e) => mismatch(format!("read back failed: {e}")),
    }
}

/// The body of the helper (`kropa __clipboard-serve`): request on stdin, one answer line on stdout,
/// then serves the selection until another client takes it. The exit code is 0 unless the helper
/// failed to start serving.
#[must_use]
pub fn run_helper() -> ExitCode {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        // Typed by hand: not an error to explain, only something that does not wait for input.
        return ExitCode::from(2);
    }
    let reporter = Arc::new(Reporter::default());
    let request = match read_request(&mut stdin.lock()) {
        Ok(request) => request,
        Err(e) => {
            let reason = if matches!(e, RequestError::TooLarge) {
                ClipboardReason::TooLarge
            } else {
                ClipboardReason::HelperFailed
            };
            reporter.report(&ClipboardOutcome::failed(reason, e.to_string()));
            return ExitCode::from(2);
        }
    };
    let (offers, read_back, data) = sources(request);
    let mut options = Options::new();
    options.foreground(true);
    let prepared = match options.prepare_copy_multi(offers) {
        Ok(prepared) => prepared,
        Err(e) => {
            reporter.report(&classify(&e));
            return ExitCode::from(1);
        }
    };
    // The selection is set. The proof that the compositor holds it needs a second client, which asks
    // for the data while this thread serves it.
    let checker = Arc::clone(&reporter);
    thread::spawn(move || {
        let outcome = verify(read_back, &data);
        checker.report(&outcome);
        if outcome != ClipboardOutcome::Copied {
            std::process::exit(1);
        }
    });
    // Returns when the selection is taken by another client. Before the read-back finished, that is a
    // failure; after the answer, the normal end of the helper (the report is then ignored).
    match prepared.serve() {
        Ok(()) => reporter.report(&ClipboardOutcome::failed(
            ClipboardReason::SelectionMismatch,
            "the selection was replaced before it could be read back",
        )),
        Err(e) => reporter.report(&classify(&e)),
    }
    ExitCode::SUCCESS
}

/// What the session says about data-control, for `kropa doctor`: `true` when the compositor offers
/// `ext-data-control-v1` or `zwlr_data_control_v1`, `Absent` when it does not (GNOME) or when there is
/// no Wayland session, a text when the compositor could not be asked. Reads only the MIME types
/// currently offered, never the content.
#[must_use]
pub fn probe_data_control() -> FactValue {
    probe_with(&Environment::from_process(), || {
        paste::get_mime_types(paste::ClipboardType::Regular, paste::Seat::Unspecified).map(|_| ())
    })
}

fn probe_with(env: &Environment, ask: impl FnOnce() -> Result<(), paste::Error>) -> FactValue {
    use paste::Error;
    if env.wayland_display.is_none() {
        return FactValue::Absent;
    }
    match ask() {
        // An empty clipboard still proves the protocol is there.
        Ok(()) | Err(Error::ClipboardEmpty | Error::NoMimeType) => FactValue::Bool(true),
        Err(Error::MissingProtocol { .. }) => FactValue::Absent,
        Err(e) => FactValue::Text(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;

    use wl_clipboard_rs::{copy, paste};

    use super::{
        Environment, Kind, Request, WaylandClipboard, classify, precheck, probe_with, sources,
    };
    use crate::FactValue;
    use crate::clipboard::{Clipboard, ClipboardOutcome, ClipboardReason, TextKind};

    fn wayland() -> Environment {
        Environment {
            wayland_display: Some("wayland-test".into()),
            runtime_dir: Some("/nonexistent".into()),
            x11_display: false,
        }
    }

    /// A clipboard whose helper is `/bin/sh -c script`: the script plays the helper, to test what
    /// the caller does with each thing a helper can do.
    fn with_script(script: &str) -> WaylandClipboard {
        WaylandClipboard::new(
            PathBuf::from("/bin/sh"),
            vec![OsString::from("-c"), OsString::from(script)],
            wayland(),
        )
        .with_timeout(Duration::from_millis(1500))
    }

    #[test]
    fn without_a_wayland_display_nothing_is_tried() {
        let none = Environment::default();
        assert_eq!(
            precheck(&none),
            Some(ClipboardOutcome::unavailable(
                ClipboardReason::NoGraphicalSession
            ))
        );
        let x11 = Environment {
            x11_display: true,
            ..none
        };
        assert_eq!(
            precheck(&x11),
            Some(ClipboardOutcome::unavailable(
                ClipboardReason::X11NotSupported
            ))
        );
        assert_eq!(precheck(&wayland()), None);
        // The helper is `/nonexistent`: reaching it would be a failure, not an `unavailable`.
        let clipboard = WaylandClipboard::new("/nonexistent".into(), Vec::new(), x11);
        assert_eq!(
            clipboard.copy_text("x", TextKind::Plain).status(),
            "unavailable"
        );
    }

    #[test]
    fn a_compositor_without_data_control_is_unavailable_not_failed() {
        let missing = copy::Error::MissingProtocol {
            name: "ext-data-control, or wlr-data-control",
            version: 1,
        };
        assert_eq!(
            classify(&missing),
            ClipboardOutcome::unavailable(ClipboardReason::NoDataControl)
        );
        assert_eq!(
            classify(&copy::Error::NoSeats).reason(),
            Some(ClipboardReason::NoSeat)
        );
        let refused = copy::Error::SocketOpenError(std::io::Error::other("refused"));
        assert_eq!(
            classify(&refused).reason(),
            Some(ClipboardReason::ConnectionFailed)
        );
        assert_eq!(
            classify(&copy::Error::PrimarySelectionUnsupported).status(),
            "failed"
        );
    }

    #[test]
    fn the_probe_tells_present_absent_and_unreachable_apart() {
        let missing = || {
            Err(paste::Error::MissingProtocol {
                name: "ext-data-control, or wlr-data-control",
                version: 1,
            })
        };
        assert_eq!(probe_with(&wayland(), || Ok(())), FactValue::Bool(true));
        assert_eq!(
            probe_with(&wayland(), || Err(paste::Error::ClipboardEmpty)),
            FactValue::Bool(true)
        );
        assert_eq!(probe_with(&wayland(), missing), FactValue::Absent);
        assert!(matches!(
            probe_with(&wayland(), || Err(paste::Error::NoSeats)),
            FactValue::Text(_)
        ));
        // No Wayland session: the compositor is not even asked.
        assert_eq!(
            probe_with(&Environment::default(), || panic!("asked")),
            FactValue::Absent
        );
    }

    #[test]
    fn html_is_offered_as_html_and_as_text_and_the_image_as_png() {
        let (offers, _, _) = sources(Request {
            kind: Kind::Html,
            data: b"<a>".to_vec(),
        });
        let mimes: Vec<_> = offers.into_iter().map(|o| o.mime_type).collect();
        assert_eq!(
            mimes,
            [
                copy::MimeType::Specific("text/html".into()),
                copy::MimeType::Text
            ]
        );
        let (offers, _, data) = sources(Request {
            kind: Kind::Png,
            data: b"png".to_vec(),
        });
        assert_eq!(offers.len(), 1);
        assert_eq!(
            offers[0].mime_type,
            copy::MimeType::Specific("image/png".into())
        );
        assert_eq!(data, b"png");
    }

    #[test]
    fn what_a_helper_answers_is_what_the_caller_reports() {
        let ok = with_script("read l; echo ok; exec sleep 0.1");
        assert_eq!(ok.copy_text("x", TextKind::Plain), ClipboardOutcome::Copied);
        let none = with_script("echo 'unavailable no_data_control'");
        assert_eq!(
            none.copy_image(b"x"),
            ClipboardOutcome::unavailable(ClipboardReason::NoDataControl)
        );
        let failed = with_script("echo 'failed no_seat there is no seat'");
        assert_eq!(
            failed.copy_text("x", TextKind::Plain),
            ClipboardOutcome::failed(ClipboardReason::NoSeat, "there is no seat")
        );
    }

    #[test]
    fn a_helper_that_dies_or_babbles_never_gives_copied() {
        for script in ["exit 3", "echo copied", "echo", "echo ok ok"] {
            let outcome = with_script(script).copy_text("x", TextKind::Plain);
            assert_eq!(outcome.status(), "failed", "{script}");
            assert_eq!(
                outcome.reason(),
                Some(ClipboardReason::HelperFailed),
                "{script}"
            );
        }
    }

    #[test]
    fn a_helper_that_does_not_answer_is_a_timeout_and_is_stopped() {
        let clipboard = with_script("exec sleep 30").with_timeout(Duration::from_millis(200));
        let started = std::time::Instant::now();
        let outcome = clipboard.copy_text("x", TextKind::Plain);
        assert_eq!(outcome.reason(), Some(ClipboardReason::Timeout));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_helper_is_a_spawn_failure() {
        let clipboard = WaylandClipboard::new("/nonexistent/kropa".into(), Vec::new(), wayland());
        assert_eq!(
            clipboard.copy_image(b"x").reason(),
            Some(ClipboardReason::HelperSpawnFailed)
        );
    }

    #[test]
    fn a_huge_content_is_refused_before_anything_is_started() {
        let clipboard = WaylandClipboard::new("/nonexistent/kropa".into(), Vec::new(), wayland());
        let huge = vec![0; super::MAX_PAYLOAD + 1];
        assert_eq!(
            clipboard.copy_image(&huge).reason(),
            Some(ClipboardReason::TooLarge)
        );
    }

    #[test]
    fn the_helper_gets_the_request_on_stdin_and_a_clean_environment() {
        // The script answers `ok` only if the first line it reads is the header of the protocol, and
        // the environment holds the Wayland display and nothing from the caller.
        let script = r#"read l
[ "$l" = "kropa-clipboard 1" ] || { echo "failed helper_failed header"; exit 0; }
[ -z "$HOME" ] || { echo "failed helper_failed home"; exit 0; }
[ "$WAYLAND_DISPLAY" = wayland-test ] || { echo "failed helper_failed display"; exit 0; }
echo ok"#;
        assert_eq!(
            with_script(script).copy_text("x", TextKind::Plain),
            ClipboardOutcome::Copied
        );
    }
}
