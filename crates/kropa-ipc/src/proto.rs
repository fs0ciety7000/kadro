// SPDX-License-Identifier: GPL-3.0-or-later

//! The wire format: JSON-RPC 2.0, one message per line (`\n`), at most [`MAX_LINE`] bytes.
//!
//! Application errors travel in `error.data` as `{exit_code, code, reason}`, so that the command line
//! can end with the exit code the application chose.

use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncBufReadExt;

/// Version of the protocol, announced and checked by `hello`.
pub const PROTOCOL: u32 = 1;

/// Longest message in bytes, without its line feed. A longer one closes the connection.
pub const MAX_LINE: usize = 65_536;

/// JSON-RPC error codes (the standard ones, and one for application errors).
pub(crate) mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const APPLICATION: i64 = -32000;
}

/// An error returned by a method: what the command line turns into its exit code and `error` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    /// Process exit code (`docs/cli.md` section 3).
    pub exit_code: u8,
    /// Name of the exit code (`error.code`), e.g. `cancelled`.
    pub code: String,
    /// Machine sub-cause (`error.reason`).
    pub reason: Option<String>,
    /// Message, in English: the command line localises its own.
    pub message: String,
}

impl RpcError {
    /// An error with an exit code, its name and a message.
    #[must_use]
    pub fn new(exit_code: u8, code: &str, reason: Option<&str>, message: &str) -> Self {
        Self {
            exit_code,
            code: code.to_owned(),
            reason: reason.map(str::to_owned),
            message: message.to_owned(),
        }
    }

    /// Exit code 14 with a reason.
    pub(crate) fn daemon(reason: &str, message: &str) -> Self {
        Self::new(14, "daemon_unavailable", Some(reason), message)
    }

    pub(crate) fn wire(&self, rpc_code: i64) -> WireError {
        WireError {
            code: rpc_code,
            message: self.message.clone(),
            data: Some(WireData {
                exit_code: self.exit_code,
                code: self.code.clone(),
                reason: self.reason.clone(),
            }),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireData {
    pub exit_code: u8,
    pub code: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<WireData>,
}

impl WireError {
    /// The error as the command line sees it. A standard JSON-RPC error without `data` is a protocol
    /// error of the peer.
    pub(crate) fn into_rpc(self) -> RpcError {
        match self.data {
            Some(d) => RpcError {
                exit_code: d.exit_code,
                code: strip_control(&d.code),
                reason: d.reason.map(|r| strip_control(&r)),
                message: strip_control(&self.message),
            },
            None => RpcError::daemon("ipc_protocol_error", &strip_control(&self.message)),
        }
    }
}

/// Parameters of `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    /// Protocol version of the client.
    pub protocol: u32,
    /// Always `kropa`.
    pub client: String,
    /// Version of the client.
    pub version: String,
}

/// Result of `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    /// Protocol version of the server.
    pub protocol: u32,
    /// `kropa-gtk` or `kropa-app`.
    pub server: String,
    /// Version of the server.
    pub version: String,
    /// Methods the server answers.
    pub methods: Vec<String>,
    /// Optional features of the server, e.g. `layer-shell`.
    #[serde(default)]
    pub features: Vec<String>,
}

/// Parameters of a method for logs, by allow-list: only `hello`, `app.*` and `capture.*` are shown.
/// Everything else (`secrets.*`, and any method added later, a typo included) is masked, whatever the
/// verbosity (`docs/security/secrets.md` section 1.2, review 2.1a m9). A new family of methods that
/// may be logged is added here on purpose.
#[must_use]
pub fn loggable_params(method: &str, params: &Value) -> Value {
    let shown = method == "hello"
        || method
            .strip_prefix("app.")
            .or_else(|| method.strip_prefix("capture."))
            .is_some_and(|rest| !rest.is_empty());
    if shown {
        params.clone()
    } else {
        Value::String("<redacted>".to_owned())
    }
}

/// Text of the peer without control characters (escape sequences, line feeds, NUL...), for what is
/// printed on a terminal or put in a message: a server that is not the real one must not be able to
/// write on the user's screen (review 2.1a m11).
#[must_use]
pub fn strip_control(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// What reading one line produced.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A complete line, in the buffer, without its line feed.
    Complete,
    /// The peer closed the connection (a partial line is dropped).
    Eof,
    /// More than [`MAX_LINE`] bytes without a line feed: the connection must be closed.
    TooLong,
}

/// Reads one line of at most [`MAX_LINE`] bytes; never buffers more than that plus one chunk.
pub(crate) async fn read_line<R>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<Line>
where
    R: AsyncBufReadExt + Unpin,
{
    line.clear();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(Line::Eof);
        }
        let (take, found) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        if line.len() + take > MAX_LINE {
            return Ok(Line::TooLong);
        }
        line.extend_from_slice(&chunk[..take]);
        reader.consume(take + usize::from(found));
        if found {
            return Ok(Line::Complete);
        }
    }
}

/// Serialises a message into one line, or `None` when it exceeds [`MAX_LINE`].
pub(crate) fn encode<T: Serialize>(message: &T) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_LINE {
        return Ok(None);
    }
    bytes.push(b'\n');
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;

    async fn read(input: &[u8]) -> (Line, Vec<u8>) {
        let mut reader = BufReader::with_capacity(16, input);
        let mut line = Vec::new();
        let result = read_line(&mut reader, &mut line).await.unwrap();
        (result, line)
    }

    #[tokio::test]
    async fn a_line_of_the_maximum_length_is_read() {
        let mut input = vec![b'a'; MAX_LINE];
        input.push(b'\n');
        let (result, line) = read(&input).await;
        assert_eq!(result, Line::Complete);
        assert_eq!(line.len(), MAX_LINE);
    }

    #[tokio::test]
    async fn one_byte_more_is_too_long_with_or_without_a_line_feed() {
        let mut input = vec![b'a'; MAX_LINE + 1];
        assert_eq!(read(&input).await.0, Line::TooLong);
        input.push(b'\n');
        assert_eq!(read(&input).await.0, Line::TooLong);
    }

    #[tokio::test]
    async fn a_partial_line_at_eof_is_dropped() {
        assert_eq!(read(b"{\"a\":1}").await.0, Line::Eof);
        assert_eq!(read(b"").await.0, Line::Eof);
    }

    #[tokio::test]
    async fn two_lines_are_read_one_at_a_time() {
        let mut reader = BufReader::with_capacity(4, &b"one\ntwo\n"[..]);
        let mut line = Vec::new();
        assert_eq!(
            read_line(&mut reader, &mut line).await.unwrap(),
            Line::Complete
        );
        assert_eq!(line, b"one");
        assert_eq!(
            read_line(&mut reader, &mut line).await.unwrap(),
            Line::Complete
        );
        assert_eq!(line, b"two");
    }

    #[test]
    fn only_the_listed_methods_are_loggable() {
        let params = serde_json::json!({"value": "hunter2"});
        for shown in ["hello", "app.status", "app.quit", "capture.region"] {
            assert_eq!(loggable_params(shown, &params), params, "{shown}");
        }
        for masked in [
            "secrets.set",
            "secrets.unlock",
            "secret.set", // a typo is masked too
            "future.method",
            "helloo",
            "application.x",
            "app.",
            "capture.",
            "",
        ] {
            assert_eq!(loggable_params(masked, &params), "<redacted>", "{masked}");
        }
    }

    #[test]
    fn control_characters_are_removed_from_the_text_of_the_peer() {
        assert_eq!(strip_control("a\x1b[2Jb\nc\0d\u{9b}e"), "a[2Jbcde");
        assert_eq!(strip_control("naïve — ok"), "naïve — ok");
        let wire = WireError {
            code: 1,
            message: "bad\x1b]0;title\x07".into(),
            data: Some(WireData {
                exit_code: 1,
                code: "in\nternal".into(),
                reason: Some("re\x1bason".into()),
            }),
        };
        let e = wire.into_rpc();
        assert_eq!(
            (e.message.as_str(), e.code.as_str(), e.reason.as_deref()),
            ("bad]0;title", "internal", Some("reason"))
        );
    }

    #[test]
    fn an_oversized_message_is_not_encoded() {
        let big = Value::String("x".repeat(MAX_LINE));
        assert!(encode(&big).unwrap().is_none());
        assert!(encode(&Value::Null).unwrap().is_some());
    }

    #[test]
    fn a_standard_error_without_data_is_a_protocol_error() {
        let wire = WireError {
            code: code::METHOD_NOT_FOUND,
            message: "no".into(),
            data: None,
        };
        let e = wire.into_rpc();
        assert_eq!(
            (e.exit_code, e.reason.as_deref()),
            (14, Some("ipc_protocol_error"))
        );
    }
}
