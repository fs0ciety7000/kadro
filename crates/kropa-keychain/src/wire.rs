// SPDX-License-Identifier: GPL-3.0-or-later

//! What the client and the broker say to each other (`docs/security/secrets.md` section 2.2.1,
//! point 3): **one request per launch**, one JSON line of at most [`MAX_REQUEST_LEN`] bytes, and one
//! reply.
//!
//! The client writes its line and then closes its writing end; the broker reads up to that end of
//! file. That is how a second request, or a line that goes on for ever, is told from a single one
//! without waiting for a person: more than one newline, no final newline, more than 64 KiB, or a
//! field this version does not know is refused as malformed.
//!
//! Operations: `read <profile> [name]` (the items of one profile, never the whole store),
//! `write <profile>/<name>` (create or replace), `remove <profile>/<name>` and `status` (names, no
//! value). Documents travel as JSON strings; they are kept in `Zeroizing` buffers on both sides as
//! far as `serde_json` allows (it may leave a copy in its own scratch space, as everywhere else in
//! Kropa: `docs/security/secrets.md` section 3.5).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::item::{
    Failure, FailureKind, Item, ItemName, MAX_DOCUMENT_LEN, check_document, check_part,
};

/// Protocol version.
pub const VERSION: u8 = 1;

/// Longest request line, newline included.
pub const MAX_REQUEST_LEN: usize = 64 * 1024;

/// Longest reply the client accepts.
pub const MAX_REPLY_LEN: usize = 1024 * 1024;

/// How a `write` treats an item that exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Create; fail when the item exists.
    Create,
    /// Modify in place; fail when the item does not exist.
    Replace,
}

/// One operation.
#[derive(Debug)]
pub enum Request {
    /// The items of a profile, or one of them.
    Read {
        /// Profile identifier.
        profile: String,
        /// Restrict to this secret.
        name: Option<String>,
    },
    /// Create or replace one item.
    Write {
        /// Profile identifier.
        profile: String,
        /// Secret name.
        name: String,
        /// Create or replace.
        mode: WriteMode,
        /// The version 2 document.
        document: Zeroizing<Vec<u8>>,
    },
    /// Remove one item.
    Remove {
        /// Profile identifier.
        profile: String,
        /// Secret name.
        name: String,
    },
    /// The names of every item (no value).
    Status,
}

/// A request with the way it wants to be served.
#[derive(Debug)]
pub struct Envelope {
    /// Whether the keychain may show a prompt for it.
    pub interactive: bool,
    /// The operation.
    pub request: Request,
}

/// Why a request line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadRequest {
    /// Not one JSON line of the expected shape: too long, several lines, unknown or missing field,
    /// wrong version, bad name.
    Malformed,
    /// A well-formed request for an operation this version does not have.
    UnknownOperation,
}

fn zstring<'de, D: Deserializer<'de>>(d: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(d).map(Zeroizing::new)
}

fn opt_zstring<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Zeroizing<String>>, D::Error> {
    Option::<String>::deserialize(d).map(|o| o.map(Zeroizing::new))
}

#[derive(Serialize)]
struct RequestOut<'a> {
    v: u8,
    op: &'static str,
    interactive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    document: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestIn {
    v: u8,
    op: String,
    #[serde(default)]
    interactive: bool,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default, deserialize_with = "opt_zstring")]
    document: Option<Zeroizing<String>>,
}

/// The line of a request, in a buffer that is wiped on drop.
///
/// # Errors
/// [`FailureKind::StoreError`] when a name or the document is not acceptable, or the document is
/// not UTF-8 text.
pub fn encode_request(envelope: &Envelope) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut out = RequestOut {
        v: VERSION,
        op: "status",
        interactive: envelope.interactive,
        profile: None,
        name: None,
        mode: None,
        document: None,
    };
    let text;
    match &envelope.request {
        Request::Status => {}
        Request::Read { profile, name } => {
            check_part("profile", profile)?;
            if let Some(name) = name {
                check_part("name", name)?;
            }
            out.op = "read";
            out.profile = Some(profile);
            out.name = name.as_deref();
        }
        Request::Write {
            profile,
            name,
            mode,
            document,
        } => {
            check_part("profile", profile)?;
            check_part("name", name)?;
            check_document(document)?;
            text = std::str::from_utf8(document).map_err(|_| {
                Failure::new(FailureKind::StoreError, "the document is not UTF-8 text")
            })?;
            out.op = "write";
            out.profile = Some(profile);
            out.name = Some(name);
            out.mode = Some(match mode {
                WriteMode::Create => "create",
                WriteMode::Replace => "replace",
            });
            out.document = Some(text);
        }
        Request::Remove { profile, name } => {
            check_part("profile", profile)?;
            check_part("name", name)?;
            out.op = "remove";
            out.profile = Some(profile);
            out.name = Some(name);
        }
    }
    let mut line = Zeroizing::new(Vec::with_capacity(MAX_REQUEST_LEN));
    serde_json::to_writer(&mut *line, &out)
        .map_err(|_| Failure::new(FailureKind::StoreError, "the request cannot be encoded"))?;
    line.push(b'\n');
    if line.len() > MAX_REQUEST_LEN {
        return Err(Failure::new(
            FailureKind::StoreError,
            "the request is too large",
        ));
    }
    Ok(line)
}

/// Reads a request line as the broker does: exactly one line, at most [`MAX_REQUEST_LEN`] bytes.
///
/// # Errors
/// [`BadRequest`] for anything that is not exactly one valid request.
pub fn parse_request(line: &[u8]) -> Result<Envelope, BadRequest> {
    if line.len() > MAX_REQUEST_LEN || line.last() != Some(&b'\n') {
        return Err(BadRequest::Malformed);
    }
    let body = &line[..line.len() - 1];
    if body.contains(&b'\n') {
        // A second request, or a line break inside the first.
        return Err(BadRequest::Malformed);
    }
    let raw: RequestIn = serde_json::from_slice(body).map_err(|_| BadRequest::Malformed)?;
    if raw.v != VERSION {
        return Err(BadRequest::Malformed);
    }
    let part = |what: &str, value: Option<String>| -> Result<String, BadRequest> {
        let value = value.ok_or(BadRequest::Malformed)?;
        check_part(what, &value).map_err(|_| BadRequest::Malformed)?;
        Ok(value)
    };
    let RequestIn {
        op,
        interactive,
        profile,
        name,
        mode,
        document,
        ..
    } = raw;
    let request = match op.as_str() {
        "status" => {
            if profile.is_some() || name.is_some() || mode.is_some() || document.is_some() {
                return Err(BadRequest::Malformed);
            }
            Request::Status
        }
        "read" => {
            if mode.is_some() || document.is_some() {
                return Err(BadRequest::Malformed);
            }
            let name = match name {
                Some(n) => Some(part("name", Some(n))?),
                None => None,
            };
            Request::Read {
                profile: part("profile", profile)?,
                name,
            }
        }
        "remove" => {
            if mode.is_some() || document.is_some() {
                return Err(BadRequest::Malformed);
            }
            Request::Remove {
                profile: part("profile", profile)?,
                name: part("name", name)?,
            }
        }
        "write" => {
            let mode = match mode.as_deref() {
                Some("create") => WriteMode::Create,
                Some("replace") => WriteMode::Replace,
                _ => return Err(BadRequest::Malformed),
            };
            let mut text = document.ok_or(BadRequest::Malformed)?;
            if text.len() > MAX_DOCUMENT_LEN {
                return Err(BadRequest::Malformed);
            }
            // Moves the buffer out of the wrapper: no copy of the value is left behind.
            let bytes = std::mem::take(&mut *text).into_bytes();
            Request::Write {
                profile: part("profile", profile)?,
                name: part("name", name)?,
                mode,
                document: Zeroizing::new(bytes),
            }
        }
        _ => return Err(BadRequest::UnknownOperation),
    };
    Ok(Envelope {
        interactive,
        request,
    })
}

/// The answer to a request.
#[derive(Debug)]
pub enum Reply {
    /// A `write` was done.
    Done,
    /// A `remove` was done: whether there was an item.
    Removed(bool),
    /// The items a `read` found.
    Items(Vec<Item>),
    /// The names a `status` found.
    Names(Vec<ItemName>),
    /// The operation failed.
    Failed(Failure),
}

#[derive(Serialize)]
struct ItemOut<'a> {
    name: &'a str,
    document: &'a str,
}

#[derive(Serialize)]
struct NameOut<'a> {
    profile: &'a str,
    name: &'a str,
}

struct Detail<'a>(&'a str);

impl Serialize for Detail<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

#[derive(Serialize)]
struct ReplyOut<'a> {
    v: u8,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Detail<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<ItemOut<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    names: Option<Vec<NameOut<'a>>>,
}

#[derive(Deserialize)]
struct ItemIn {
    name: String,
    #[serde(deserialize_with = "zstring")]
    document: Zeroizing<String>,
}

#[derive(Deserialize)]
struct NameIn {
    profile: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyIn {
    v: u8,
    ok: bool,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    removed: Option<bool>,
    #[serde(default)]
    items: Option<Vec<ItemIn>>,
    #[serde(default)]
    names: Option<Vec<NameIn>>,
}

/// The line of a reply, in a buffer that is wiped on drop. A reply that cannot be encoded (a
/// document that is not UTF-8) is replaced by a failure.
#[must_use]
pub fn encode_reply(reply: &Reply) -> Zeroizing<Vec<u8>> {
    let mut out = ReplyOut {
        v: VERSION,
        ok: true,
        kind: None,
        detail: None,
        removed: None,
        items: None,
        names: None,
    };
    let mut capacity = 1024;
    match reply {
        Reply::Done => {}
        Reply::Removed(removed) => out.removed = Some(*removed),
        Reply::Items(items) => {
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                let Ok(document) = std::str::from_utf8(&item.document) else {
                    return encode_reply(&Reply::Failed(Failure::new(
                        FailureKind::StoreError,
                        "a stored document is not UTF-8 text",
                    )));
                };
                capacity += document.len() + item.name.len() + 64;
                list.push(ItemOut {
                    name: &item.name,
                    document,
                });
            }
            out.items = Some(list);
        }
        Reply::Names(names) => {
            capacity += names
                .iter()
                .map(|n| n.profile.len() + n.name.len() + 32)
                .sum::<usize>();
            out.names = Some(
                names
                    .iter()
                    .map(|n| NameOut {
                        profile: &n.profile,
                        name: &n.name,
                    })
                    .collect(),
            );
        }
        Reply::Failed(failure) => {
            out.ok = false;
            out.kind = Some(failure.kind.wire_name());
            out.detail = Some(Detail(&failure.detail));
        }
    }
    let mut line = Zeroizing::new(Vec::with_capacity(capacity));
    // Writing into a Vec cannot fail, and the types above always serialize.
    if serde_json::to_writer(&mut *line, &out).is_err() {
        line.clear();
        line.extend_from_slice(
            br#"{"v":1,"ok":false,"kind":"store_error","detail":"reply not encodable"}"#,
        );
    }
    line.push(b'\n');
    line
}

/// Reads a reply as the client does.
///
/// # Errors
/// [`FailureKind::BrokerUnavailable`] when the bytes are not a reply of this protocol: whatever
/// answered is not the broker this client speaks to.
pub fn decode_reply(bytes: &[u8]) -> Result<Reply, Failure> {
    let bad = || {
        Failure::new(
            FailureKind::BrokerUnavailable,
            "the Keychain broker sent an answer that is not understood",
        )
    };
    if bytes.len() > MAX_REPLY_LEN {
        return Err(bad());
    }
    let raw: ReplyIn = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if raw.v != VERSION {
        return Err(bad());
    }
    if !raw.ok {
        let kind = raw
            .kind
            .as_deref()
            .and_then(FailureKind::from_wire_name)
            .ok_or_else(bad)?;
        if raw.removed.is_some() || raw.items.is_some() || raw.names.is_some() {
            return Err(bad());
        }
        return Ok(Reply::Failed(Failure::new(
            kind,
            raw.detail.unwrap_or_default(),
        )));
    }
    if raw.kind.is_some() || raw.detail.is_some() {
        return Err(bad());
    }
    match (raw.removed, raw.items, raw.names) {
        (None, None, None) => Ok(Reply::Done),
        (Some(removed), None, None) => Ok(Reply::Removed(removed)),
        (None, Some(items), None) => {
            let mut out = Vec::with_capacity(items.len());
            for ItemIn { name, mut document } in items {
                let bytes = std::mem::take(&mut *document).into_bytes();
                out.push(Item {
                    name,
                    document: Zeroizing::new(bytes),
                });
            }
            Ok(Reply::Items(out))
        }
        (None, None, Some(names)) => Ok(Reply::Names(
            names
                .into_iter()
                .map(|n| ItemName {
                    profile: n.profile,
                    name: n.name,
                })
                .collect(),
        )),
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::{
        BadRequest, Envelope, MAX_REQUEST_LEN, Reply, Request, WriteMode, decode_reply,
        encode_reply, encode_request, parse_request,
    };
    use crate::item::{Failure, FailureKind, Item, ItemName};

    fn round_trip(request: Request) -> Envelope {
        let line = encode_request(&Envelope {
            interactive: true,
            request,
        })
        .unwrap();
        parse_request(&line).unwrap()
    }

    #[test]
    fn every_request_survives_the_wire() {
        let e = round_trip(Request::Read {
            profile: "zipline".into(),
            name: Some("token".into()),
        });
        assert!(e.interactive);
        assert!(
            matches!(e.request, Request::Read { ref profile, name: Some(ref n) } if profile == "zipline" && n == "token")
        );

        let e = round_trip(Request::Write {
            profile: "p".into(),
            name: "n".into(),
            mode: WriteMode::Replace,
            document: Zeroizing::new(br#"{"v":2,"entries":{}}"#.to_vec()),
        });
        let Request::Write { mode, document, .. } = e.request else {
            panic!("not a write");
        };
        assert_eq!(mode, WriteMode::Replace);
        assert_eq!(&**document, br#"{"v":2,"entries":{}}"#);

        let e = round_trip(Request::Remove {
            profile: "p".into(),
            name: "n".into(),
        });
        assert!(matches!(e.request, Request::Remove { .. }));
        assert!(matches!(
            round_trip(Request::Status).request,
            Request::Status
        ));
    }

    #[test]
    fn a_request_is_one_line() {
        let line = encode_request(&Envelope {
            interactive: false,
            request: Request::Status,
        })
        .unwrap();
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(line.last(), Some(&b'\n'));
    }

    #[test]
    fn two_requests_in_one_launch_are_malformed() {
        let one = br#"{"v":1,"op":"status"}"#;
        let two = [&one[..], b"\n", &one[..], b"\n"].concat();
        assert_eq!(parse_request(&two).unwrap_err(), BadRequest::Malformed);
    }

    #[test]
    fn a_line_without_its_newline_or_too_long_is_malformed() {
        assert_eq!(
            parse_request(br#"{"v":1,"op":"status"}"#).unwrap_err(),
            BadRequest::Malformed
        );
        let mut long = vec![b' '; MAX_REQUEST_LEN];
        long.push(b'\n');
        assert_eq!(parse_request(&long).unwrap_err(), BadRequest::Malformed);
    }

    #[test]
    fn unknown_operations_are_told_from_malformed_requests() {
        assert_eq!(
            parse_request(b"{\"v\":1,\"op\":\"dump\"}\n").unwrap_err(),
            BadRequest::UnknownOperation
        );
        assert_eq!(
            parse_request(b"not json\n").unwrap_err(),
            BadRequest::Malformed
        );
    }

    #[test]
    fn unknown_fields_wrong_versions_and_stray_fields_are_malformed() {
        for line in [
            &b"{\"v\":1,\"op\":\"status\",\"all\":true}\n"[..],
            b"{\"v\":2,\"op\":\"status\"}\n",
            b"{\"v\":1,\"op\":\"status\",\"profile\":\"p\"}\n",
            b"{\"v\":1,\"op\":\"read\"}\n",
            b"{\"v\":1,\"op\":\"read\",\"profile\":\"p\",\"document\":\"x\"}\n",
            b"{\"v\":1,\"op\":\"remove\",\"profile\":\"p\"}\n",
            b"{\"v\":1,\"op\":\"write\",\"profile\":\"p\",\"name\":\"n\",\"document\":\"x\"}\n",
            b"{\"v\":1,\"op\":\"write\",\"profile\":\"p\",\"name\":\"n\",\"mode\":\"upsert\",\"document\":\"x\"}\n",
        ] {
            assert_eq!(
                parse_request(line).unwrap_err(),
                BadRequest::Malformed,
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn names_with_a_slash_or_empty_are_malformed() {
        for line in [
            &b"{\"v\":1,\"op\":\"read\",\"profile\":\"a/b\"}\n"[..],
            b"{\"v\":1,\"op\":\"read\",\"profile\":\"\"}\n",
            b"{\"v\":1,\"op\":\"remove\",\"profile\":\"p\",\"name\":\"../x\"}\n",
        ] {
            assert_eq!(parse_request(line).unwrap_err(), BadRequest::Malformed);
        }
    }

    #[test]
    fn the_client_refuses_to_send_what_the_broker_would_refuse() {
        let bad_name = Envelope {
            interactive: false,
            request: Request::Remove {
                profile: "p".into(),
                name: "a/b".into(),
            },
        };
        assert!(encode_request(&bad_name).is_err());
        let not_text = Envelope {
            interactive: false,
            request: Request::Write {
                profile: "p".into(),
                name: "n".into(),
                mode: WriteMode::Create,
                document: Zeroizing::new(vec![0xff, 0xfe]),
            },
        };
        assert!(encode_request(&not_text).is_err());
    }

    #[test]
    fn replies_survive_the_wire() {
        let reply = Reply::Items(vec![Item {
            name: "token".into(),
            document: Zeroizing::new(b"{\"a\":\"b\\n\"}".to_vec()),
        }]);
        let Reply::Items(items) = decode_reply(&encode_reply(&reply)).unwrap() else {
            panic!("not items");
        };
        assert_eq!(items[0].name, "token");
        assert_eq!(&**items[0].document, b"{\"a\":\"b\\n\"}");

        assert!(matches!(
            decode_reply(&encode_reply(&Reply::Done)).unwrap(),
            Reply::Done
        ));
        assert!(matches!(
            decode_reply(&encode_reply(&Reply::Removed(true))).unwrap(),
            Reply::Removed(true)
        ));
        let names = Reply::Names(vec![ItemName {
            profile: "p".into(),
            name: "n".into(),
        }]);
        let Reply::Names(names) = decode_reply(&encode_reply(&names)).unwrap() else {
            panic!("not names");
        };
        assert_eq!(names[0].profile, "p");
        let failed = Reply::Failed(Failure::new(FailureKind::Denied, "no"));
        let Reply::Failed(f) = decode_reply(&encode_reply(&failed)).unwrap() else {
            panic!("not a failure");
        };
        assert_eq!((f.kind, f.detail.as_str()), (FailureKind::Denied, "no"));
    }

    #[test]
    fn a_document_that_is_not_text_becomes_a_failure_not_a_broken_line() {
        let reply = Reply::Items(vec![Item {
            name: "n".into(),
            document: Zeroizing::new(vec![0xff]),
        }]);
        assert!(matches!(
            decode_reply(&encode_reply(&reply)).unwrap(),
            Reply::Failed(Failure {
                kind: FailureKind::StoreError,
                ..
            })
        ));
    }

    #[test]
    fn what_is_not_a_reply_is_refused() {
        for bytes in [
            &b""[..],
            b"hello",
            b"{\"v\":9,\"ok\":true}",
            b"{\"v\":1,\"ok\":false}",
            b"{\"v\":1,\"ok\":false,\"kind\":\"nope\"}",
            b"{\"v\":1,\"ok\":true,\"removed\":true,\"items\":[]}",
            b"{\"v\":1,\"ok\":true,\"extra\":1}",
        ] {
            let e = decode_reply(bytes).unwrap_err();
            assert_eq!(e.kind, FailureKind::BrokerUnavailable);
        }
    }
}
