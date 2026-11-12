// SPDX-License-Identifier: GPL-3.0-or-later

//! Real client and real server in a temporary directory (lot 2.1a).

use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener as StdListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kropa_ipc::dir::{ensure_dir, socket_path};
use kropa_ipc::{
    BoxFuture, Client, Handler, IpcError, Launcher, MAX_LINE, PROTOCOL, ProbeState, RpcError,
    ServerConfig, ServerThread, Spawn, current_uid, ensure_running_in, probe_in, serve,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const FAKE: &str = env!("CARGO_BIN_EXE_kropa-ipc-fake");

struct Echo;

impl Handler for Echo {
    fn call(&self, method: &str, params: Value) -> BoxFuture<'_, Result<Value, RpcError>> {
        let method = method.to_owned();
        Box::pin(async move {
            match method.as_str() {
                "echo" => Ok(params),
                "cancel" => Err(RpcError::new(
                    3,
                    "cancelled",
                    Some("overlay_escape"),
                    "Escape",
                )),
                other => Err(RpcError::method_not_found(other)),
            }
        })
    }
}

/// A private directory like `$XDG_RUNTIME_DIR` and the `kropa` directory inside it.
fn runtime() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kropa");
    (tmp, dir)
}

fn start(dir: &Path) -> ServerThread {
    let mut config = ServerConfig::new("kropa-gtk", "9.9.9");
    config.methods = vec!["echo".into(), "cancel".into()];
    config.features = vec!["layer-shell".into()];
    serve(dir, config, Arc::new(Echo)).unwrap()
}

fn reason(e: &IpcError) -> &str {
    e.reason().unwrap()
}

#[tokio::test]
async fn hello_then_calls_work_and_errors_keep_their_exit_code() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut client = Client::connect(&socket_path(&dir)).await.unwrap();
    assert_eq!(client.server().server, "kropa-gtk");
    assert_eq!(client.server().version, "9.9.9");
    assert_eq!(client.server().protocol, PROTOCOL);
    assert_eq!(client.server().features, ["layer-shell"]);
    let back: Value = client.call("echo", json!({"a": [1, 2]})).await.unwrap();
    assert_eq!(back, json!({"a": [1, 2]}));
    let err = client.call::<_, Value>("cancel", ()).await.unwrap_err();
    assert_eq!((err.exit_code(), reason(&err)), (3, "overlay_escape"));
    let err = client.call::<_, Value>("nope", ()).await.unwrap_err();
    assert_eq!(err.exit_code(), 1);
    // The connection survives an error.
    let again: Value = client.call("echo", 7).await.unwrap();
    assert_eq!(again, json!(7));
}

#[tokio::test]
async fn the_directory_and_the_socket_are_private() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&socket_path(&dir)), 0o600);
}

/// A server that is not ours: nothing may be written to it.
#[tokio::test]
async fn a_server_of_another_user_receives_nothing() {
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    let listener = StdListener::bind(socket_path(&dir)).unwrap();
    let spy = std::thread::spawn(move || {
        use std::io::Read;
        let (mut stream, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        stream.read_to_end(&mut seen).unwrap();
        seen
    });
    // The uid is injected: this server runs as the current user, so it is "another" one for uid + 1.
    let err = Client::connect_as(&socket_path(&dir), current_uid() + 1)
        .await
        .unwrap_err();
    assert!(matches!(err, IpcError::PeerUidMismatch { .. }), "{err}");
    assert_eq!((err.exit_code(), reason(&err)), (14, "peer_uid_mismatch"));
    assert!(
        spy.join().unwrap().is_empty(),
        "bytes were sent to a foreign server"
    );
}

#[tokio::test]
async fn a_client_of_another_user_is_dropped_without_an_answer() {
    let (_tmp, dir) = runtime();
    let mut config = ServerConfig::new("kropa-gtk", "1");
    config.peer_uid = current_uid() + 1;
    let _server = serve(&dir, config, Arc::new(Echo)).unwrap();
    let mut stream = UnixStream::connect(socket_path(&dir)).await.unwrap();
    let _ = stream
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\"}\n")
        .await;
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    assert!(
        buf.is_empty(),
        "the server answered a foreign user: {buf:?}"
    );
}

async fn greeted(dir: &Path) -> UnixStream {
    let mut stream = UnixStream::connect(socket_path(dir)).await.unwrap();
    let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "hello",
        "params": {"protocol": PROTOCOL, "client": "kropa", "version": "0"}});
    stream
        .write_all(format!("{hello}\n").as_bytes())
        .await
        .unwrap();
    let mut byte = [0u8; 1];
    // The answer ends with a line feed: read until it.
    loop {
        stream.read_exact(&mut byte).await.unwrap();
        if byte[0] == b'\n' {
            break;
        }
    }
    stream
}

#[tokio::test]
async fn a_line_over_64_kib_closes_the_connection() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut stream = greeted(&dir).await;
    let line = vec![b'a'; MAX_LINE + 1];
    let _ = stream.write_all(&line).await;
    let _ = stream.write_all(b"\n").await;
    let mut rest = Vec::new();
    // End of stream, or a reset when unread bytes were still in the socket: closed either way.
    let _ = stream.read_to_end(&mut rest).await;
    assert!(rest.is_empty(), "no answer to an oversized line: {rest:?}");
    // The server itself is fine.
    Client::connect(&socket_path(&dir)).await.unwrap();
}

#[tokio::test]
async fn a_line_of_exactly_64_kib_is_still_answered() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut client = Client::connect(&socket_path(&dir)).await.unwrap();
    let overhead =
        serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": 2, "method": "echo", "params": ""}))
            .unwrap()
            .len();
    let payload = "x".repeat(MAX_LINE - overhead);
    let back: String = client.call("echo", payload.clone()).await.unwrap();
    assert_eq!(back.len(), payload.len());
    // A longer request is refused by the client before it sends anything.
    let err = client
        .call::<_, Value>("echo", "x".repeat(MAX_LINE))
        .await
        .unwrap_err();
    assert_eq!(reason(&err), "ipc_protocol_error");
}

#[tokio::test]
async fn hello_in_protocol_2_is_refused_with_code_14_and_the_connection_closes() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut stream = UnixStream::connect(socket_path(&dir)).await.unwrap();
    let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "hello",
        "params": {"protocol": 2, "client": "kropa", "version": "9"}});
    stream
        .write_all(format!("{hello}\n").as_bytes())
        .await
        .unwrap();
    let mut all = String::new();
    stream.read_to_string(&mut all).await.unwrap();
    let answer: Value = serde_json::from_str(all.trim_end()).unwrap();
    assert_eq!(answer["error"]["data"]["exit_code"], 14);
    assert_eq!(answer["error"]["data"]["reason"], "ipc_version_mismatch");
}

#[tokio::test]
async fn a_client_in_protocol_2_gets_code_14_from_the_real_server() {
    // The same refusal as seen from `Client`: a server that speaks protocol 1 answers a client that
    // would announce 2 (simulated on the wire above); here, the client side of a protocol-2 server.
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    let listener = tokio::net::UnixListener::bind(socket_path(&dir)).unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf).await.unwrap();
        let reply = json!({"jsonrpc": "2.0", "id": 1, "result":
            {"protocol": 2, "server": "kropa-gtk", "version": "9", "methods": []}});
        stream
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
    });
    let err = Client::connect(&socket_path(&dir)).await.unwrap_err();
    assert!(
        matches!(err, IpcError::VersionMismatch { server: 2, .. }),
        "{err}"
    );
    assert_eq!(
        (err.exit_code(), reason(&err)),
        (14, "ipc_version_mismatch")
    );
}

#[tokio::test]
async fn nothing_works_before_hello() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut stream = UnixStream::connect(socket_path(&dir)).await.unwrap();
    stream
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":1}\n")
        .await
        .unwrap();
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).await.unwrap();
    let answer: Value = serde_json::from_slice(&buf[..n]).unwrap();
    assert_eq!(answer["error"]["data"]["reason"], "ipc_protocol_error");
}

#[tokio::test]
async fn malformed_requests_get_standard_errors_and_the_connection_goes_on() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let mut stream = greeted(&dir).await;
    let mut ask = async |text: &str| -> Value {
        stream
            .write_all(format!("{text}\n").as_bytes())
            .await
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        serde_json::from_slice(&buf[..n]).unwrap()
    };
    assert_eq!(ask("not json").await["error"]["code"], -32700);
    assert_eq!(
        ask(r#"{"id":3,"method":"echo"}"#).await["error"]["code"],
        -32600
    );
    assert_eq!(
        ask(r#"{"jsonrpc":"2.0","id":4,"method":"echo","params":5}"#).await["result"],
        5
    );
}

#[test]
fn a_second_server_cannot_take_the_socket_and_an_orphan_is_replaced() {
    let (_tmp, dir) = runtime();
    let first = start(&dir);
    let err = serve(&dir, ServerConfig::new("kropa-gtk", "1"), Arc::new(Echo)).unwrap_err();
    assert_eq!(reason(&err), "daemon_start_failed");
    // The first one still answers.
    assert_eq!(probe_in(&dir).state, ProbeState::Listening);
    drop(first);
    assert_eq!(
        probe_in(&dir).state,
        ProbeState::Absent,
        "the socket is removed on exit"
    );

    // A server that died leaves its file behind: a new one removes it, under the lock.
    let listener = StdListener::bind(socket_path(&dir)).unwrap();
    drop(listener);
    assert!(socket_path(&dir).exists());
    assert_eq!(probe_in(&dir).state, ProbeState::Orphan);
    let _second = start(&dir);
    assert_eq!(probe_in(&dir).state, ProbeState::Listening);
}

#[test]
fn something_that_is_not_a_socket_is_never_removed() {
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    std::fs::write(socket_path(&dir), "precious").unwrap();
    let err = serve(&dir, ServerConfig::new("kropa-gtk", "1"), Arc::new(Echo)).unwrap_err();
    assert_eq!(reason(&err), "unsafe_runtime_dir");
    assert_eq!(
        std::fs::read_to_string(socket_path(&dir)).unwrap(),
        "precious"
    );
}

#[test]
fn the_server_refuses_an_unsafe_directory() {
    let (_tmp, dir) = runtime();
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = serve(&dir, ServerConfig::new("kropa-gtk", "1"), Arc::new(Echo)).unwrap_err();
    assert_eq!(reason(&err), "unsafe_runtime_dir");
    assert_eq!(probe_in(&dir).state, ProbeState::Unsafe);
}

#[tokio::test]
async fn the_client_refuses_a_directory_open_to_others_before_connecting() {
    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = Client::connect(&socket_path(&dir)).await.unwrap_err();
    assert_eq!((err.exit_code(), reason(&err)), (14, "unsafe_runtime_dir"));
}

#[test]
fn shutdown_removes_the_socket() {
    let (_tmp, dir) = runtime();
    let server = start(&dir);
    server.shutdown_handle().trigger();
    server.wait().unwrap();
    assert!(!socket_path(&dir).exists());
}

// ---- launching ----

struct Counting<'a> {
    count: &'a Cell<u32>,
    result: fn() -> Result<(), IpcError>,
}

impl Launcher for Counting<'_> {
    fn launch(&self, _budget: Duration) -> Result<(), IpcError> {
        self.count.set(self.count.get() + 1);
        (self.result)()
    }
}

#[tokio::test]
async fn a_launcher_that_fails_is_daemon_start_failed() {
    let (_tmp, dir) = runtime();
    let count = Cell::new(0);
    let launcher = Counting {
        count: &count,
        result: || Err(IpcError::StartFailed("no way".into())),
    };
    let err = ensure_running_in(&dir, &launcher, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!((err.exit_code(), reason(&err)), (14, "daemon_start_failed"));
    assert_eq!(count.get(), 1);
}

#[tokio::test]
async fn a_launch_that_never_listens_is_a_timeout() {
    let (_tmp, dir) = runtime();
    let count = Cell::new(0);
    let launcher = Counting {
        count: &count,
        result: || Ok(()),
    };
    let err = ensure_running_in(&dir, &launcher, Duration::from_millis(300))
        .await
        .unwrap_err();
    assert_eq!(
        (err.exit_code(), reason(&err)),
        (14, "daemon_start_timeout")
    );
    assert_eq!(count.get(), 1);
}

#[tokio::test]
async fn nothing_is_launched_into_an_unsafe_directory_or_when_the_server_is_up() {
    let (_tmp, dir) = runtime();
    let count = Cell::new(0);
    let launcher = Counting {
        count: &count,
        result: || Ok(()),
    };
    let _server = start(&dir);
    ensure_running_in(&dir, &launcher, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(count.get(), 0, "already listening: no launch");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = ensure_running_in(&dir, &launcher, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(reason(&err), "unsafe_runtime_dir");
    assert_eq!(count.get(), 0);
}

#[tokio::test]
async fn spawn_starts_the_program_and_the_client_reaches_it() {
    let (_tmp, dir) = runtime();
    let spawn = Spawn::new(
        Path::new(FAKE),
        &["--dir", dir.to_str().unwrap(), "--idle", "20"],
    );
    let mut client = ensure_running_in(&dir, &spawn, Duration::from_secs(5))
        .await
        .unwrap();
    let status: Value = client.call("app.status", ()).await.unwrap();
    assert_eq!(status["protocol"], PROTOCOL);
    let _: Value = client.call("app.quit", ()).await.unwrap();
    // The fake closes its socket on the way out.
    for _ in 0..100 {
        if !socket_path(&dir).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the fake application did not quit");
}

#[tokio::test]
async fn spawn_of_a_missing_program_is_daemon_start_failed() {
    let (_tmp, dir) = runtime();
    let spawn = Spawn::new(Path::new("/nonexistent/kropa-app"), &[]);
    let err = ensure_running_in(&dir, &spawn, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(reason(&err), "daemon_start_failed");
}

#[test]
fn the_service_file_is_looked_up_in_the_bus_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let services = tmp.path().join("dbus-1/services");
    std::fs::create_dir_all(&services).unwrap();
    let data = tmp.path().to_str().unwrap().to_owned();
    let lookup = move |name: &str| match name {
        "XDG_DATA_HOME" => Some("/nonexistent".to_owned()),
        "XDG_DATA_DIRS" => Some(format!("/nonexistent2:{data}")),
        _ => None,
    };
    if cfg!(target_os = "linux") {
        assert_eq!(kropa_ipc::service_file_present(&lookup), Some(false));
        std::fs::write(services.join(format!("{}.service", kropa_ipc::APP_ID)), "").unwrap();
        assert_eq!(kropa_ipc::service_file_present(&lookup), Some(true));
    } else {
        assert_eq!(kropa_ipc::service_file_present(&lookup), None);
    }
}

// ---- lot 2.1d: review 2.1a ----

/// m13: a request without `id` is not executed and gets an explicit error; `hello` alone may omit it.
#[tokio::test]
async fn a_request_without_an_id_is_refused_not_executed() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counter(Arc<AtomicUsize>);
    impl Handler for Counter {
        fn call(&self, _method: &str, _params: Value) -> BoxFuture<'_, Result<Value, RpcError>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Value::Null) })
        }
    }

    let (_tmp, dir) = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let _server = serve(
        &dir,
        ServerConfig::new("kropa-gtk", "1"),
        Arc::new(Counter(Arc::clone(&calls))),
    )
    .unwrap();
    let mut stream = greeted(&dir).await;
    let mut ask = async |text: &str| -> Value {
        stream
            .write_all(format!("{text}\n").as_bytes())
            .await
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        serde_json::from_slice(&buf[..n]).unwrap()
    };
    let answer = ask(r#"{"jsonrpc":"2.0","method":"app.quit"}"#).await;
    assert_eq!(answer["error"]["code"], -32600);
    assert_eq!(answer["error"]["data"]["reason"], "ipc_protocol_error");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "a notification was run");
    // With an id the same method runs.
    let answer = ask(r#"{"jsonrpc":"2.0","id":9,"method":"app.quit"}"#).await;
    assert_eq!(answer["id"], 9);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// m7: a FIFO at the name of the lock must not block the server for ever.
#[test]
fn a_fifo_in_place_of_the_lock_is_refused_without_blocking() {
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        dir.join("app.lock"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let dir2 = dir.clone();
    std::thread::spawn(move || {
        let outcome = serve(&dir2, ServerConfig::new("kropa-gtk", "1"), Arc::new(Echo));
        tx.send(outcome.map(|_| ())).ok();
    });
    let outcome = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the server blocked on the FIFO");
    let err = outcome.unwrap_err();
    assert_eq!(reason(&err), "unsafe_runtime_dir");
}

/// m7: a lock that belongs to someone else (or a directory) is refused too.
#[test]
fn a_lock_that_is_not_a_regular_file_is_refused() {
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    std::fs::create_dir(dir.join("app.lock")).unwrap();
    // A directory cannot be opened for writing: refused as an I/O error, never taken as a lock.
    let err = serve(&dir, ServerConfig::new("kropa-gtk", "1"), Arc::new(Echo)).unwrap_err();
    assert_eq!(err.exit_code(), 14);
}

/// m12: the whole call, launch included, is bound by one deadline.
#[tokio::test]
async fn the_deadline_covers_a_slow_launch() {
    struct Slow(Cell<Option<Duration>>);
    impl Launcher for Slow {
        fn launch(&self, budget: Duration) -> Result<(), IpcError> {
            self.0.set(Some(budget));
            std::thread::sleep(Duration::from_millis(400));
            Ok(())
        }
    }
    let (_tmp, dir) = runtime();
    let slow = Slow(Cell::new(None));
    let started = std::time::Instant::now();
    let err = ensure_running_in(&dir, &slow, Duration::from_millis(500))
        .await
        .unwrap_err();
    assert_eq!(reason(&err), "daemon_start_timeout");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "took {:?}",
        started.elapsed()
    );
    let budget = slow.0.get().expect("launched");
    assert!(budget <= Duration::from_millis(500), "budget {budget:?}");
}

/// m12: a server that accepts but never answers `hello` does not hold the call for the 5 s of one
/// connection, nor for the sum of several.
#[tokio::test]
async fn the_deadline_covers_a_server_that_never_answers() {
    let (_tmp, dir) = runtime();
    ensure_dir(&dir, current_uid()).unwrap();
    let listener = tokio::net::UnixListener::bind(socket_path(&dir)).unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream); // never answered, never closed
        }
    });
    let count = Cell::new(0);
    let launcher = Counting {
        count: &count,
        result: || Ok(()),
    };
    let started = std::time::Instant::now();
    let err = ensure_running_in(&dir, &launcher, Duration::from_millis(600))
        .await
        .unwrap_err();
    assert_eq!(reason(&err), "daemon_start_timeout");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "took {:?}",
        started.elapsed()
    );
}

/// m8: a server short of descriptors stays up. The limit is lowered for the whole process: this test
/// is meant for `nextest`, where each test has a process of its own.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_accept_that_fails_for_lack_of_descriptors_does_not_stop_the_server() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    let (_tmp, dir) = runtime();
    let _server = start(&dir);
    let socket = socket_path(&dir);
    let saved = getrlimit(Resource::Nofile);
    let lowered = Rlimit {
        current: Some(256),
        maximum: saved.maximum,
    };
    setrlimit(Resource::Nofile, lowered).unwrap();
    // Take every descriptor but none: the next socket of this process is the last one.
    let mut filler = Vec::new();
    while let Ok(file) = std::fs::File::open("/dev/null") {
        filler.push(file);
    }
    filler.pop(); // one is free again: the client's socket will use it
    let queued = StdStream::connect(&socket).unwrap();
    // The server's accept now fails with EMFILE. Give it time to notice.
    tokio::time::sleep(Duration::from_millis(400)).await;
    drop(filler);
    drop(queued);
    setrlimit(Resource::Nofile, saved).unwrap();
    // It is still there and answers.
    let mut client = Client::connect(&socket).await.unwrap();
    let back: Value = client.call("echo", 1).await.unwrap();
    assert_eq!(back, json!(1));
}

use std::os::unix::net::UnixStream as StdStream;
