// SPDX-License-Identifier: GPL-3.0-or-later

//! Records the target triple and, when the sources come from a git checkout of this very workspace,
//! the commit (`<12 hexadecimal digits>`, plus `-dirty` when tracked files have uncommitted changes)
//! for `kropa version`. Without git, or when the sources are not a checkout of the workspace (a
//! tarball unpacked inside another repository), the commit is simply absent: nothing is invented.
//!
//! The paths git reports (`git rev-parse --git-path`) are used as they are, so that a worktree or a
//! submodule, where `.git` is a file, works, and so that references kept in `packed-refs` do not make
//! cargo rebuild at every run (it does so for a `rerun-if-changed` on a file that does not exist).
//! The `-dirty` mark is computed when the script runs: it follows commits and staging, not an edit of
//! a tracked file that changes neither.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Output of `git <args>` run in `dir`, trimmed; `None` if git is missing or fails.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A path printed by git, relative to `dir` when it is not absolute.
fn git_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(git(dir, &["rev-parse", "--git-path", name])?);
    Some(if path.is_absolute() {
        path
    } else {
        dir.join(path)
    })
}

/// Asks cargo to rebuild when `path` changes, if it exists.
fn watch(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// The commit of the workspace checkout, with the files to watch registered.
fn commit(manifest_dir: &Path) -> Option<String> {
    // The repository must be the one of this workspace, not one that happens to contain it.
    let workspace = manifest_dir.join("../..").canonicalize().ok()?;
    let top = PathBuf::from(git(manifest_dir, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .ok()?;
    if top != workspace {
        return None;
    }

    watch(&git_path(manifest_dir, "HEAD")?);
    watch(&git_path(manifest_dir, "packed-refs")?);
    watch(&git_path(manifest_dir, "index")?);
    if let Some(reference) = git(manifest_dir, &["symbolic-ref", "--quiet", "HEAD"])
        && let Some(path) = git_path(manifest_dir, &reference)
    {
        if path.exists() {
            watch(&path);
        } else if let Some(parent) = path.parent() {
            // The reference is packed for now: a new commit writes a loose file in this folder.
            watch(parent);
        }
    }

    let full = git(manifest_dir, &["rev-parse", "HEAD"])?;
    let hash = full.get(..12).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    // `--no-optional-locks`: do not refresh the index, which would look like a change to cargo.
    let dirty = git(
        manifest_dir,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ],
    )
    .is_some();
    Some(if dirty {
        format!("{hash}-dirty")
    } else {
        hash.to_owned()
    })
}

fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=KROPA_BUILD_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    if let Some(commit) = manifest_dir.as_deref().and_then(commit) {
        println!("cargo:rustc-env=KROPA_BUILD_COMMIT={commit}");
    }
}
