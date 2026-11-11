// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of `kropa-app` for macOS (placeholder: no interface yet).
//!
//! The one thing it does is a diagnostic for the Keychain broker (lot 1.8m): `kropa-app
//! --keychain-status` reads the state of the secret store **through the broker** (`kropa
//! __store-broker`, the binary next to this one), the way the application will. It exists so that the
//! manual list of `docs/testing/manual-macos.md` can try the broker end to end before there is an
//! interface; its output is for whoever runs it, and goes away with the placeholder.

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--keychain-status"] {
        return keychain_status();
    }
    eprintln!("kropa-app (macOS): graphical shell not implemented yet");
    std::process::ExitCode::SUCCESS
}

/// Prints what the broker says of the store; exit 0 when it is available, 7 (secret unavailable)
/// otherwise, with the reason.
#[cfg(target_os = "macos")]
fn keychain_status() -> std::process::ExitCode {
    use std::process::ExitCode;
    use std::sync::Arc;

    use kropa_core::SystemClock;
    use kropa_upload::secret::StoreState;

    let store = match kropa_store::open_broker_store(Arc::new(SystemClock)) {
        Ok(store) => store,
        Err(failure) => {
            eprintln!("kropa-app: {failure}");
            return ExitCode::from(7);
        }
    };
    // This is a person at a terminal: a Keychain prompt may be shown.
    store.set_interactive(true);
    let status = store.status();
    println!("store: {}", status.store);
    println!("state: {}", status.state.as_str());
    if let Some(detail) = &status.detail {
        println!("detail: {detail}");
    }
    for (profile, count) in &status.profiles {
        println!("profile {profile}: {} entries", count.entries);
    }
    if status.state == StoreState::Available {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(7)
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("kropa-app: this shell only runs on macOS");
    std::process::exit(1);
}
