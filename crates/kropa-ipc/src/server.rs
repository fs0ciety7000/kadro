// SPDX-License-Identifier: GPL-3.0-or-later

//! The server side, for `kropa-gtk` and `kropa-app`: a Unix socket in a private directory, owned by
//! the process that holds `app.lock`.
//!
//! The lock is what decides who may bind: an orphan socket (a crashed server) is removed only by the
//! process that holds the lock, and a second server fails instead of stealing the socket.

use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::fs::{FlockOperation, OFlags, flock};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, watch};
use tokio::time::timeout;

use crate::dir::{LOCK_NAME, check_socket_len, current_uid, ensure_dir, socket_path};
use crate::error::IpcError;
use crate::proto::{HelloParams, HelloReply, Line, PROTOCOL, RpcError, code, encode, read_line};

/// A boxed future, so that [`Handler`] can be used as a trait object.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the application does with a request. `hello` is answered by the server itself.
pub trait Handler: Send + Sync {
    /// Runs `method`. An unknown method is `Err(RpcError::method_not_found(..))`.
    fn call(&self, method: &str, params: Value) -> BoxFuture<'_, Result<Value, RpcError>>;
}

impl RpcError {
    /// The error for a method that does not exist (exit code 1: it is a bug of the client).
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            1,
            "internal",
            Some("unknown_method"),
            &format!("unknown method {method}"),
        )
    }
}

/// What the server announces and how it protects itself.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// `kropa-gtk` or `kropa-app`.
    pub server: &'static str,
    /// Version of the application.
    pub version: String,
    /// Methods the handler answers (announced by `hello`).
    pub methods: Vec<String>,
    /// Optional features (`layer-shell`).
    pub features: Vec<String>,
    /// UID that every client must have. The process UID, except in tests.
    pub peer_uid: u32,
    /// A connection that sends nothing for this long is closed.
    pub idle_timeout: Duration,
}

impl ServerConfig {
    /// A configuration for this process's user, with a 60 s idle timeout.
    #[must_use]
    pub fn new(server: &'static str, version: &str) -> Self {
        Self {
            server,
            version: version.to_owned(),
            methods: Vec::new(),
            features: Vec::new(),
            peer_uid: current_uid(),
            idle_timeout: Duration::from_secs(60),
        }
    }
}

/// More connections than this at once are closed on arrival.
const MAX_CONNECTIONS: usize = 8;

/// A bound, locked socket that has not started to answer yet.
pub struct Server {
    listener: std::os::unix::net::UnixListener,
    // Held for the life of the server: dropping it releases the lock.
    lock: File,
    socket: PathBuf,
    config: Arc<ServerConfig>,
    handler: Arc<dyn Handler>,
    stop: watch::Sender<bool>,
}

/// Asks a running server to stop; cheap to clone, usable from a handler (`app.quit`).
#[derive(Debug, Clone)]
pub struct Shutdown(watch::Sender<bool>);

impl Shutdown {
    /// Stops accepting and closes the connections.
    pub fn trigger(&self) {
        self.0.send_replace(true);
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Creates the directory (`0700`), takes `app.lock`, removes an orphan socket and binds a new one
    /// (mode `0600`).
    ///
    /// # Errors
    /// `unsafe_runtime_dir` for a directory or a socket path that is not safe, `daemon_start_failed`
    /// when another server holds the lock.
    pub fn bind(
        dir: &Path,
        config: ServerConfig,
        handler: Arc<dyn Handler>,
    ) -> Result<Self, IpcError> {
        let socket = socket_path(dir);
        check_socket_len(&socket)?;
        ensure_dir(dir, current_uid())?;
        let lock = take_lock(&dir.join(LOCK_NAME))?;
        remove_orphan(&socket)?;
        let listener =
            std::os::unix::net::UnixListener::bind(&socket).map_err(|e| IpcError::io("bind", e))?;
        // The directory is already private; the mode of the file is a second fence.
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| IpcError::io("chmod socket", e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| IpcError::io("nonblocking", e))?;
        Ok(Self {
            listener,
            lock,
            socket,
            config: Arc::new(config),
            handler,
            stop: watch::channel(false).0,
        })
    }

    /// Path of the socket.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// A handle that stops [`Server::run`].
    #[must_use]
    pub fn shutdown_handle(&self) -> Shutdown {
        Shutdown(self.stop.clone())
    }

    /// Answers connections until [`Shutdown::trigger`]. Removes the socket on the way out.
    ///
    /// # Errors
    /// [`IpcError::Io`] when the listener cannot be used.
    pub async fn run(self) -> Result<(), IpcError> {
        let Self {
            listener,
            lock,
            socket,
            config,
            handler,
            stop,
        } = self;
        let listener = UnixListener::from_std(listener).map_err(|e| IpcError::io("listener", e))?;
        let mut stopped = stop.subscribe();
        let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let result = loop {
            tokio::select! {
                _ = stopped.wait_for(|s| *s) => break Ok(()),
                accepted = listener.accept() => {
                    let stream = match accepted {
                        Ok((stream, _)) => stream,
                        // A connection that fell before accept(): not the end of the server.
                        Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => continue,
                        Err(e) => break Err(IpcError::io("accept", e)),
                    };
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                        continue; // too many clients: closed on arrival
                    };
                    let (config, handler) = (Arc::clone(&config), Arc::clone(&handler));
                    tokio::spawn(async move {
                        let _permit = permit;
                        connection(stream, &config, handler.as_ref()).await;
                    });
                }
            }
        };
        // Still under the lock: nobody else may bind before the file is gone.
        let _ = std::fs::remove_file(&socket);
        drop(lock);
        result
    }
}

fn take_lock(path: &Path) -> Result<File, IpcError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
        .map_err(|e| IpcError::io("open the lock", e))?;
    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(file),
        Err(rustix::io::Errno::WOULDBLOCK) => Err(IpcError::StartFailed(
            "another instance of the application holds the lock".to_owned(),
        )),
        Err(e) => Err(IpcError::io("lock", io::Error::from(e))),
    }
}

/// Removes a socket left by a server that died, with the lock held. Anything else at that path is
/// not ours to remove.
fn remove_orphan(socket: &Path) -> Result<(), IpcError> {
    match std::fs::symlink_metadata(socket) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(IpcError::io("inspect the socket", e)),
        Ok(meta) if meta.file_type().is_socket() => {
            std::fs::remove_file(socket).map_err(|e| IpcError::io("remove the old socket", e))
        }
        Ok(_) => Err(IpcError::UnsafeRuntimeDir {
            path: socket.to_owned(),
            why: "something that is not a socket is in its place".to_owned(),
        }),
    }
}

/// Runs a server on a thread of its own, with a tokio runtime of its own, for an application whose
/// main thread is something else (the GTK or `AppKit` loop).
///
/// # Errors
/// The errors of [`Server::bind`] (reported here, before the thread starts) and
/// `daemon_start_failed` when the thread cannot be created.
pub fn serve(
    dir: &Path,
    config: ServerConfig,
    handler: Arc<dyn Handler>,
) -> Result<ServerThread, IpcError> {
    let server = Server::bind(dir, config, handler)?;
    let shutdown = server.shutdown_handle();
    let join = std::thread::Builder::new()
        .name("kropa-ipc".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| IpcError::io("runtime", e))?;
            runtime.block_on(server.run())
        })
        .map_err(|e| IpcError::StartFailed(format!("cannot start the IPC thread: {e}")))?;
    Ok(ServerThread {
        shutdown,
        join: Some(join),
    })
}

/// A server running on its own thread. Dropping it stops the server and waits for the thread.
#[derive(Debug)]
pub struct ServerThread {
    shutdown: Shutdown,
    join: Option<JoinHandle<Result<(), IpcError>>>,
}

impl ServerThread {
    /// A handle to give to handlers that must be able to stop the server.
    #[must_use]
    pub fn shutdown_handle(&self) -> Shutdown {
        self.shutdown.clone()
    }

    /// Waits for the thread to end (after [`Shutdown::trigger`]) and returns how it ended.
    ///
    /// # Errors
    /// What [`Server::run`] returned; a panic of the thread is [`IpcError::StartFailed`].
    pub fn wait(mut self) -> Result<(), IpcError> {
        self.join_inner()
    }

    fn join_inner(&mut self) -> Result<(), IpcError> {
        match self.join.take() {
            Some(join) => join.join().unwrap_or_else(|_| {
                Err(IpcError::StartFailed("the IPC thread panicked".to_owned()))
            }),
            None => Ok(()),
        }
    }
}

impl Drop for ServerThread {
    fn drop(&mut self) {
        self.shutdown.trigger();
        let _ = self.join_inner();
    }
}

async fn write_message(stream: &mut tokio::net::unix::OwnedWriteHalf, message: &Value) -> bool {
    let bytes = match encode(message) {
        Ok(Some(bytes)) => bytes,
        // An answer that does not fit is replaced by an error that does.
        Ok(None) => {
            let error = RpcError::new(
                1,
                "internal",
                Some("answer_too_long"),
                "the answer is too long",
            );
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            match encode(&error_message(&id, &error, code::APPLICATION)) {
                Ok(Some(bytes)) => bytes,
                _ => return false,
            }
        }
        Err(_) => return false,
    };
    stream.write_all(&bytes).await.is_ok()
}

fn error_message(id: &Value, error: &RpcError, rpc_code: i64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": error.wire(rpc_code)})
}

/// One connection, from the check of the peer to the end.
async fn connection(stream: UnixStream, config: &ServerConfig, handler: &dyn Handler) {
    // The first thing: who is calling? Another user gets nothing, not even an error message.
    match stream.peer_cred() {
        Ok(peer) if peer.uid() == config.peer_uid => {}
        _ => return,
    }
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = Vec::new();
    let mut greeted = false;
    loop {
        match timeout(config.idle_timeout, read_line(&mut reader, &mut line)).await {
            Ok(Ok(Line::Complete)) => {}
            // Too long, closed, failed, or silent for too long: the connection ends.
            _ => return,
        }
        let Some(reply) = answer(&line, config, handler, &mut greeted).await else {
            continue;
        };
        let fatal = matches!(reply.close, Close::Yes);
        if !write_message(&mut writer, &reply.message).await || fatal {
            return;
        }
    }
}

enum Close {
    Yes,
    No,
}

struct Reply {
    message: Value,
    close: Close,
}

/// Answer to one line, or `None` for a notification (a request without `id`).
async fn answer(
    line: &[u8],
    config: &ServerConfig,
    handler: &dyn Handler,
    greeted: &mut bool,
) -> Option<Reply> {
    let no_close = |message| {
        Some(Reply {
            message,
            close: Close::No,
        })
    };
    let Ok(request) = serde_json::from_slice::<Value>(line) else {
        let e = RpcError::daemon("ipc_protocol_error", "the request is not JSON");
        return no_close(error_message(&Value::Null, &e, code::PARSE));
    };
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);
    let valid_id = matches!(
        id,
        None | Some(Value::Null | Value::Number(_) | Value::String(_))
    );
    let (Some(method), true, true) = (
        method,
        request.get("jsonrpc") == Some(&json!("2.0")),
        valid_id,
    ) else {
        let e = RpcError::daemon("ipc_protocol_error", "not a JSON-RPC 2.0 request");
        return no_close(error_message(
            id.as_ref().unwrap_or(&Value::Null),
            &e,
            code::INVALID_REQUEST,
        ));
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let outcome = if method == "hello" {
        hello(params, config, greeted)
    } else if *greeted {
        handler.call(method, params).await
    } else {
        Err(RpcError::daemon(
            "ipc_protocol_error",
            "hello is required first",
        ))
    };
    let id = id?; // a notification gets no answer, whatever the outcome
    let close = match &outcome {
        Err(e) if e.reason.as_deref() == Some("ipc_version_mismatch") => Close::Yes,
        _ => Close::No,
    };
    let message = match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(e) => {
            let rpc_code = if e.reason.as_deref() == Some("unknown_method") {
                code::METHOD_NOT_FOUND
            } else {
                code::APPLICATION
            };
            error_message(&id, &e, rpc_code)
        }
    };
    Some(Reply { message, close })
}

fn hello(params: Value, config: &ServerConfig, greeted: &mut bool) -> Result<Value, RpcError> {
    let params: HelloParams = serde_json::from_value(params)
        .map_err(|e| RpcError::daemon("ipc_protocol_error", &format!("malformed hello: {e}")))?;
    if params.protocol != PROTOCOL {
        return Err(RpcError::daemon(
            "ipc_version_mismatch",
            &format!(
                "IPC protocol {PROTOCOL} on the server, {} on the client",
                params.protocol
            ),
        ));
    }
    *greeted = true;
    let reply = HelloReply {
        protocol: PROTOCOL,
        server: config.server.to_owned(),
        version: config.version.clone(),
        methods: config.methods.clone(),
        features: config.features.clone(),
    };
    serde_json::to_value(reply).map_err(|e| RpcError::new(1, "internal", None, &e.to_string()))
}
