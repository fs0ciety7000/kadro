// SPDX-License-Identifier: GPL-3.0-or-later

//! A real HTTPS server on the loopback for the tests that carry a secret (`docs/security/secrets.md`
//! section 4.5, decision U3: no `http://` exception for a secret, not even on the loopback).
//!
//! [`Front`] starts a `wiremock` server and puts a TLS listener in front of it. The certificate is
//! made for `127.0.0.1` by a CA generated for the test (`rcgen`, `aws-lc-rs` backend). The client of the test trusts that
//! CA through [`HttpClient::with_extra_root`], which exists in test builds of this crate only.

use std::net::{IpAddr, Ipv4Addr};
use std::ops::Deref;
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use wiremock::MockServer;

use crate::engine::Uploader;
use crate::http::HttpClient;
use crate::secret::SecretSource;

/// A mock server reachable over HTTPS only (`uri()` is `https://127.0.0.1:<port>`); everything else
/// is that of the `wiremock` server behind it.
pub(crate) struct Front {
    mock: MockServer,
    port: u16,
    ca: CertificateDer<'static>,
    accept: JoinHandle<()>,
}

impl Front {
    pub(crate) async fn start() -> Self {
        let (ca, chain, key) = certificates();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("server certificate");
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind the loopback");
        let port = listener.local_addr().expect("local address").port();
        let mock = MockServer::start().await;
        let backend = *mock.address();
        let accept = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let Ok(mut plain) = TcpStream::connect(backend).await else {
                        return;
                    };
                    let _ = tokio::io::copy_bidirectional(&mut tls, &mut plain).await;
                });
            }
        });
        Self {
            mock,
            port,
            ca,
            accept,
        }
    }

    /// `https://127.0.0.1:<port>`.
    pub(crate) fn uri(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    /// The CA of this server, for a client built by hand.
    pub(crate) fn ca_for_tests(&self) -> CertificateDer<'static> {
        self.ca.clone()
    }

    /// The engine, on a client that trusts the CA of this server.
    pub(crate) fn uploader<'a>(&self, secrets: &'a dyn SecretSource) -> Uploader<'a> {
        let http = HttpClient::with_extra_root(self.ca.clone()).expect("test client");
        Uploader::with_client(secrets, http)
    }
}

impl Deref for Front {
    type Target = MockServer;

    fn deref(&self) -> &MockServer {
        &self.mock
    }
}

impl Drop for Front {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// A CA and a leaf certificate for `127.0.0.1`, both made now.
fn certificates() -> (
    CertificateDer<'static>,
    Vec<CertificateDer<'static>>,
    PrivateKeyDer<'static>,
) {
    let ca_key = KeyPair::generate().expect("CA key");
    let mut ca_params = CertificateParams::new(Vec::new()).expect("CA parameters");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Kropa test CA");
    let ca = ca_params.self_signed(&ca_key).expect("CA certificate");
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().expect("leaf key");
    let mut leaf_params = CertificateParams::new(Vec::new()).expect("leaf parameters");
    leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "127.0.0.1");
    let leaf = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("leaf certificate");
    (
        ca.der().clone(),
        vec![leaf.der().clone()],
        PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
    )
}
