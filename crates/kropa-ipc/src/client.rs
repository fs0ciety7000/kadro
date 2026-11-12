// SPDX-License-Identifier: GPL-3.0-or-later

//! The client side: connect, check who answers, `hello`, then calls.
//!
//! The order matters (`docs/security/secrets.md` section 1.2): the directory is checked, the socket is
//! opened, the UID of the peer is compared with ours, and only then is the first byte written.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::timeout;

use crate::dir::{check_socket_len, current_uid, verify_dir};
use crate::error::IpcError;
use crate::proto::{
    HelloParams, HelloReply, Line, PROTOCOL, WireError, encode, read_line, strip_control,
};

/// How long the connection and the `hello` may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a call may take unless [`Client::set_call_timeout`] says otherwise.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to the application, after a successful `hello`.
#[derive(Debug)]
pub struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
    call_timeout: Duration,
    line: Vec<u8>,
    hello: HelloReply,
}

impl Client {
    /// Connects to the socket of this user's application.
    ///
    /// # Errors
    /// [`IpcError`]: `unsafe_runtime_dir`, `daemon_not_running`, `peer_uid_mismatch`,
    /// `ipc_version_mismatch`, `ipc_protocol_error`.
    pub async fn connect(socket: &Path) -> Result<Self, IpcError> {
        Self::connect_as(socket, current_uid()).await
    }

    /// [`Client::connect`] with the UID that the server must have: a hook for the tests, which cannot
    /// be another user without `sudo` (not part of the API). The directory must always belong to this
    /// process's user.
    ///
    /// # Errors
    /// See [`Client::connect`].
    #[doc(hidden)]
    pub async fn connect_as(socket: &Path, peer_uid: u32) -> Result<Self, IpcError> {
        check_socket_len(socket)?;
        if let Some(dir) = socket.parent() {
            verify_dir(dir, current_uid())?;
        }
        Self::connect_checked(socket, peer_uid).await
    }

    /// [`Client::connect_as`] without the check of the directory: a hook for the tests with two real
    /// users (lot 2.1d), where the server's directory belongs to the other one. Never used by the
    /// command line.
    ///
    /// # Errors
    /// See [`Client::connect`], except the directory errors.
    #[doc(hidden)]
    pub async fn connect_as_unchecked_dir(socket: &Path, peer_uid: u32) -> Result<Self, IpcError> {
        check_socket_len(socket)?;
        Self::connect_checked(socket, peer_uid).await
    }

    async fn connect_checked(socket: &Path, peer_uid: u32) -> Result<Self, IpcError> {
        let stream = match timeout(CONNECT_TIMEOUT, UnixStream::connect(socket)).await {
            Err(_) => return Err(IpcError::Protocol("the connection timed out".to_owned())),
            Ok(Ok(stream)) => stream,
            Ok(Err(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(IpcError::NotRunning(socket.to_owned()));
            }
            Ok(Err(e)) => return Err(IpcError::io("connect", e)),
        };
        // Before anything is sent: is the server ours?
        let peer = stream
            .peer_cred()
            .map_err(|e| IpcError::io("peer credentials", e))?;
        if peer.uid() != peer_uid {
            return Err(IpcError::PeerUidMismatch {
                peer: peer.uid(),
                expected: peer_uid,
            });
        }
        let (read, writer) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(read),
            writer,
            next_id: 1,
            call_timeout: CONNECT_TIMEOUT,
            line: Vec::new(),
            hello: HelloReply {
                protocol: 0,
                server: String::new(),
                version: String::new(),
                methods: Vec::new(),
                features: Vec::new(),
            },
        };
        let params = HelloParams {
            protocol: PROTOCOL,
            client: "kropa".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        // A server that speaks another protocol answers with `ipc_version_mismatch` (exit code 14).
        let reply: HelloReply = client.call("hello", params).await?;
        if reply.protocol != PROTOCOL {
            return Err(IpcError::VersionMismatch {
                server: reply.protocol,
                client: PROTOCOL,
            });
        }
        // What the peer says about itself is shown to the user: no control characters.
        client.hello = HelloReply {
            server: strip_control(&reply.server),
            version: strip_control(&reply.version),
            methods: reply.methods.iter().map(|m| strip_control(m)).collect(),
            features: reply.features.iter().map(|f| strip_control(f)).collect(),
            ..reply
        };
        client.call_timeout = CALL_TIMEOUT;
        Ok(client)
    }

    /// What the server said about itself in `hello`.
    #[must_use]
    pub fn server(&self) -> &HelloReply {
        &self.hello
    }

    /// Changes how long a call may wait for its answer (an overlay waits for the user: long).
    pub fn set_call_timeout(&mut self, call_timeout: Duration) {
        self.call_timeout = call_timeout;
    }

    /// Calls a method and waits for its answer.
    ///
    /// # Errors
    /// [`IpcError::Rpc`] when the application answers with an error, [`IpcError::Protocol`] when it
    /// answers badly or not at all in time, [`IpcError::Io`] when the socket fails.
    pub async fn call<P, R>(&mut self, method: &str, params: P) -> Result<R, IpcError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = self.next_id;
        self.next_id += 1;
        let params = serde_json::to_value(params)
            .map_err(|e| IpcError::Protocol(format!("parameters of {method}: {e}")))?;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let bytes = encode(&request)
            .map_err(|e| IpcError::io("encode", e))?
            .ok_or_else(|| IpcError::Protocol(format!("the request {method} is too long")))?;
        let wait = self.call_timeout;
        match timeout(wait, self.exchange(id, &bytes)).await {
            Ok(result) => result,
            Err(_) => Err(IpcError::Protocol(format!(
                "no answer to {method} within {wait:?}"
            ))),
        }
    }

    async fn exchange<R: DeserializeOwned>(
        &mut self,
        id: u64,
        bytes: &[u8],
    ) -> Result<R, IpcError> {
        self.writer
            .write_all(bytes)
            .await
            .map_err(|e| IpcError::io("write", e))?;
        match read_line(&mut self.reader, &mut self.line)
            .await
            .map_err(|e| IpcError::io("read", e))?
        {
            Line::Complete => {}
            Line::Eof => {
                return Err(IpcError::Protocol(
                    "the application closed the connection".to_owned(),
                ));
            }
            Line::TooLong => {
                return Err(IpcError::Protocol("the answer is too long".to_owned()));
            }
        }
        let message: Value = serde_json::from_slice(&self.line)
            .map_err(|e| IpcError::Protocol(format!("the answer is not JSON: {e}")))?;
        if message.get("jsonrpc") != Some(&json!("2.0")) || message.get("id") != Some(&json!(id)) {
            return Err(IpcError::Protocol(
                "the answer is not the one expected".to_owned(),
            ));
        }
        if let Some(error) = message.get("error") {
            let wire: WireError = serde_json::from_value(error.clone())
                .map_err(|e| IpcError::Protocol(format!("malformed error: {e}")))?;
            return Err(IpcError::Rpc(wire.into_rpc()));
        }
        let result = message.get("result").cloned().unwrap_or(Value::Null);
        serde_json::from_value(result)
            .map_err(|e| IpcError::Protocol(format!("unexpected result: {e}")))
    }
}
