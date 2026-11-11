// SPDX-License-Identifier: GPL-3.0-or-later

//! `trycmd` fixtures of the command line: `tests/cmd/*.toml` with their `.stdout` and `.stderr`.
//!
//! Each case runs the real `kropa` binary in a clean environment (no inherited variables), so the
//! language comes from `--lang` or from the `env.add` of the case. Cases that need files run in a
//! copy of a folder of `tests/cmd/ctx/`. Regenerate the expected output of a case after a
//! deliberate change with `TRYCMD=overwrite cargo test -p kropa --test cmd`, then review the diff.

#[test]
fn fixtures() {
    trycmd::TestCases::new()
        .default_bin_name("kropa")
        .case("tests/cmd/*.toml");
}
