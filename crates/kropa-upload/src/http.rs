// SPDX-License-Identifier: GPL-3.0-or-later

//! The one HTTP client of Kropa (`AGENTS.md`, rule 10).
//!
//! - `reqwest` with rustls, certificates checked by the system verifier (`rustls-platform-verifier`:
//!   a private CA is added to the system store, there is no "ignore TLS" option);
//! - the cryptography of `rustls` comes from Graviola, which needs no C compiler and no OpenSSL;
//! - no proxy: neither `HTTP(S)_PROXY` nor the system settings are read (`.no_proxy()`);
//! - redirections are followed by the engine itself, never by the client;
//! - `User-Agent: kropa/<version>`, nothing else is sent that the destination did not ask for;
//! - no request is made when the client is built, and nothing here ever checks for an update.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::redirect::Policy;
use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt as _;

use crate::error::UploadError;

/// Time allowed to open a connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// `User-Agent` of every request.
#[must_use]
pub fn user_agent() -> String {
    format!("kropa/{}", env!("CARGO_PKG_VERSION"))
}

/// The client. Built once per run, and only when a request is about to be made.
pub struct HttpClient {
    pub(crate) inner: reqwest::Client,
}

impl HttpClient {
    /// Builds the client.
    ///
    /// # Errors
    /// [`UploadError::TlsUnsupported`] when the processor lacks the instructions of the TLS
    /// provider, [`UploadError::Network`] (`tls`) if the system verifier cannot start.
    pub fn new() -> Result<Self, UploadError> {
        if !graviola::util::is_cpu_supported() {
            return Err(UploadError::TlsUnsupported);
        }
        let tls_error = |_| UploadError::Network {
            reason: "tls",
            detail: "the TLS configuration could not be built".to_owned(),
        };
        let provider = Arc::new(rustls_graviola::default_provider());
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .with_platform_verifier()
            .map_err(tls_error)?
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let inner = reqwest::Client::builder()
            .use_preconfigured_tls(config)
            .no_proxy()
            .redirect(Policy::none())
            .referer(false)
            .user_agent(user_agent())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| UploadError::Network {
                reason: "network",
                detail: "the HTTP client could not be built".to_owned(),
            })?;
        Ok(Self { inner })
    }
}

/// Runs a future to its end on a small single-threaded runtime. The command line is synchronous;
/// this is its only bridge to the async engine. Must not be called from inside a runtime.
///
/// # Panics
/// If the runtime cannot be created (no thread or no file descriptor left).
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

/// A network error without its URL (which may carry a query) and without the text of the
/// underlying error, classified for `error.reason`.
pub(crate) fn network_error(e: &reqwest::Error) -> UploadError {
    let mut tls = false;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = source {
        if err.downcast_ref::<rustls::Error>().is_some() {
            tls = true;
        }
        if let Some(io) = err.downcast_ref::<std::io::Error>()
            && io
                .get_ref()
                .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
        {
            tls = true;
        }
        source = err.source();
    }
    let (reason, detail) = if e.is_timeout() {
        ("timeout", "the server did not answer in time")
    } else if tls {
        (
            "tls",
            "the TLS handshake failed: the certificate of the server could not be verified",
        )
    } else if e.is_connect() {
        ("connect", "could not connect to the server")
    } else {
        ("network", "the request failed")
    };
    UploadError::Network {
        reason,
        detail: detail.to_owned(),
    }
}
