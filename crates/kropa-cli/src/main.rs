// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of the `kropa` command.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    // Only the standard locale variables are ever looked up (see `kropa_i18n::ENV_VARS`).
    let lookup = |name: &str| std::env::var(name).ok();
    kropa_cli::run(&args, &lookup).into()
}
