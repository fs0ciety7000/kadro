// SPDX-License-Identifier: GPL-3.0-or-later

//! `trycmd` fixtures of the command line: `tests/cmd/*.toml` with their `.stdout` and `.stderr`.
//!
//! Each case runs the real `kropa` binary in a clean environment (no inherited variables), so the
//! language comes from `--lang` or from the `env.add` of the case. Cases that need files run in a
//! copy of a folder of `tests/cmd/ctx/`.
//!
//! The expected files use `[..]` where the output depends on the machine: the version, commit and
//! target of `kropa version`, and the lot that brings `capture screen|output` and `doctor`
//! (1.4 on Linux, 1.5 on macOS). After a deliberate change, `TRYCMD=overwrite cargo test -p kropa
//! --test cmd` rewrites the expected files with the real output: review the diff and put these
//! wildcards back.

#[test]
fn fixtures() {
    trycmd::TestCases::new()
        .default_bin_name("kropa")
        .case("tests/cmd/*.toml");
}
