// SPDX-License-Identifier: GPL-3.0-or-later

//! The check of the peer's UID with two real users (lot 2.1d, review 2.1a I1, `docs/security/secrets.md`
//! section 7): the value the kernel returns for `SO_PEERCRED` is compared, in both directions, with
//! a process that really runs as `nobody` (through `sudo -n -u nobody`).
//!
//! These tests need `sudo` without a password and an account `nobody`. The Linux CI has both, and sets
//! `KROPA_REQUIRE_IPC_UID_TESTS=1` so that a missing tool fails the run instead of skipping. Anywhere
//! else they print why they skip and pass. The fake application is copied to a directory that
//! `nobody` can enter, because the build directory usually is not.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use kropa_ipc::dir::socket_path;
use kropa_ipc::{
    BoxFuture, Client, Handler, IpcError, PROTOCOL, RpcError, ServerConfig, current_uid, serve,
};
use serde_json::Value;

const FAKE: &str = env!("CARGO_BIN_EXE_kropa-ipc-fake");

struct Nothing;

impl Handler for Nothing {
    fn call(&self, method: &str, _params: Value) -> BoxFuture<'_, Result<Value, RpcError>> {
        let method = method.to_owned();
        Box::pin(async move { Err(RpcError::method_not_found(&method)) })
    }
}

/// `sudo -n -u nobody -- program args...`, without standard input.
fn as_nobody(program: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("sudo");
    command
        .args(["-n", "-u", "nobody", "--"])
        .arg(program)
        .args(args)
        .stdin(Stdio::null());
    command
}

/// The UID of `nobody` when these tests can run, else `None` (after saying why, or panicking when the
/// run requires them).
fn nobody() -> Option<u32> {
    let required = std::env::var("KROPA_REQUIRE_IPC_UID_TESTS").is_ok_and(|v| v == "1");
    let why = match Command::new("sudo")
        .args(["-n", "-u", "nobody", "--", "id", "-u"])
        .stdin(Stdio::null())
        .output()
    {
        Err(e) => format!("sudo cannot be run: {e}"),
        Ok(out) if !out.status.success() => format!(
            "`sudo -n -u nobody` failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Ok(out) => match String::from_utf8_lossy(&out.stdout).trim().parse::<u32>() {
            Ok(uid) if uid == current_uid() => {
                format!("the tests already run as UID {uid}, there is no other user to test with")
            }
            Ok(uid) => return Some(uid),
            Err(e) => format!("`id -u` of nobody is not a number: {e}"),
        },
    };
    assert!(!required, "KROPA_REQUIRE_IPC_UID_TESTS=1 but: {why}");
    eprintln!("SKIPPED (set KROPA_REQUIRE_IPC_UID_TESTS=1 to make this an error): {why}");
    None
}

/// A directory under `/tmp` (not `$TMPDIR`: it may be private) with the given mode.
fn shared_dir(mode: u32) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("kropa-uid-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    tmp
}

/// The fake application, copied where `nobody` can run it.
fn copy_fake(into: &Path) -> PathBuf {
    let copy = into.join("fake");
    std::fs::copy(FAKE, &copy).unwrap();
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    copy
}

/// A server of this user, and a client of `nobody`: the server drops it without a byte.
///
/// The directory is `0700` in real life, which already keeps `nobody` out; here it is opened to
/// `0711` and the socket to `0666` after the bind, so that the only thing left to refuse `nobody` is
/// the check of `SO_PEERCRED`. The client first proves that it did connect.
#[test]
fn a_client_running_as_another_user_is_dropped_and_gets_nothing() {
    let Some(_nobody_uid) = nobody() else { return };
    let tmp = shared_dir(0o711);
    let dir = tmp.path().join("kropa");
    let server = serve(
        &dir,
        ServerConfig::new("kropa-gtk", "1"),
        Arc::new(Nothing),
    )
    .unwrap();
    let socket = socket_path(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o711)).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();
    let fake = copy_fake(tmp.path());

    let out = as_nobody(&fake, &["--raw-client", socket.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the helper failed: {stdout} / {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.lines().any(|l| l == "connected"),
        "the client of nobody did not reach the socket, the test proves nothing: {stdout}"
    );
    assert!(
        stdout.lines().any(|l| l == "received 0"),
        "the server answered another user: {stdout}"
    );

    // The server itself is fine: its own user still gets an answer (the directory back to 0700).
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let client = Client::connect(&socket).await.unwrap();
        assert_eq!(client.server().protocol, PROTOCOL);
    });
    drop(server);
}

/// A server of `nobody`, and a client of this user: the client refuses before it writes anything.
///
/// The spy (the fake in `--spy` mode, run by `nobody`) counts the bytes it reads. Its first
/// connection is the client under test: 0 bytes. Its second one is a raw connection of this process
/// that does send `hello`: more than 0, which proves that the spy would have seen bytes.
#[test]
fn a_server_running_as_another_user_receives_nothing_from_the_client() {
    let Some(nobody_uid) = nobody() else { return };
    // `nobody` creates the `kropa` directory in it.
    let tmp = shared_dir(0o777);
    let dir = tmp.path().join("kropa");
    let fake = copy_fake(tmp.path());

    let mut spy = as_nobody(&fake, &["--spy", "--dir", dir.to_str().unwrap()])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(spy.stdout.take().unwrap()).lines();
    let first = lines.next().transpose().unwrap();
    assert_eq!(
        first.as_deref(),
        Some("ready"),
        "the spy of nobody did not start"
    );
    let socket = socket_path(&dir);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt
        .block_on(Client::connect_as_unchecked_dir(&socket, current_uid()))
        .unwrap_err();
    match err {
        IpcError::PeerUidMismatch { peer, expected } => {
            assert_eq!((peer, expected), (nobody_uid, current_uid()));
        }
        other => panic!("expected peer_uid_mismatch, got {other}"),
    }
    assert_eq!(err.exit_code(), 14);
    assert_eq!(err.reason(), Some("peer_uid_mismatch"));
    let refused = lines.next().transpose().unwrap();
    assert_eq!(
        refused.as_deref(),
        Some("received 0"),
        "bytes were sent to a server of another user"
    );

    // Control: a connection that does write is seen by the spy.
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\"}\n")
        .unwrap();
    drop(stream);
    let control = lines.next().transpose().unwrap().unwrap_or_default();
    let count: usize = control
        .strip_prefix("received ")
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("unexpected spy output: {control:?}"));
    assert!(count > 0, "the spy sees nothing: the test proves nothing");

    drop(lines);
    assert!(spy.wait().unwrap().success());
}
