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

#[test]
fn fixtures() {
    trycmd::TestCases::new()
        .default_bin_name("kropa")
        .case("tests/cmd/*.toml");
}

#[cfg(target_os = "linux")]
#[test]
fn fixtures_linux() {
    trycmd::TestCases::new()
        .default_bin_name("kropa")
        .case("tests/cmd-linux/*.toml");
}

#[cfg(target_os = "macos")]
#[test]
fn fixtures_macos() {
    trycmd::TestCases::new()
        .default_bin_name("kropa")
        .case("tests/cmd-macos/*.toml");
}
