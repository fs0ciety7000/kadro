// SPDX-License-Identifier: GPL-3.0-or-later

//! The clipboard step of `upload` and `capture`: what `--copy` asked for, what the system really did.
//!
//! The status in the JSON is the outcome of [`Clipboard`], nothing else: `copied` only after the
//! platform confirmed the content is in the clipboard. A copy that cannot be done is a warning on
//! stderr and a status in the JSON, never an error: the exit code stays 0 whether `--copy` was given
//! or not (`docs/cli.md` section 5, decision of 11/11/2026), and stdout keeps the raw URL or path.

use kropa_capture::{Clipboard, ClipboardOutcome, ClipboardReason, TextKind};
use kropa_i18n::Localizer;
use serde_json::{Value, json};

use crate::upload::CopyChoice;

/// What there is to put in the clipboard.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    /// Nothing was asked for.
    Nothing,
    /// Something was asked for and there is nothing to copy (a link format without an upload).
    Missing,
    /// A link, formatted as asked.
    Link(&'a str),
    /// The captured image, as PNG.
    Image(&'a [u8]),
}

/// The result of the step.
#[derive(Debug, Clone)]
pub struct Step {
    /// The `clipboard` member of the JSON documents.
    pub json: Value,
    /// The warning to add to `warnings` and to show on stderr, when the copy did not happen.
    pub warning: Option<Value>,
    /// The note to show on stderr when it did.
    pub note: Option<String>,
}

/// Fluent id of the text that explains a reason.
fn reason_id(code: &str) -> String {
    format!("clipboard-reason-{}", code.replace('_', "-"))
}

/// Does the copy `requested` asks for (`explicit`: given on the command line) and reports it.
#[must_use]
pub fn run(
    clipboard: &dyn Clipboard,
    loc: &Localizer,
    requested: CopyChoice,
    explicit: bool,
    content: Content<'_>,
) -> Step {
    let outcome = match content {
        Content::Nothing | Content::Missing => None,
        Content::Link(text) => Some(clipboard.copy_text(text, TextKind::Plain)),
        Content::Image(png) => Some(clipboard.copy_image(png)),
    };
    let (status, reason) = match &outcome {
        Some(o) => (o.status(), o.reason().map(ClipboardReason::as_str)),
        None => (
            "skipped",
            matches!(content, Content::Missing).then_some("nothing_to_copy"),
        ),
    };
    let json = json!({
        "requested": requested.as_str(),
        "explicit": explicit,
        "status": status,
        "reason": reason,
    });
    let what = if requested == CopyChoice::Image {
        "image"
    } else {
        "link"
    };
    let (warning, note) = match &outcome {
        None => (None, None),
        Some(ClipboardOutcome::Copied) => (
            None,
            Some(loc.message_args(
                "note-clipboard-copied",
                &[("what", what), ("format", requested.as_str())],
            )),
        ),
        Some(failed) => (Some(warning(failed, what, loc)), None),
    };
    Step {
        json,
        warning,
        note,
    }
}

/// The `{code, message}` warning for an outcome that is not `Copied`.
fn warning(outcome: &ClipboardOutcome, what: &str, loc: &Localizer) -> Value {
    let reason = outcome.reason().map_or("", |r| r.as_str());
    let why = loc.message(&reason_id(reason));
    let (code, message) = if outcome.status() == "unavailable" {
        (
            "clipboard_unavailable",
            loc.message_args(
                "warn-clipboard-unavailable",
                &[("what", what), ("reason", &why)],
            ),
        )
    } else if let Some(detail) = outcome.detail() {
        (
            "clipboard_failed",
            loc.message_args(
                "warn-clipboard-failed",
                &[("what", what), ("reason", &why), ("detail", detail)],
            ),
        )
    } else {
        (
            "clipboard_failed",
            loc.message_args(
                "warn-clipboard-failed-bare",
                &[("what", what), ("reason", &why)],
            ),
        )
    };
    json!({ "code": code, "message": message })
}

#[cfg(test)]
mod tests {
    use kropa_capture::{
        ALL_CLIPBOARD_REASONS, ClipboardOutcome, ClipboardReason, FakeClipboard, FakeCopy,
    };
    use kropa_i18n::{Lang, Localizer};

    use super::{Content, reason_id, run, warning};
    use crate::upload::CopyChoice;

    fn en() -> Localizer {
        Localizer::new(Lang::En).unwrap()
    }

    #[test]
    fn every_reason_is_explained_in_both_languages() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            for code in ALL_CLIPBOARD_REASONS {
                let id = reason_id(code);
                assert_ne!(loc.message(&id), id, "{lang:?}: {id}");
            }
            for outcome in [
                ClipboardOutcome::unavailable(ClipboardReason::NoDataControl),
                ClipboardOutcome::failed(ClipboardReason::Timeout, "detail"),
                ClipboardOutcome::failed(ClipboardReason::Timeout, ""),
            ] {
                for what in ["link", "image"] {
                    let w = warning(&outcome, what, &loc);
                    let message = w["message"].as_str().unwrap();
                    assert!(!message.starts_with("warn-"), "{lang:?}: {message}");
                }
            }
            for what in ["link", "image"] {
                let note = loc.message_args(
                    "note-clipboard-copied",
                    &[("what", what), ("format", "url")],
                );
                assert_ne!(note, "note-clipboard-copied");
            }
        }
    }

    #[test]
    fn a_copy_that_worked_says_so_and_nothing_more() {
        let fake = FakeClipboard::working();
        let step = run(
            &fake,
            &en(),
            CopyChoice::Markdown,
            true,
            Content::Link("![a](https://x/a.png)"),
        );
        assert_eq!(step.json["status"], "copied");
        assert_eq!(step.json["reason"], serde_json::Value::Null);
        assert_eq!(step.json["explicit"], true);
        assert_eq!(step.note.as_deref(), Some("Link copied (markdown)"));
        assert!(step.warning.is_none());
        assert_eq!(fake.copies().len(), 1);
    }

    #[test]
    fn an_image_is_copied_as_png() {
        let fake = FakeClipboard::working();
        let step = run(
            &fake,
            &en(),
            CopyChoice::Image,
            true,
            Content::Image(b"png"),
        );
        assert_eq!(step.note.as_deref(), Some("Image copied"));
        assert_eq!(fake.copies(), [FakeCopy::Image(b"png".to_vec())]);
    }

    #[test]
    fn gnome_gets_the_honest_warning_and_the_status_unavailable() {
        let fake = FakeClipboard::answering(ClipboardOutcome::unavailable(
            ClipboardReason::NoDataControl,
        ));
        let step = run(
            &fake,
            &en(),
            CopyChoice::Url,
            true,
            Content::Link("https://x"),
        );
        assert_eq!(step.json["status"], "unavailable");
        assert_eq!(step.json["reason"], "no_data_control");
        let warning = step.warning.unwrap();
        assert_eq!(warning["code"], "clipboard_unavailable");
        let message = warning["message"].as_str().unwrap();
        assert!(message.starts_with("Link not copied:"), "{message}");
        assert!(message.contains("without focus"), "{message}");
        assert!(message.ends_with("The link is printed above."), "{message}");
        assert!(step.note.is_none());
    }

    #[test]
    fn a_failure_keeps_its_technical_detail_for_stderr_only() {
        let fake = FakeClipboard::answering(ClipboardOutcome::failed(
            ClipboardReason::CompositorError,
            "protocol error 7",
        ));
        let step = run(&fake, &en(), CopyChoice::Image, false, Content::Image(b"p"));
        assert_eq!(step.json["status"], "failed");
        assert!(!step.json.to_string().contains("protocol error"));
        let warning = step.warning.unwrap();
        assert_eq!(warning["code"], "clipboard_failed");
        assert!(
            warning["message"]
                .as_str()
                .unwrap()
                .contains("protocol error 7")
        );
        assert!(
            warning["message"]
                .as_str()
                .unwrap()
                .starts_with("Image not copied:")
        );
    }

    #[test]
    fn nothing_asked_or_nothing_to_copy_never_touches_the_clipboard() {
        let fake = FakeClipboard::working();
        let none = run(&fake, &en(), CopyChoice::None, false, Content::Nothing);
        assert_eq!(none.json["status"], "skipped");
        assert_eq!(none.json["reason"], serde_json::Value::Null);
        let missing = run(&fake, &en(), CopyChoice::Url, true, Content::Missing);
        assert_eq!(missing.json["status"], "skipped");
        assert_eq!(missing.json["reason"], "nothing_to_copy");
        assert!(missing.warning.is_none());
        assert_eq!(fake.copies(), Vec::<FakeCopy>::new());
    }
}
