// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry point of the `kropa` command.

use clap::Parser;

/// Capture, annotate and upload screenshots.
#[derive(Parser)]
#[command(name = "kropa", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
