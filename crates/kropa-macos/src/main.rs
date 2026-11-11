// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of `kropa-app` for macOS (placeholder: no interface yet).

#[cfg(target_os = "macos")]
fn main() {
    eprintln!("kropa-app (macOS): graphical shell not implemented yet");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("kropa-app: this shell only runs on macOS");
    std::process::exit(1);
}
