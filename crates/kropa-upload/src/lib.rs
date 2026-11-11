// SPDX-License-Identifier: GPL-3.0-or-later

//! Upload engine of Kropa: destinations, request building and the retry queue.

// Pedantic lints that cost more than they bring here: product names (ShareX, Zipline) in docs, long
// but linear parsing functions, and binding names like `first`/`last`.
#![allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::cast_precision_loss,
    clippy::format_push_string,
    clippy::format_collect,
    clippy::assert_is_empty
)]

pub mod debug_log;
pub mod destination;
pub mod engine;
pub mod error;
pub mod http;
pub mod import;
pub mod links;
pub mod masked;
pub mod origin;
pub mod profiles;
pub mod secret;
pub mod sensitive;
pub mod template;
pub use error::UploadError;

// Tests that carry a secret run against a real HTTPS server on the loopback, in this crate only: the
// client that trusts a test CA exists in test builds of this crate and nowhere else.
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod testing;
