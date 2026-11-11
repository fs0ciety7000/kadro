// SPDX-License-Identifier: GPL-3.0-or-later

//! The clipboard of macOS: the general `NSPasteboard`.
//!
//! A copy is `clearContents`, then one `setString:forType:` / `setData:forType:` per type, then a
//! read back of each type through `stringForType:` / `dataForType:`. `Copied` is only answered when
//! every `set` returned `YES` and every read back equals what was written ("status = real effect").
//!
//! Threads: `NSPasteboard` is not main-thread-only (the generated binding is not `MainThreadOnly`)
//! and the general pasteboard is a client of the pasteboard server, so no `NSApplication` and no run
//! loop are needed (`pbcopy` has neither). The data is stored by the server at once: it outlives the
//! process, unlike a Wayland selection. The calls run in an autorelease pool because the
//! pasteboard returns autoreleased objects and this may run on a thread that has no pool.
//!
//! The types written are `public.utf8-plain-text` (`NSPasteboardTypeString`), `public.html`
//! (`NSPasteboardTypeHTML`) and `public.png` (`NSPasteboardTypePNG`); other applications convert
//! what they need (an `NSImage` reads `public.png`).

use objc2::rc::autoreleasepool;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardType, NSPasteboardTypeHTML, NSPasteboardTypePNG,
    NSPasteboardTypeString,
};
use objc2_foundation::{NSData, NSString};

use crate::clipboard::{Clipboard, ClipboardOutcome, ClipboardReason, TextKind};

/// `public.utf8-plain-text`.
fn plain_type() -> &'static NSPasteboardType {
    // SAFETY: an immutable `NSString` constant exported by AppKit, valid for the whole process;
    // AppKit is linked by `objc2-app-kit`.
    unsafe { NSPasteboardTypeString }
}

/// `public.html`.
fn html_type() -> &'static NSPasteboardType {
    // SAFETY: as `plain_type`, for `NSPasteboardTypeHTML`.
    unsafe { NSPasteboardTypeHTML }
}

/// `public.png`.
fn png_type() -> &'static NSPasteboardType {
    // SAFETY: as `plain_type`, for `NSPasteboardTypePNG`.
    unsafe { NSPasteboardTypePNG }
}

/// The general pasteboard of the user's session.
#[derive(Debug, Default, Clone, Copy)]
pub struct PasteboardClipboard;

impl PasteboardClipboard {
    /// The clipboard of the session. The pasteboard is only looked up when something is copied.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Clipboard for PasteboardClipboard {
    fn copy_text(&self, text: &str, kind: TextKind) -> ClipboardOutcome {
        autoreleasepool(|_| copy_text_to(&NSPasteboard::generalPasteboard(), text, kind))
    }

    fn copy_image(&self, png: &[u8]) -> ClipboardOutcome {
        autoreleasepool(|_| copy_image_to(&NSPasteboard::generalPasteboard(), png))
    }
}

/// Replaces the content of `pasteboard` by `text`: plain text, and for HTML the markup under
/// `public.html` as well, so that a rich-text field gets the markup and any other gets the source.
fn copy_text_to(pasteboard: &NSPasteboard, text: &str, kind: TextKind) -> ClipboardOutcome {
    pasteboard.clearContents();
    let ns_text = NSString::from_str(text);
    let mut written: Vec<(&'static NSPasteboardType, &'static str)> = Vec::with_capacity(2);
    if kind == TextKind::Html {
        written.push((html_type(), "public.html"));
    }
    written.push((plain_type(), "public.utf8-plain-text"));
    for (ty, name) in &written {
        if !pasteboard.setString_forType(&ns_text, ty) {
            return rejected(name);
        }
    }
    for (ty, name) in &written {
        if let Err(detail) = verify_string(pasteboard, ty, name, text) {
            return mismatch(detail);
        }
    }
    ClipboardOutcome::Copied
}

/// Replaces the content of `pasteboard` by the PNG `png`.
fn copy_image_to(pasteboard: &NSPasteboard, png: &[u8]) -> ClipboardOutcome {
    pasteboard.clearContents();
    let data = NSData::with_bytes(png);
    if !pasteboard.setData_forType(Some(&data), png_type()) {
        return rejected("public.png");
    }
    match verify_data(pasteboard, png_type(), "public.png", png) {
        Ok(()) => ClipboardOutcome::Copied,
        Err(detail) => mismatch(detail),
    }
}

/// Reads `ty` back as text and compares. The error never contains the content (it can be a link
/// with a token), only lengths.
fn verify_string(
    pasteboard: &NSPasteboard,
    ty: &NSPasteboardType,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    match pasteboard.stringForType(ty) {
        None => Err(format!("{name}: nothing to read back")),
        Some(read) => {
            let read = read.to_string();
            if read == expected {
                Ok(())
            } else {
                Err(format!(
                    "{name}: read back {} bytes, expected {}",
                    read.len(),
                    expected.len()
                ))
            }
        }
    }
}

/// Reads `ty` back as bytes and compares (lengths only in the error).
fn verify_data(
    pasteboard: &NSPasteboard,
    ty: &NSPasteboardType,
    name: &str,
    expected: &[u8],
) -> Result<(), String> {
    match pasteboard.dataForType(ty) {
        None => Err(format!("{name}: nothing to read back")),
        Some(read) => {
            let read = read.to_vec();
            if read == expected {
                Ok(())
            } else {
                Err(format!(
                    "{name}: read back {} bytes, expected {}",
                    read.len(),
                    expected.len()
                ))
            }
        }
    }
}

fn rejected(type_name: &str) -> ClipboardOutcome {
    ClipboardOutcome::failed(
        ClipboardReason::PasteboardRejected,
        format!("the pasteboard refused {type_name}"),
    )
}

fn mismatch(detail: String) -> ClipboardOutcome {
    ClipboardOutcome::failed(ClipboardReason::SelectionMismatch, detail)
}

#[cfg(test)]
mod tests {
    //! These tests need a Mac with a login session (the macOS runners of the CI have one, with a
    //! pasteboard). Without it they fail: a copy that could not be checked is never a pass.
    //!
    //! Only `the_general_pasteboard_...` touches the general pasteboard, so that tests running in
    //! parallel never overwrite one another; the others use a pasteboard of their own.

    use objc2::rc::Retained;
    use objc2_app_kit::{NSPasteboard, NSPasteboardType};
    use objc2_foundation::NSString;

    use super::{
        Clipboard, ClipboardOutcome, ClipboardReason, PasteboardClipboard, TextKind, copy_image_to,
        copy_text_to, html_type, plain_type, png_type, verify_string,
    };

    /// A minimal valid PNG (1 x 1), as bytes with a NUL and a high byte in them.
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R', 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0xFF,
    ];

    fn private_pasteboard() -> Retained<NSPasteboard> {
        NSPasteboard::pasteboardWithUniqueName()
    }

    fn string_of(pasteboard: &NSPasteboard, ty: &NSPasteboardType) -> Option<String> {
        pasteboard.stringForType(ty).map(|s| s.to_string())
    }

    #[test]
    fn plain_text_is_copied_and_read_back_as_plain_text_only() {
        let pb = private_pasteboard();
        let text = "https://example.test/a?b=c&d=é 日本語";
        assert_eq!(
            copy_text_to(&pb, text, TextKind::Plain),
            ClipboardOutcome::Copied
        );
        assert_eq!(string_of(&pb, plain_type()).as_deref(), Some(text));
        assert_eq!(string_of(&pb, html_type()), None);
    }

    #[test]
    fn html_is_copied_as_html_and_as_its_source_text() {
        let pb = private_pasteboard();
        let html = "<a href=\"https://example.test/\">lien</a>";
        assert_eq!(
            copy_text_to(&pb, html, TextKind::Html),
            ClipboardOutcome::Copied
        );
        assert_eq!(string_of(&pb, html_type()).as_deref(), Some(html));
        assert_eq!(string_of(&pb, plain_type()).as_deref(), Some(html));
    }

    #[test]
    fn an_image_is_copied_byte_for_byte_as_png() {
        let pb = private_pasteboard();
        assert_eq!(copy_image_to(&pb, PNG), ClipboardOutcome::Copied);
        let read = pb.dataForType(png_type()).map(|d| d.to_vec());
        assert_eq!(read.as_deref(), Some(PNG));
    }

    #[test]
    fn a_new_copy_replaces_what_was_there() {
        let pb = private_pasteboard();
        assert_eq!(
            copy_text_to(&pb, "<b>x</b>", TextKind::Html),
            ClipboardOutcome::Copied
        );
        assert_eq!(copy_image_to(&pb, PNG), ClipboardOutcome::Copied);
        assert_eq!(string_of(&pb, html_type()), None, "html survived an image");
        assert_eq!(string_of(&pb, plain_type()), None, "text survived an image");
        assert_eq!(
            copy_text_to(&pb, "after", TextKind::Plain),
            ClipboardOutcome::Copied
        );
        assert!(
            pb.dataForType(png_type()).is_none(),
            "the image survived a text"
        );
    }

    #[test]
    fn an_empty_text_is_still_checked() {
        let pb = private_pasteboard();
        let outcome = copy_text_to(&pb, "", TextKind::Plain);
        // Whatever the system does with an empty string, the answer is the read back's, never a
        // blind success: `Copied` only if an empty string really comes back.
        let read_back = string_of(&pb, plain_type());
        match outcome {
            ClipboardOutcome::Copied => assert_eq!(read_back.as_deref(), Some("")),
            ClipboardOutcome::Failed { .. } => assert_ne!(read_back.as_deref(), Some("")),
            ClipboardOutcome::Unavailable { .. } => panic!("the pasteboard is never unavailable"),
        }
    }

    #[test]
    fn a_read_back_that_differs_is_reported_with_lengths_and_not_the_content() {
        let pb = private_pasteboard();
        assert!(pb.setString_forType(&NSString::from_str("something else"), plain_type()));
        let err = verify_string(&pb, plain_type(), "public.utf8-plain-text", "secret-token")
            .expect_err("different content must not verify");
        assert!(
            err.contains("14 bytes") && err.contains("expected 12"),
            "{err}"
        );
        assert!(
            !err.contains("secret-token") && !err.contains("something"),
            "{err}"
        );
    }

    #[test]
    fn a_missing_type_is_a_read_back_failure() {
        let pb = private_pasteboard();
        pb.clearContents();
        assert!(verify_string(&pb, html_type(), "public.html", "x").is_err());
    }

    #[test]
    fn the_general_pasteboard_gets_text_html_then_an_image() {
        let clipboard = PasteboardClipboard::new();

        let text = "https://example.test/general-text";
        assert_eq!(
            clipboard.copy_text(text, TextKind::Plain),
            ClipboardOutcome::Copied
        );
        let general = NSPasteboard::generalPasteboard();
        assert_eq!(string_of(&general, plain_type()).as_deref(), Some(text));

        let html = "<a href=\"https://example.test/general-html\">x</a>";
        assert_eq!(
            clipboard.copy_text(html, TextKind::Html),
            ClipboardOutcome::Copied
        );
        let general = NSPasteboard::generalPasteboard();
        assert_eq!(string_of(&general, html_type()).as_deref(), Some(html));
        assert_eq!(string_of(&general, plain_type()).as_deref(), Some(html));

        assert_eq!(clipboard.copy_image(PNG), ClipboardOutcome::Copied);
        let general = NSPasteboard::generalPasteboard();
        let read = general.dataForType(png_type()).map(|d| d.to_vec());
        assert_eq!(read.as_deref(), Some(PNG));
    }

    #[test]
    fn the_failure_reason_of_a_refused_type_is_its_own() {
        let outcome = super::rejected("public.png");
        assert_eq!(outcome.reason(), Some(ClipboardReason::PasteboardRejected));
        assert_eq!(outcome.detail(), Some("the pasteboard refused public.png"));
    }
}
