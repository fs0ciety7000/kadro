// SPDX-License-Identifier: GPL-3.0-or-later

//! What the process that asks for a copy and the helper that serves it say to each other, over the
//! helper's standard streams (never over its arguments, which any user can read in `ps`).
//!
//! Request, on the helper's stdin:
//!
//! ```text
//! kropa-clipboard 1\n
//! <kind>\n                 text | html | png
//! <length in bytes>\n
//! <that many bytes>
//! ```
//!
//! Answer, on its stdout, one line, written once the selection is set and read back (or has failed):
//!
//! ```text
//! ok\n
//! unavailable <reason>\n
//! failed <reason> <detail on one line>\n
//! ```

use std::io::{self, BufRead, Read, Write};

use super::{ClipboardOutcome, ClipboardReason};

/// First line of a request.
const MAGIC: &str = "kropa-clipboard 1";

/// Largest content the helper accepts, in bytes: a 8K screenshot as PNG stays far below it.
pub(crate) const MAX_PAYLOAD: usize = 64 * 1024 * 1024;

/// Longest technical detail kept in an answer, in characters.
const MAX_DETAIL: usize = 300;

/// What is copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Plain text.
    Text,
    /// HTML text.
    Html,
    /// PNG image.
    Png,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
            Self::Png => "png",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Text, Self::Html, Self::Png]
            .into_iter()
            .find(|k| k.as_str() == name)
    }
}

/// A request read by the helper.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) kind: Kind,
    pub(crate) data: Vec<u8>,
}

/// Why a request could not be read.
#[derive(Debug)]
pub(crate) enum RequestError {
    /// The stream broke or ended early.
    Io(io::Error),
    /// The header is not one of ours.
    Malformed(&'static str),
    /// The announced content is above [`MAX_PAYLOAD`].
    TooLarge,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "request not readable: {e}"),
            Self::Malformed(what) => write!(f, "malformed request: {what}"),
            Self::TooLarge => write!(f, "content above {MAX_PAYLOAD} bytes"),
        }
    }
}

/// Writes a request.
///
/// # Errors
/// The error of the stream (a helper that already died closes it: `BrokenPipe`).
pub(crate) fn write_request(out: &mut impl Write, kind: Kind, data: &[u8]) -> io::Result<()> {
    write!(out, "{MAGIC}\n{}\n{}\n", kind.as_str(), data.len())?;
    out.write_all(data)?;
    out.flush()
}

fn header_line(input: &mut impl BufRead) -> Result<String, RequestError> {
    let mut line = String::new();
    // A header line is a few bytes: a longer one is not ours, and is not read to the end.
    input
        .take(64)
        .read_line(&mut line)
        .map_err(RequestError::Io)?;
    match line.strip_suffix('\n') {
        Some(text) => Ok(text.to_owned()),
        None => Err(RequestError::Malformed("header line")),
    }
}

/// Reads a request.
///
/// # Errors
/// [`RequestError`] on a broken stream, a header that is not ours, or too much content.
pub(crate) fn read_request(input: &mut impl BufRead) -> Result<Request, RequestError> {
    if header_line(input)? != MAGIC {
        return Err(RequestError::Malformed("magic line"));
    }
    let kind = Kind::from_name(&header_line(input)?).ok_or(RequestError::Malformed("kind"))?;
    let length: usize = header_line(input)?
        .parse()
        .map_err(|_| RequestError::Malformed("length"))?;
    if length > MAX_PAYLOAD {
        return Err(RequestError::TooLarge);
    }
    let mut data = vec![0; length];
    input.read_exact(&mut data).map_err(RequestError::Io)?;
    Ok(Request { kind, data })
}

/// The answer line (without the newline) for an outcome.
pub(crate) fn format_answer(outcome: &ClipboardOutcome) -> String {
    match outcome {
        ClipboardOutcome::Copied => "ok".to_owned(),
        ClipboardOutcome::Unavailable { reason } => format!("unavailable {}", reason.as_str()),
        ClipboardOutcome::Failed { reason, detail } => {
            let one_line: String = detail
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(MAX_DETAIL)
                .collect();
            format!("failed {} {}", reason.as_str(), one_line.trim())
                .trim_end()
                .to_owned()
        }
    }
}

/// The outcome an answer line stands for. A line that is not ours is a failure of the helper, never
/// a success.
pub(crate) fn parse_answer(line: &str) -> ClipboardOutcome {
    let line = line.trim_end_matches(['\n', '\r']);
    let garbled = || ClipboardOutcome::failed(ClipboardReason::HelperFailed, "unreadable answer");
    let mut parts = line.splitn(3, ' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("ok"), None, _) => ClipboardOutcome::Copied,
        (Some("unavailable"), Some(code), None) => {
            ClipboardReason::from_code(code).map_or_else(garbled, ClipboardOutcome::unavailable)
        }
        (Some("failed"), Some(code), detail) => ClipboardReason::from_code(code)
            .map_or_else(garbled, |reason| {
                ClipboardOutcome::failed(reason, detail.unwrap_or_default())
            }),
        _ => garbled(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        ClipboardOutcome, ClipboardReason, Kind, MAGIC, MAX_PAYLOAD, Request, RequestError,
        format_answer, parse_answer, read_request, write_request,
    };

    fn read(bytes: &[u8]) -> Result<Request, RequestError> {
        read_request(&mut Cursor::new(bytes))
    }

    #[test]
    fn a_request_reads_back_whatever_its_bytes() {
        for (kind, data) in [
            (Kind::Text, b"link\nwith newline".to_vec()),
            (Kind::Html, Vec::new()),
            (Kind::Png, (0..=255).collect()),
        ] {
            let mut wire = Vec::new();
            write_request(&mut wire, kind, &data).unwrap();
            assert_eq!(read(&wire).unwrap(), Request { kind, data });
        }
    }

    #[test]
    fn a_request_of_another_program_is_refused() {
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\n\r\n"),
            Err(RequestError::Malformed("magic line"))
        ));
        let wrong_kind = format!("{MAGIC}\nvideo\n1\nx");
        assert!(matches!(
            read(wrong_kind.as_bytes()),
            Err(RequestError::Malformed("kind"))
        ));
        let wrong_length = format!("{MAGIC}\ntext\nmany\nx");
        assert!(matches!(
            read(wrong_length.as_bytes()),
            Err(RequestError::Malformed("length"))
        ));
        assert!(matches!(
            read(&[b'a'; 1000]),
            Err(RequestError::Malformed("header line"))
        ));
    }

    #[test]
    fn a_truncated_or_oversized_request_is_refused() {
        let short = format!("{MAGIC}\ntext\n10\nabc");
        assert!(matches!(read(short.as_bytes()), Err(RequestError::Io(_))));
        let big = format!("{MAGIC}\npng\n{}\n", MAX_PAYLOAD + 1);
        assert!(matches!(read(big.as_bytes()), Err(RequestError::TooLarge)));
    }

    #[test]
    fn answers_round_trip() {
        for outcome in [
            ClipboardOutcome::Copied,
            ClipboardOutcome::unavailable(ClipboardReason::NoDataControl),
            ClipboardOutcome::failed(ClipboardReason::Timeout, "no answer in 5 s"),
            ClipboardOutcome::failed(ClipboardReason::NoSeat, ""),
        ] {
            assert_eq!(
                parse_answer(&format!("{}\n", format_answer(&outcome))),
                outcome
            );
        }
    }

    #[test]
    fn a_detail_stays_on_one_line() {
        let outcome = ClipboardOutcome::failed(ClipboardReason::CompositorError, "a\nok\r\nb");
        let line = format_answer(&outcome);
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.ends_with(' '));
        assert_eq!(parse_answer(&line).status(), "failed");
    }

    #[test]
    fn a_line_that_is_not_an_answer_is_a_failure_never_a_success() {
        for line in [
            "",
            "okay",
            "ok extra",
            "copied",
            "failed",
            "failed nonsense x",
            "unavailable",
        ] {
            let outcome = parse_answer(line);
            assert_eq!(outcome.status(), "failed", "{line:?}");
            assert_eq!(
                outcome.reason(),
                Some(ClipboardReason::HelperFailed),
                "{line:?}"
            );
        }
    }
}
