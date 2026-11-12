// SPDX-License-Identifier: GPL-3.0-or-later

//! `trycmd` fixtures of the command line: `tests/cmd/*.toml` with their `.stdout` and `.stderr`.
//!
//! Each case runs the real `kropa` binary in a clean environment (no inherited variables), so the
//! language comes from `--lang` or from the `env.add` of the case. Cases that need files run in a
//! copy of a folder of `tests/cmd/ctx/`.
//!
//! The expected files use `[..]` where the output depends on the machine, such as the version,
//! commit and target of `kropa version`. After a deliberate change, `TRYCMD=overwrite cargo test -p
//! kropa --test cmd` rewrites the expected files with the real output: review the diff and put these
//! wildcards back.
//!
//! `tests/cmd/` holds the cases whose result is the same everywhere. What `capture screen|output`
//! and `doctor` do depends on the platform: `tests/cmd-linux/` has the cases of the portal backend
//! (they point `DBUS_SESSION_BUS_ADDRESS` at a missing socket so that no real session is ever
//! reached), `tests/cmd-macos/` those of the `ScreenCaptureKit` backend that hold on a machine
//! without the Screen Recording permission (the rest is `tests/macos.rs` and the manual check-list
//! `docs/testing/manual-macos.md`).
//!
//! Whatever the folder, no case can reach the real session: see `isolated`. The three `doctor` cases
//! of `tests/cmd-linux-no-runtime-dir/` are the ones that must run without any `XDG_RUNTIME_DIR` (the
//! state "no runtime directory" of the IPC socket); they still get the unreachable bus.

/// The cases of a folder, with what keeps them away from the real session (n6): the session bus is a
/// socket that does not exist, so that no case can reach a real keyring (or any service), and
/// `XDG_RUNTIME_DIR` is a private directory of the test, never the one of the user. Set on the runner,
/// these win over the `env.add` of a case: a new case cannot forget them.
///
/// `trycmd` runs the cases when `TestCases` is dropped: the pair is returned with the cases first, so
/// that the directory outlives the run.
fn isolated(glob: &str, runtime_dir: bool) -> (trycmd::TestCases, Option<tempfile::TempDir>) {
    let cases = trycmd::TestCases::new();
    cases.default_bin_name("kropa").env(
        "DBUS_SESSION_BUS_ADDRESS",
        "unix:path=/nonexistent-kropa-bus",
    );
    let runtime = runtime_dir.then(|| tempfile::tempdir().expect("a private runtime directory"));
    if let Some(dir) = &runtime {
        cases.env("XDG_RUNTIME_DIR", dir.path().display().to_string());
    }
    cases.case(glob);
    (cases, runtime)
}

#[test]
fn fixtures() {
    let _run = isolated("tests/cmd/*.toml", true);
}

#[cfg(target_os = "linux")]
#[test]
fn fixtures_linux() {
    let _run = isolated("tests/cmd-linux/*.toml", true);
}

#[cfg(target_os = "linux")]
#[test]
fn fixtures_linux_no_runtime_dir() {
    let _run = isolated("tests/cmd-linux-no-runtime-dir/*.toml", false);
}

#[cfg(target_os = "macos")]
#[test]
fn fixtures_macos() {
    let _run = isolated("tests/cmd-macos/*.toml", true);
}
