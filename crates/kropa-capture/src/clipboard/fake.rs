// SPDX-License-Identifier: GPL-3.0-or-later

//! The double of [`crate::Clipboard`] for tests: it answers what it is told and records what it was
//! asked to copy.

use std::cell::RefCell;

use super::{Clipboard, ClipboardOutcome, TextKind};

/// One copy the [`FakeClipboard`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCopy {
    /// Text and its kind.
    Text(String, TextKind),
    /// PNG bytes.
    Image(Vec<u8>),
}

/// A clipboard that copies nothing: it answers with a fixed outcome and remembers the requests.
#[derive(Debug)]
pub struct FakeClipboard {
    outcome: ClipboardOutcome,
    copies: RefCell<Vec<FakeCopy>>,
}

impl FakeClipboard {
    /// A clipboard that answers `outcome` to every copy.
    #[must_use]
    pub fn answering(outcome: ClipboardOutcome) -> Self {
        Self {
            outcome,
            copies: RefCell::new(Vec::new()),
        }
    }

    /// A clipboard that reports every copy as done.
    #[must_use]
    pub fn working() -> Self {
        Self::answering(ClipboardOutcome::Copied)
    }

    /// What was asked so far, in order.
    #[must_use]
    pub fn copies(&self) -> Vec<FakeCopy> {
        self.copies.borrow().clone()
    }
}

impl Clipboard for FakeClipboard {
    fn copy_text(&self, text: &str, kind: TextKind) -> ClipboardOutcome {
        self.copies
            .borrow_mut()
            .push(FakeCopy::Text(text.to_owned(), kind));
        self.outcome.clone()
    }

    fn copy_image(&self, png: &[u8]) -> ClipboardOutcome {
        self.copies.borrow_mut().push(FakeCopy::Image(png.to_vec()));
        self.outcome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Clipboard, ClipboardOutcome, FakeClipboard, FakeCopy, TextKind};
    use crate::ClipboardReason;

    #[test]
    fn it_records_and_answers() {
        let fake = FakeClipboard::answering(ClipboardOutcome::unavailable(
            ClipboardReason::NoDataControl,
        ));
        assert_eq!(
            fake.copy_text("a", TextKind::Html).reason(),
            Some(ClipboardReason::NoDataControl)
        );
        assert_eq!(fake.copy_image(b"p").status(), "unavailable");
        assert_eq!(
            fake.copies(),
            [
                FakeCopy::Text("a".into(), TextKind::Html),
                FakeCopy::Image(b"p".to_vec())
            ]
        );
    }
}
