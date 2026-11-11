// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of the `kropa` command.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Before anything is read: no core file may keep a secret (`docs/security/secrets.md` section 6).
    // Off in debug builds, so that a debugger and core dumps stay usable while developing; never off
    // by an environment variable.
    #[cfg(not(debug_assertions))]
    if let Err(e) = kropa_cli::hardening::disable_core_dumps() {
        // `--lang` is not parsed yet: the language comes from the standard locale variables.
        let lookup = |name: &str| std::env::var(name).ok();
        let lang = kropa_i18n::negotiate(None, None, &lookup);
        let detail = e.source.to_string();
        let text = kropa_i18n::Localizer::new(lang).map_or_else(
            |_| format!("{e}; refusing to run with memory dumps allowed"),
            |loc| {
                loc.message_args(
                    "error-hardening-failed",
                    &[("step", e.step), ("detail", &detail)],
                )
            },
        );
        eprintln!("kropa: {text}");
        return ExitCode::from(1);
    }
    let args: Vec<OsString> = std::env::args_os().collect();
    // macOS: `kropa __store-broker` is the Keychain broker that `kropa-app` starts on a socket pair
    // (`docs/security/secrets.md` section 2.2.1). It is not a command: it takes nothing from the
    // command line, prints no help and does nothing until its launch has passed every check.
    #[cfg(target_os = "macos")]
    if args.len() == 2 && args[1] == kropa_store::BROKER_ARGUMENT {
        return kropa_store::run_broker();
    }
    // Linux: `kropa __clipboard-serve` is the process that keeps a Wayland selection alive after the
    // command that asked for the copy has exited (`kropa_capture::clipboard`). It is not a command
    // either: it reads one request on stdin, answers one line, and does not run on a terminal.
    #[cfg(target_os = "linux")]
    if args.len() == 2 && args[1] == kropa_capture::clipboard::CLIPBOARD_SERVE_ARGUMENT {
        return kropa_capture::clipboard::run_helper();
    }
    // Only the standard locale variables are ever looked up (see `kropa_i18n::ENV_VARS`).
    let lookup = |name: &str| std::env::var(name).ok();
    kropa_cli::run(&args, &lookup).into()
}
