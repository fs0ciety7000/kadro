// SPDX-License-Identifier: GPL-3.0-or-later

//! The broker: what `kropa __store-broker` does once it is started (`docs/security/secrets.md`
//! section 2.2.1).
//!
//! Order of events, and what each step may touch:
//!
//! 1. [`check_launch`]: nothing but facts about the process. **No access to the store.**
//! 2. fd 3 is taken, the request is read to the end of file (one line, 64 KiB at most) and parsed.
//!    A bad request ends here, **still without touching the store**.
//! 3. The parent is looked at again; if it is gone nothing is done.
//! 4. The one operation runs against the store.
//! 5. The parent is looked at again; if it is gone nothing is sent. Otherwise the reply goes out and
//!    the broker ends.
//!
//! A reply is only ever the items of the profile that was asked for, or names, or a failure: never
//! the whole store.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::guard::{Admitted, LaunchFacts, Refusal, check_launch};
use crate::item::{Failure, ItemStore};
use crate::wire::{
    BadRequest, Envelope, MAX_REQUEST_LEN, Reply, Request, WriteMode, encode_reply, parse_request,
};

/// How long the broker waits for the client to finish its request line.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the broker lives at most, a Keychain prompt included (section 2.2.1 point 3).
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Serves one launch: checks, one request, one reply.
///
/// `open` gives the channel; it is called only once the checks passed.
///
/// # Errors
/// [`Refusal`] when a check fails or the request is not acceptable. The store has not been touched
/// in that case, and nothing has been answered.
pub fn serve(
    open: impl FnOnce() -> io::Result<UnixStream>,
    facts: &dyn LaunchFacts,
    store: &dyn ItemStore,
) -> Result<(), Refusal> {
    let admitted = check_launch(facts)?;
    let mut stream = open().map_err(|_| Refusal::NoChannel)?;
    serve_admitted(&mut stream, admitted, facts, store)
}

fn io_refusal(error: &io::Error) -> Refusal {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Refusal::Timeout,
        _ => Refusal::ChannelIo,
    }
}

fn read_request(stream: &mut UnixStream) -> Result<Zeroizing<Vec<u8>>, Refusal> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| io_refusal(&e))?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| io_refusal(&e))?;
    // Allocated once, so that reading never reallocates and leaves a copy of a value behind.
    let mut line = Zeroizing::new(Vec::with_capacity(MAX_REQUEST_LEN + 1));
    stream
        .by_ref()
        .take(MAX_REQUEST_LEN as u64 + 1)
        .read_to_end(&mut line)
        .map_err(|e| io_refusal(&e))?;
    if line.len() > MAX_REQUEST_LEN {
        return Err(Refusal::BadRequest);
    }
    Ok(line)
}

fn still_parented(facts: &dyn LaunchFacts, admitted: Admitted) -> Result<(), Refusal> {
    if facts.parent_pid() == admitted.parent {
        Ok(())
    } else {
        Err(Refusal::ParentGone)
    }
}

fn execute(envelope: &Envelope, store: &dyn ItemStore) -> Reply {
    store.set_interactive(envelope.interactive);
    let outcome: Result<Reply, Failure> = match &envelope.request {
        Request::Read { profile, name } => store.read(profile, name.as_deref()).map(Reply::Items),
        Request::Write {
            profile,
            name,
            mode,
            document,
        } => match mode {
            WriteMode::Create => store.create(profile, name, document),
            WriteMode::Replace => store.replace(profile, name, document),
        }
        .map(|()| Reply::Done),
        Request::Remove { profile, name } => store.remove(profile, name).map(Reply::Removed),
        Request::Status => store.list().map(Reply::Names),
    };
    outcome.unwrap_or_else(Reply::Failed)
}

/// Steps 2 to 5, for a launch that passed the checks.
///
/// # Errors
/// [`Refusal`], see [`serve`].
pub fn serve_admitted(
    stream: &mut UnixStream,
    admitted: Admitted,
    facts: &dyn LaunchFacts,
    store: &dyn ItemStore,
) -> Result<(), Refusal> {
    let line = read_request(stream)?;
    let envelope = parse_request(&line).map_err(|e| match e {
        BadRequest::Malformed => Refusal::BadRequest,
        BadRequest::UnknownOperation => Refusal::UnknownOperation,
    })?;
    drop(line);
    still_parented(facts, admitted)?;
    let reply = execute(&envelope, store);
    drop(envelope);
    still_parented(facts, admitted)?;
    let bytes = encode_reply(&reply);
    drop(reply);
    stream.write_all(&bytes).map_err(|e| io_refusal(&e))?;
    stream.flush().map_err(|e| io_refusal(&e))?;
    // Both directions are over: the client reads to the end of file.
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// The whole life of a broker process: the watchdog, [`serve`], the exit status. The refusal, if
/// any, is named on stderr (nothing more) and in the exit status ([`Refusal::exit_code`]).
///
/// `take_channel` gives fd 3 as a socket once the checks passed.
#[must_use]
pub fn run(
    facts: &dyn LaunchFacts,
    store: &dyn ItemStore,
    take_channel: impl FnOnce() -> io::Result<UnixStream>,
) -> u8 {
    // The whole call, prompt included, ends within the limit whatever the Keychain does.
    let spawned = std::thread::Builder::new()
        .name("kropa-broker-watchdog".to_owned())
        .spawn(|| {
            std::thread::sleep(TOTAL_TIMEOUT);
            std::process::exit(i32::from(Refusal::Timeout.exit_code()));
        });
    if spawned.is_err() {
        return Refusal::ChannelIo.exit_code();
    }
    match serve(take_channel, facts, store) {
        Ok(()) => 0,
        Err(refusal) => {
            let _ = writeln!(
                io::stderr(),
                "kropa: keychain broker refused: {}",
                refusal.reason()
            );
            refusal.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::Mutex;

    use super::{serve, serve_admitted};
    use crate::guard::{
        Admitted, ChannelState, LaunchFacts, PeerToken, ProjectCertificate, Refusal, Unverified,
    };
    use crate::item::{Failure, FailureKind, ItemStore};
    use crate::memory::MemoryItemStore;
    use crate::wire::{Envelope, Reply, Request, WriteMode, decode_reply, encode_request};
    use zeroize::Zeroizing;

    /// Facts of a launch that passes; `parents` is what `getppid` says on each call.
    struct Facts {
        parents: Mutex<Vec<u32>>,
        code: Result<(), Unverified>,
    }

    impl Facts {
        fn good() -> Self {
            Self::with_parents(vec![500])
        }

        fn with_parents(parents: Vec<u32>) -> Self {
            Self {
                parents: Mutex::new(parents),
                code: Ok(()),
            }
        }
    }

    impl LaunchFacts for Facts {
        fn parent_pid(&self) -> u32 {
            let mut parents = self.parents.lock().unwrap();
            if parents.len() > 1 {
                parents.remove(0)
            } else {
                parents[0]
            }
        }

        fn channel(&self) -> ChannelState {
            ChannelState::Connected
        }

        fn peer_token(&self) -> Result<PeerToken, Unverified> {
            Ok(PeerToken {
                pid: 500,
                raw: [0; 8],
            })
        }

        fn project_certificate(&self) -> Result<ProjectCertificate, Unverified> {
            Ok(ProjectCertificate::from_hex(&"cd".repeat(20)).unwrap())
        }

        fn check_peer_code(
            &self,
            _token: &PeerToken,
            _certificate: &ProjectCertificate,
        ) -> Result<(), Unverified> {
            self.code
        }
    }

    /// Runs the broker against `request_bytes`, as the client does: write, close the writing end, read
    /// until the end of file.
    fn exchange(
        facts: &Facts,
        store: &dyn ItemStore,
        request_bytes: &[u8],
    ) -> (Result<(), Refusal>, Vec<u8>) {
        let (client, server) = UnixStream::pair().unwrap();
        let bytes = request_bytes.to_vec();
        let thread = std::thread::spawn(move || {
            let mut client = client;
            // The broker may stop reading a request it has already refused: a write that fails then
            // is the broker's business, not the test's.
            let _ = client.write_all(&bytes);
            let _ = client.shutdown(std::net::Shutdown::Write);
            let mut reply = Vec::new();
            let _ = client.read_to_end(&mut reply);
            reply
        });
        let result = serve(|| Ok(server), facts, store);
        let reply = thread.join().unwrap();
        (result, reply)
    }

    fn line(request: Request) -> Zeroizing<Vec<u8>> {
        encode_request(&Envelope {
            interactive: false,
            request,
        })
        .unwrap()
    }

    fn doc(text: &str) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(text.as_bytes().to_vec())
    }

    #[test]
    fn a_write_then_a_read_of_the_same_profile_round_trip() {
        let store = MemoryItemStore::new();
        let write = line(Request::Write {
            profile: "zipline".into(),
            name: "token".into(),
            mode: WriteMode::Create,
            document: doc("{\"v\":2}"),
        });
        let (result, reply) = exchange(&Facts::good(), &store, &write);
        assert_eq!(result, Ok(()));
        assert!(matches!(decode_reply(&reply).unwrap(), Reply::Done));

        let read = line(Request::Read {
            profile: "zipline".into(),
            name: None,
        });
        let (result, reply) = exchange(&Facts::good(), &store, &read);
        assert_eq!(result, Ok(()));
        let Reply::Items(items) = decode_reply(&reply).unwrap() else {
            panic!("not items");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(&**items[0].document, b"{\"v\":2}");
    }

    #[test]
    fn a_read_never_returns_an_item_of_another_profile() {
        let store = MemoryItemStore::new();
        store.create("zipline", "token", b"mine").unwrap();
        store.create("other", "token", b"theirs").unwrap();
        store.create("zipline-2", "token", b"prefix").unwrap();
        let read = line(Request::Read {
            profile: "zipline".into(),
            name: None,
        });
        let (_, reply) = exchange(&Facts::good(), &store, &read);
        let Reply::Items(items) = decode_reply(&reply).unwrap() else {
            panic!("not items");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(&**items[0].document, b"mine");
        let text = String::from_utf8_lossy(&reply);
        assert!(!text.contains("theirs") && !text.contains("prefix"), "{text}");
    }

    #[test]
    fn status_gives_names_and_no_document() {
        let store = MemoryItemStore::new();
        store.create("p", "n", b"SECRET-DOCUMENT").unwrap();
        let (_, reply) = exchange(&Facts::good(), &store, &line(Request::Status));
        let Reply::Names(names) = decode_reply(&reply).unwrap() else {
            panic!("not names");
        };
        assert_eq!(names.len(), 1);
        assert!(!String::from_utf8_lossy(&reply).contains("SECRET"));
    }

    #[test]
    fn a_failure_of_the_store_is_an_answer_not_a_refusal() {
        let store = MemoryItemStore::new();
        store.fail_with(Failure::new(FailureKind::Denied, "refused at the prompt"));
        let read = line(Request::Read {
            profile: "p".into(),
            name: None,
        });
        let (result, reply) = exchange(&Facts::good(), &store, &read);
        assert_eq!(result, Ok(()));
        let Reply::Failed(f) = decode_reply(&reply).unwrap() else {
            panic!("not a failure");
        };
        assert_eq!(f.kind, FailureKind::Denied);
    }

    #[test]
    fn the_interactive_flag_reaches_the_store() {
        let store = MemoryItemStore::new();
        let request = encode_request(&Envelope {
            interactive: true,
            request: Request::Status,
        })
        .unwrap();
        exchange(&Facts::good(), &store, &request);
        assert!(store.interactive());
    }

    #[test]
    fn a_refused_launch_does_not_touch_the_store_nor_answer() {
        let store = MemoryItemStore::new();
        let facts = Facts {
            code: Err(Unverified("not kropa-app")),
            ..Facts::good()
        };
        let (client, server) = UnixStream::pair().unwrap();
        // The request is never read: nothing is written to the client either.
        let result = serve(|| Ok(server), &facts, &store);
        assert_eq!(result, Err(Refusal::PeerCodeRejected));
        assert_eq!(store.calls(), 0);
        let mut reply = Vec::new();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut client = client;
        client.read_to_end(&mut reply).unwrap();
        assert!(reply.is_empty());
    }

    #[test]
    fn a_bad_request_does_not_touch_the_store() {
        let store = MemoryItemStore::new();
        for (bytes, refusal) in [
            (&b"not json\n"[..], Refusal::BadRequest),
            (b"{\"v\":1,\"op\":\"dump\"}\n", Refusal::UnknownOperation),
            (
                b"{\"v\":1,\"op\":\"status\"}\n{\"v\":1,\"op\":\"status\"}\n",
                Refusal::BadRequest,
            ),
            (b"{\"v\":1,\"op\":\"status\"}", Refusal::BadRequest),
            (b"", Refusal::BadRequest),
        ] {
            let (result, reply) = exchange(&Facts::good(), &store, bytes);
            assert_eq!(result, Err(refusal), "{}", String::from_utf8_lossy(bytes));
            assert!(reply.is_empty());
        }
        let too_long = vec![b'a'; 70_000];
        let (result, _) = exchange(&Facts::good(), &store, &too_long);
        assert_eq!(result, Err(Refusal::BadRequest));
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn a_parent_that_ends_before_the_operation_leaves_the_store_untouched() {
        let store = MemoryItemStore::new();
        // 500 for the launch checks, then the parent is gone (launchd adopted the broker).
        let facts = Facts::with_parents(vec![500, 1]);
        let write = line(Request::Write {
            profile: "p".into(),
            name: "n".into(),
            mode: WriteMode::Create,
            document: doc("{}"),
        });
        let (result, reply) = exchange(&facts, &store, &write);
        assert_eq!(result, Err(Refusal::ParentGone));
        assert_eq!(store.calls(), 0);
        assert!(reply.is_empty());
    }

    #[test]
    fn a_parent_that_ends_during_the_operation_gets_no_answer() {
        let store = MemoryItemStore::new();
        store.create("p", "n", b"{}").unwrap();
        // launch check, before the operation, then gone before the answer.
        let facts = Facts::with_parents(vec![500, 500, 1]);
        let read = line(Request::Read {
            profile: "p".into(),
            name: None,
        });
        let (result, reply) = exchange(&facts, &store, &read);
        assert_eq!(result, Err(Refusal::ParentGone));
        assert!(reply.is_empty());
    }

    #[test]
    fn serve_admitted_answers_a_request_over_any_connected_pair() {
        let store = MemoryItemStore::new();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.write_all(&line(Request::Status)).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        serve_admitted(&mut server, Admitted { parent: 500 }, &Facts::good(), &store).unwrap();
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).unwrap();
        assert!(matches!(decode_reply(&reply).unwrap(), Reply::Names(_)));
    }
}
