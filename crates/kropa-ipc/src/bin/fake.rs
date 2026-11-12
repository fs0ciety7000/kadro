// SPDX-License-Identifier: GPL-3.0-or-later

//! A fake application for tests: serves `hello`, `app.status` and `app.quit` on the socket of the
//! directory given by `--dir`, and, with `--bus-name`, owns that name on the session bus the way the
//! real `GApplication` does (so that `StartServiceByName` returns). Exits after `--idle` seconds.
//!
//! Two more modes serve the tests between two real users (`tests/uid.rs`, lot 2.1d), where the fake is
//! run by `sudo -u nobody`: `--spy` (a raw listener that counts the bytes it receives) and
//! `--raw-client PATH` (a raw client that says `hello` without any check and counts what it gets).
//!
//! Not part of any release: only built with the `test-support` feature.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kropa_ipc::{BoxFuture, Handler, RpcError, Server, ServerConfig, Shutdown};
use serde_json::{Value, json};

struct Fake {
    quit: std::sync::OnceLock<Shutdown>,
}

impl Handler for Fake {
    fn call(&self, method: &str, _params: Value) -> BoxFuture<'_, Result<Value, RpcError>> {
        let method = method.to_owned();
        Box::pin(async move {
            match method.as_str() {
                "app.status" => Ok(json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocol": kropa_ipc::PROTOCOL,
                    "owns_clipboard": false,
                    "overlay_open": false,
                })),
                "app.quit" => {
                    if let Some(quit) = self.quit.get() {
                        quit.trigger();
                    }
                    Ok(Value::Null)
                }
                "test.cancelled" => {
                    Err(RpcError::new(3, "cancelled", None, "cancelled by the user"))
                }
                other => Err(RpcError::method_not_found(other)),
            }
        })
    }
}

fn main() {
    let mut dir = None;
    let mut bus_name = None;
    let mut bus_address = None;
    let mut idle = 30u64;
    let mut spy = false;
    let mut raw_client = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = args.next().map(PathBuf::from),
            "--bus-name" => bus_name = args.next(),
            "--bus-address" => bus_address = args.next(),
            "--spy" => spy = true,
            "--raw-client" => raw_client = args.next().map(PathBuf::from),
            "--idle" => idle = args.next().and_then(|v| v.parse().ok()).unwrap_or(idle),
            other => panic!("unknown argument {other}"),
        }
    }
    if let Some(socket) = raw_client {
        raw_client_mode(&socket);
        return;
    }
    let dir = dir.expect("--dir is required");
    if spy {
        spy_mode(&dir);
        return;
    }
    let handler = Arc::new(Fake {
        quit: std::sync::OnceLock::new(),
    });
    let mut config = ServerConfig::new("kropa-gtk", env!("CARGO_PKG_VERSION"));
    config.methods = ["app.status", "app.quit"].map(String::from).to_vec();
    let server = match Server::bind(&dir, config, handler.clone()) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("kropa-ipc-fake: {e}");
            std::process::exit(1);
        }
    };
    let quit = server.shutdown_handle();
    handler.quit.set(quit.clone()).ok();
    // The socket is bound: now the bus name, so that the activation reports success.
    #[cfg(target_os = "linux")]
    let _connection = bus_name.map(|name| {
        zbus::block_on(async {
            match &bus_address {
                Some(address) => zbus::connection::Builder::address(address.as_str()),
                None => zbus::connection::Builder::session(),
            }
            .name(name)
            .build()
            .await
            .expect("own the bus name")
        })
    });
    #[cfg(not(target_os = "linux"))]
    let _ = (bus_name, bus_address);
    let timer = quit.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(idle));
        timer.trigger();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    if let Err(e) = runtime.block_on(server.run()) {
        eprintln!("kropa-ipc-fake: {e}");
        std::process::exit(1);
    }
}

/// Connects without any check, says `hello`, and prints `connected` then `received N` (the bytes the
/// server sent back before it closed the connection or 3 s passed).
fn raw_client_mode(socket: &std::path::Path) {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(e) => {
            println!("connect failed: {e}");
            return;
        }
    };
    println!("connected");
    let hello = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\",\"params\":{{\"protocol\":{},\"client\":\"kropa\",\"version\":\"0\"}}}}\n",
        kropa_ipc::PROTOCOL
    );
    let _ = stream.write_all(hello.as_bytes());
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut total = 0usize;
    let mut buf = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        total += n;
    }
    println!("received {total}");
}

/// A raw listener in `dir` (created here, open to the other users so that a test run by another
/// account can reach it): prints `ready`, then for each of two connections `received N`, the bytes it
/// read until the end of the stream. It removes its socket and directory on the way out.
fn spy_mode(dir: &std::path::Path) {
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::UnixListener;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(dir)
        .expect("create the directory");
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o711)).expect("chmod dir");
    let socket = kropa_ipc::socket_path(dir);
    let listener = UnixListener::bind(&socket).expect("bind");
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))
        .expect("chmod socket");
    println!("ready");
    std::io::stdout().flush().expect("flush");
    for _ in 0..2 {
        let Ok((mut stream, _)) = listener.accept() else {
            break;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut total = 0usize;
        let mut buf = [0u8; 4096];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            total += n;
        }
        println!("received {total}");
        std::io::stdout().flush().expect("flush");
    }
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir(dir);
}
