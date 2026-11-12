// SPDX-License-Identifier: GPL-3.0-or-later

//! The clipboard: [`Clipboard`] is the contract every platform fills, [`ClipboardOutcome`] what it
//! reports.
//!
//! An outcome says what really happened. [`ClipboardOutcome::Copied`] is only returned once the
//! content was handed to the system and, where the platform allows it, read back; what the platform
//! cannot do is [`ClipboardOutcome::Unavailable`] with a stable reason (GNOME offers no clipboard
//! access without focus, an X11-only session is not served yet); what was tried and went wrong is
//! [`ClipboardOutcome::Failed`]. Nothing here ever pretends (`docs/cli.md` section 5).
//!
//! The implementation is chosen from what the session offers, never from the desktop name:
//!
//! - Linux, Wayland with `ext-data-control-v1` or `zwlr_data_control_v1` (KDE, wlroots):
//!   [`WaylandClipboard`]. A Wayland selection only lives as long as the process that owns it, so
//!   the content is served by a detached child process, like `wl-copy`, with no daemon (lot 1).
//! - Linux, Wayland without data-control (GNOME): `unavailable` (`no_data_control`).
//! - Linux, X11 only, or no display: `unavailable` (`x11_not_supported`, `no_graphical_session`).
//! - macOS: the general `NSPasteboard` (`PasteboardClipboard`, `macos/pasteboard.rs`); the content is
//!   stored by the pasteboard server at once and outlives the process, and every type is read back.
//!
//! [`FakeClipboard`] is the double of the tests of the crates above.

mod fake;
#[cfg(target_os = "linux")]
mod protocol;
#[cfg(target_os = "linux")]
mod wayland;

pub use fake::{FakeClipboard, FakeCopy};
#[cfg(target_os = "linux")]
pub use wayland::{
    CLIPBOARD_SERVE_ARGUMENT, Environment, WaylandClipboard, probe_data_control, run_helper,
};

/// What a piece of text is, which decides how it is offered to the applications that paste it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// Plain text: a URL, a Markdown link, or any snippet meant to be pasted as it is written.
    Plain,
    /// HTML, offered as `text/html` for rich-text fields and as plain text (the markup) elsewhere.
    Html,
}

/// Why nothing was copied. Stable codes, shown as `clipboard.reason` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardReason {
    /// Wayland session whose compositor offers no data-control protocol (GNOME): the clipboard of
    /// another client cannot be set without keyboard focus. Unavailable.
    NoDataControl,
    /// Neither `WAYLAND_DISPLAY` nor `DISPLAY`: no graphical session to copy to. Unavailable.
    NoGraphicalSession,
    /// An X11 session without Wayland: the X11 selection is not served yet. Unavailable.
    X11NotSupported,
    /// No clipboard for this operating system. Unavailable.
    UnsupportedPlatform,
    /// `WAYLAND_DISPLAY` is set but the compositor cannot be reached. Failed.
    ConnectionFailed,
    /// The compositor has no seat to copy to. Failed.
    NoSeat,
    /// The compositor answered with a protocol error. Failed.
    CompositorError,
    /// The process that serves the selection could not be started. Failed.
    HelperSpawnFailed,
    /// The process that serves the selection died or answered nonsense. Failed.
    HelperFailed,
    /// The compositor did not answer in time. Failed.
    Timeout,
    /// The system refused to store the content (macOS: `setString:forType:` or `setData:forType:`
    /// answered `NO`). Failed.
    PasteboardRejected,
    /// The content was handed over but reading it back gave something else, or nothing. Failed.
    SelectionMismatch,
    /// The content is too large to hand to the compositor. Failed.
    TooLarge,
}

/// Every reason code, for the tests that check that each has a message in both languages.
pub const ALL_CLIPBOARD_REASONS: [&str; 13] = [
    "no_data_control",
    "no_graphical_session",
    "x11_not_supported",
    "unsupported_platform",
    "connection_failed",
    "no_seat",
    "compositor_error",
    "helper_spawn_failed",
    "helper_failed",
    "timeout",
    "pasteboard_rejected",
    "selection_mismatch",
    "too_large",
];

impl ClipboardReason {
    /// Every reason, in the order of [`ALL_CLIPBOARD_REASONS`].
    pub const ALL: [Self; 13] = [
        Self::NoDataControl,
        Self::NoGraphicalSession,
        Self::X11NotSupported,
        Self::UnsupportedPlatform,
        Self::ConnectionFailed,
        Self::NoSeat,
        Self::CompositorError,
        Self::HelperSpawnFailed,
        Self::HelperFailed,
        Self::Timeout,
        Self::PasteboardRejected,
        Self::SelectionMismatch,
        Self::TooLarge,
    ];

    /// The code used in JSON and in Fluent message ids (with dashes).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoDataControl => "no_data_control",
            Self::NoGraphicalSession => "no_graphical_session",
            Self::X11NotSupported => "x11_not_supported",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::ConnectionFailed => "connection_failed",
            Self::NoSeat => "no_seat",
            Self::CompositorError => "compositor_error",
            Self::HelperSpawnFailed => "helper_spawn_failed",
            Self::HelperFailed => "helper_failed",
            Self::Timeout => "timeout",
            Self::PasteboardRejected => "pasteboard_rejected",
            Self::SelectionMismatch => "selection_mismatch",
            Self::TooLarge => "too_large",
        }
    }

    /// The reason of a code, `None` for an unknown one.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == code)
    }
}

/// What a copy really did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardOutcome {
    /// The content is in the clipboard of the session.
    Copied,
    /// This system cannot do it (or not yet): nothing was tried.
    Unavailable {
        /// Why.
        reason: ClipboardReason,
    },
    /// It was tried and failed.
    Failed {
        /// Why.
        reason: ClipboardReason,
        /// Technical text of the underlying error, not translated, may be empty.
        detail: String,
    },
}

impl ClipboardOutcome {
    /// `Unavailable` for `reason`.
    #[must_use]
    pub fn unavailable(reason: ClipboardReason) -> Self {
        Self::Unavailable { reason }
    }

    /// `Failed` for `reason` with a technical detail.
    #[must_use]
    pub fn failed(reason: ClipboardReason, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason,
            detail: detail.into(),
        }
    }

    /// Name used in JSON (`clipboard.status`): `copied`, `unavailable` or `failed`.
    #[must_use]
    pub fn status(&self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::Unavailable { .. } => "unavailable",
            Self::Failed { .. } => "failed",
        }
    }

    /// The reason code (`clipboard.reason`), `None` when copied.
    #[must_use]
    pub fn reason(&self) -> Option<ClipboardReason> {
        match self {
            Self::Copied => None,
            Self::Unavailable { reason } | Self::Failed { reason, .. } => Some(*reason),
        }
    }

    /// The technical detail of a failure, if any.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Failed { detail, .. } if !detail.is_empty() => Some(detail),
            _ => None,
        }
    }
}

/// Puts content in the clipboard of the user's session.
///
/// Both methods block until the outcome is known (a few seconds at most) and never panic. The
/// `kropa-app` daemon of lot 2 will own the selection itself and implement this trait without a
/// helper process.
pub trait Clipboard {
    /// Copies `text`.
    fn copy_text(&self, text: &str, kind: TextKind) -> ClipboardOutcome;

    /// Copies an image given as PNG bytes (offered as `image/png`).
    fn copy_image(&self, png: &[u8]) -> ClipboardOutcome;
}

/// A clipboard that does not exist on this system: every copy is `unavailable` with one reason.
///
/// Used for the platforms that have no backend.
#[derive(Debug, Clone, Copy)]
pub struct UnavailableClipboard {
    reason: ClipboardReason,
}

impl UnavailableClipboard {
    /// A clipboard that always answers `unavailable` with `reason`.
    #[must_use]
    pub fn new(reason: ClipboardReason) -> Self {
        Self { reason }
    }
}

impl Clipboard for UnavailableClipboard {
    fn copy_text(&self, _text: &str, _kind: TextKind) -> ClipboardOutcome {
        ClipboardOutcome::unavailable(self.reason)
    }

    fn copy_image(&self, _png: &[u8]) -> ClipboardOutcome {
        ClipboardOutcome::unavailable(self.reason)
    }
}

/// The clipboard this system offers: Wayland data-control on Linux, the general `NSPasteboard` on
/// macOS, `unavailable` elsewhere.
#[must_use]
pub fn default_clipboard() -> Box<dyn Clipboard> {
    #[cfg(target_os = "linux")]
    {
        Box::new(WaylandClipboard::from_process())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(crate::macos::PasteboardClipboard::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Box::new(UnavailableClipboard::new(
            ClipboardReason::UnsupportedPlatform,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ALL_CLIPBOARD_REASONS, Clipboard, ClipboardOutcome, ClipboardReason, TextKind,
        UnavailableClipboard,
    };

    #[test]
    fn every_reason_has_a_unique_code_that_reads_back() {
        let codes: Vec<&str> = ClipboardReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(codes, ALL_CLIPBOARD_REASONS);
        for reason in ClipboardReason::ALL {
            assert_eq!(ClipboardReason::from_code(reason.as_str()), Some(reason));
        }
        assert_eq!(ClipboardReason::from_code("gnome"), None);
    }

    #[test]
    fn outcomes_name_their_status_reason_and_detail() {
        assert_eq!(ClipboardOutcome::Copied.status(), "copied");
        assert_eq!(ClipboardOutcome::Copied.reason(), None);
        let un = ClipboardOutcome::unavailable(ClipboardReason::NoDataControl);
        assert_eq!((un.status(), un.detail()), ("unavailable", None));
        let failed = ClipboardOutcome::failed(ClipboardReason::Timeout, "slow");
        assert_eq!(failed.status(), "failed");
        assert_eq!(failed.reason(), Some(ClipboardReason::Timeout));
        assert_eq!(failed.detail(), Some("slow"));
        assert_eq!(
            ClipboardOutcome::failed(ClipboardReason::Timeout, "").detail(),
            None
        );
    }

    #[test]
    fn the_unavailable_clipboard_never_copies() {
        let clipboard = UnavailableClipboard::new(ClipboardReason::UnsupportedPlatform);
        let expected = ClipboardOutcome::unavailable(ClipboardReason::UnsupportedPlatform);
        assert_eq!(clipboard.copy_text("x", TextKind::Plain), expected);
        assert_eq!(clipboard.copy_image(b"png"), expected);
    }
}
