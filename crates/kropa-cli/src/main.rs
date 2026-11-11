// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of the `kropa` command.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    // macOS: `kropa __store-broker` is the Keychain broker that `kropa-app` starts on a socket pair
    // (`docs/security/secrets.md` section 2.2.1). It is not a command: it takes nothing from the
    // command line, prints no help and does nothing until its launch has passed every check.
    #[cfg(target_os = "macos")]
    if args.len() == 2 && args[1] == kropa_store::BROKER_ARGUMENT {
        return kropa_store::run_broker();
    }
    // Only the standard locale variables are ever looked up (see `kropa_i18n::ENV_VARS`).
    let lookup = |name: &str| std::env::var(name).ok();
    kropa_cli::run(&args, &lookup).into()
}
