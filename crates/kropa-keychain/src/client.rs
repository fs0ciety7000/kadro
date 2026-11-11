// SPDX-License-Identifier: GPL-3.0-or-later

//! The side of `kropa-app`: an [`ItemStore`] that asks the broker (`kropa __store-broker`) for
//! everything and never reads the Keychain itself (decision U1, `docs/security/secrets.md`
//! section 2.2).
//!
//! One broker is started per operation, with a new socket pair ([`crate::spawn`]); the client
//! writes its one request, closes its writing end, and reads the reply to the end of file. When the
//! broker refuses it answers nothing and ends with an exit status that says why
//! ([`Refusal::exit_code`]); the client turns that into [`FailureKind::BrokerRefused`] (or
//! [`FailureKind::BrokerUnavailable`] when the broker itself is not signed with the project
//! certificate and so serves nobody).
//!
//! Before it starts anything the client checks that the program it is about to start is the
//! project's broker: signed with the same certificate as itself, under the identifier of `kropa`
//! (macOS). A client that is not signed with that certificate cannot say so, and refuses to start a
//! broker at all.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::guard::Refusal;
use crate::item::{Failure, FailureKind, Item, ItemName, ItemStore};
use crate::spawn::{Spawned, spawn_with_channel};
use crate::wire::{
    Envelope, MAX_REPLY_LEN, Reply, Request, WriteMode, decode_reply, encode_request,
};

/// The argument that makes `kropa` a broker.
pub const BROKER_ARGUMENT: &str = "__store-broker";

/// Deadlines of a call.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// A call that shows nothing to the user.
    pub call: Duration,
    /// A call that may show a Keychain prompt (the broker's own limit is 120 s).
    pub prompt: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            call: Duration::from_secs(15),
            prompt: Duration::from_secs(125),
        }
    }
}

/// The Keychain, as served by the broker.
pub struct BrokerClient {
    broker: PathBuf,
    arguments: Vec<String>,
    check_binary: bool,
    interactive: AtomicBool,
    timeouts: Timeouts,
}

fn unavailable(detail: impl Into<String>) -> Failure {
    Failure::new(FailureKind::BrokerUnavailable, detail)
}

impl BrokerClient {
    /// A client of the broker at `broker` (the `kropa` binary), which is checked on every call.
    #[must_use]
    pub fn new(broker: impl Into<PathBuf>) -> Self {
        Self {
            broker: broker.into(),
            arguments: vec![BROKER_ARGUMENT.to_owned()],
            check_binary: true,
            interactive: AtomicBool::new(false),
            timeouts: Timeouts::default(),
        }
    }

    /// A client of the `kropa` binary that sits next to the running one (in `Kropa.app`).
    ///
    /// # Errors
    /// [`FailureKind::BrokerUnavailable`] when the path of the running binary is unknown.
    pub fn beside_current_exe() -> Result<Self, Failure> {
        let me = std::env::current_exe()
            .map_err(|_| unavailable("the path of this program is unknown"))?;
        Ok(Self::new(me.with_file_name("kropa")))
    }

    /// Does not check the signature of the broker's binary before starting it, and starts it with
    /// `arguments`. For tests that start a stand-in broker; the real broker is never reachable
    /// that way, since it checks its caller itself.
    #[doc(hidden)]
    #[must_use]
    pub fn for_tests(mut self, arguments: &[&str], timeouts: Timeouts) -> Self {
        self.arguments = arguments.iter().map(|a| (*a).to_owned()).collect();
        self.check_binary = false;
        self.timeouts = timeouts;
        self
    }

    fn call(&self, request: Request) -> Result<Reply, Failure> {
        if self.check_binary {
            check_broker_binary(&self.broker)?;
        }
        let interactive = self.interactive.load(Ordering::Relaxed);
        let line = encode_request(&Envelope {
            interactive,
            request,
        })?;
        let arguments: Vec<&str> = self.arguments.iter().map(String::as_str).collect();
        let Spawned { mut child, mut channel } = spawn_with_channel(&self.broker, &arguments)
            .map_err(|e| unavailable(format!("cannot start the Keychain broker: {}", e.kind())))?;
        let timeout = if interactive {
            self.timeouts.prompt
        } else {
            self.timeouts.call
        };
        let _ = channel.set_read_timeout(Some(timeout));
        let _ = channel.set_write_timeout(Some(timeout));
        // A broker that refused has already gone: the write may fail, and what matters then is its
        // exit status, read below.
        let _ = channel
            .write_all(&line)
            .and_then(|()| channel.shutdown(Shutdown::Write));
        drop(line);
        let mut answer = Zeroizing::new(Vec::with_capacity(4096));
        let read = channel
            .by_ref()
            .take(MAX_REPLY_LEN as u64 + 1)
            .read_to_end(&mut answer);
        let status = reap(&mut child);
        if let Err(error) = &read
            && matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
        {
            return Err(Failure::new(
                FailureKind::Timeout,
                "the Keychain broker did not answer in time",
            ));
        }
        if answer.is_empty() {
            return Err(no_answer(status));
        }
        decode_reply(&answer)
    }

    fn items(&self, request: Request) -> Result<Reply, Failure> {
        match self.call(request)? {
            Reply::Failed(failure) => Err(failure),
            other => Ok(other),
        }
    }
}

fn unexpected() -> Failure {
    unavailable("the Keychain broker answered something else than what was asked")
}

/// What a broker that said nothing means, from its exit status.
fn no_answer(status: Option<i32>) -> Failure {
    match status.and_then(Refusal::from_exit_code) {
        Some(Refusal::BrokerNotProjectSigned) => unavailable(
            "the Keychain broker is disabled: it is not signed with the project certificate",
        ),
        Some(refusal) => Failure::new(
            FailureKind::BrokerRefused,
            format!("the Keychain broker refused this caller ({})", refusal.reason()),
        ),
        None => unavailable(match status {
            Some(code) => format!("the Keychain broker ended without an answer (status {code})"),
            None => "the Keychain broker was stopped without an answer".to_owned(),
        }),
    }
}

/// Waits for the broker to end (it does as soon as it has answered), and kills it if it does not.
fn reap(child: &mut Child) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                return child.wait().ok().and_then(|s| s.code());
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn check_broker_binary(path: &Path) -> Result<(), Failure> {
    crate::apple::verify_broker_binary(path).map_err(|e| {
        unavailable(format!(
            "the Keychain broker is disabled: {e} (a build signed with the project certificate is needed)"
        ))
    })
}

#[cfg(not(target_os = "macos"))]
fn check_broker_binary(_path: &Path) -> Result<(), Failure> {
    Err(unavailable("the Keychain broker exists on macOS only"))
}

impl ItemStore for BrokerClient {
    fn read(&self, profile: &str, name: Option<&str>) -> Result<Vec<Item>, Failure> {
        match self.items(Request::Read {
            profile: profile.to_owned(),
            name: name.map(str::to_owned),
        })? {
            Reply::Items(items) => Ok(items),
            _ => Err(unexpected()),
        }
    }

    fn list(&self) -> Result<Vec<ItemName>, Failure> {
        match self.items(Request::Status)? {
            Reply::Names(names) => Ok(names),
            _ => Err(unexpected()),
        }
    }

    fn create(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        write(self, profile, name, WriteMode::Create, document)
    }

    fn replace(&self, profile: &str, name: &str, document: &[u8]) -> Result<(), Failure> {
        write(self, profile, name, WriteMode::Replace, document)
    }

    fn remove(&self, profile: &str, name: &str) -> Result<bool, Failure> {
        match self.items(Request::Remove {
            profile: profile.to_owned(),
            name: name.to_owned(),
        })? {
            Reply::Removed(removed) => Ok(removed),
            _ => Err(unexpected()),
        }
    }

    fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::Relaxed);
    }
}

fn write(
    client: &BrokerClient,
    profile: &str,
    name: &str,
    mode: WriteMode,
    document: &[u8],
) -> Result<(), Failure> {
    match client.items(Request::Write {
        profile: profile.to_owned(),
        name: name.to_owned(),
        mode,
        document: Zeroizing::new(document.to_vec()),
    })? {
        Reply::Done => Ok(()),
        _ => Err(unexpected()),
    }
}

#[cfg(test)]
mod tests {
    use super::{BrokerClient, Timeouts, no_answer};
    use crate::guard::Refusal;
    use crate::item::{FailureKind, ItemStore};

    #[test]
    fn a_missing_broker_is_unavailable_not_a_hang() {
        let client = BrokerClient::new("/nonexistent/kropa")
            .for_tests(&["__store-broker"], Timeouts::default());
        let error = client.list().unwrap_err();
        assert_eq!(error.kind, FailureKind::BrokerUnavailable);
    }

    #[test]
    fn an_unchecked_client_is_never_the_default() {
        // On this OS the real check is either the macOS one (not signed: unavailable) or "macOS only".
        let client = BrokerClient::new("/bin/true");
        assert_eq!(client.list().unwrap_err().kind, FailureKind::BrokerUnavailable);
    }

    #[test]
    fn exit_statuses_become_failures() {
        let refused = no_answer(Some(i32::from(Refusal::PeerCodeRejected.exit_code())));
        assert_eq!(refused.kind, FailureKind::BrokerRefused);
        assert!(refused.detail.contains("peer_code_rejected"), "{}", refused.detail);
        let unsigned = no_answer(Some(i32::from(Refusal::BrokerNotProjectSigned.exit_code())));
        assert_eq!(unsigned.kind, FailureKind::BrokerUnavailable);
        assert_eq!(no_answer(Some(0)).kind, FailureKind::BrokerUnavailable);
        assert_eq!(no_answer(None).kind, FailureKind::BrokerUnavailable);
    }
}
