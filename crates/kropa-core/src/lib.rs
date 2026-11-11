// SPDX-License-Identifier: GPL-3.0-or-later

//! Core types of Kropa: configuration, name templates, action chain, errors and IPC protocol types.

mod clock;
mod config;
mod error;
mod exit;
mod template;

/// Application identifier (reverse-DNS), shared by the `.desktop` file, the XDG portals and the macOS bundle.
pub const APP_ID: &str = "io.github.fs0ciety7000.Kropa";

pub use clock::{Clock, FixedClock, SystemClock};
pub use config::{
    CONFIG_FILE, Config, DEFAULT_NAME_TEMPLATE, ImageFormat, config_dir, default_output_dir,
};
pub use error::{CoreError, NameError};
pub use exit::ExitCode;
pub use template::{NameContext, NameTemplate, validate_file_name};
