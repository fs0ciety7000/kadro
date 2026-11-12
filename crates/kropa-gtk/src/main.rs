// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of `kropa-gtk` for Linux (placeholder: no interface yet).

#[cfg(target_os = "linux")]
fn main() {
    eprintln!("kropa-gtk (Linux): graphical shell not implemented yet");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("kropa-gtk: this shell only runs on Linux");
    std::process::exit(1);
}
