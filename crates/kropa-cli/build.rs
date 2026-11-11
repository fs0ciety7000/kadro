// SPDX-License-Identifier: GPL-3.0-or-later

//! Records the target triple and, when the sources come from a git checkout, the commit, for
//! `kropa version`. Without git the commit is simply absent: nothing is invented.

use std::path::Path;
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=KROPA_BUILD_TARGET={target}");

    let git_dir = Path::new("../../.git");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD"))
        && let Some(reference) = head.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed=../../.git/{reference}");
    }
    let commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()));
    if let Some(commit) = commit {
        println!("cargo:rustc-env=KROPA_BUILD_COMMIT={commit}");
    }
}
