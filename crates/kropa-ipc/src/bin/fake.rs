// SPDX-License-Identifier: GPL-3.0-or-later

//! A fake application for tests: serves `hello`, `app.status` and `app.quit` on the socket of the
//! directory given by `--dir`, and, with `--bus-name`, owns that name on the session bus the way the
//! real `GApplication` does (so that `StartServiceByName` returns). Exits after `--idle` seconds.
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = args.next().map(PathBuf::from),
            "--bus-name" => bus_name = args.next(),
            "--bus-address" => bus_address = args.next(),
            "--idle" => idle = args.next().and_then(|v| v.parse().ok()).unwrap_or(idle),
            other => panic!("unknown argument {other}"),
        }
    }
    let dir = dir.expect("--dir is required");
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
