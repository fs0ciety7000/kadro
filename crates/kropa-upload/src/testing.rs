// SPDX-License-Identifier: GPL-3.0-or-later

//! A real HTTPS server on the loopback for the tests that carry a secret (`docs/security/secrets.md`
//! section 4.5, decision U3: no `http://` exception for a secret, not even on the loopback).
//!
//! [`Front`] starts a `wiremock` server and puts a TLS listener in front of it. The certificate is
//! made for `127.0.0.1` by a CA generated for the test (`rcgen`). The client of the test trusts that
//! CA through [`HttpClient::with_extra_root`], which exists in test builds of this crate only.

use std::net::{IpAddr, Ipv4Addr};
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyUsagePurpose, PublicKeyData,
    SanType, SerialNumber, SigningKey,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
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
        let provider = Arc::new(rustls_graviola::default_provider());
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

/// An ECDSA P-256 key made by Graviola (the provider of the product, pure Rust: no C compiler, so the
/// tests build for every target), which `rcgen` uses to sign through [`SigningKey`].
struct TestKey {
    pair: graviola::signing::ecdsa::SigningKey<graviola::signing::ecdsa::P256>,
    /// The public point, X9.62 uncompressed (what `rcgen` puts in the certificate).
    point: Vec<u8>,
}

impl TestKey {
    fn generate() -> Self {
        let pair = graviola::signing::ecdsa::SigningKey::generate().expect("P-256 key");
        let mut buffer = [0u8; 128];
        let spki = pair.to_spki_der(&mut buffer).expect("SPKI");
        // The point closes the SPKI of a P-256 key: 65 bytes (0x04 || X || Y).
        let point = spki[spki.len() - 65..].to_vec();
        assert_eq!(point[0], 4);
        Self { pair, point }
    }

    fn pkcs8(&self) -> PrivateKeyDer<'static> {
        let mut buffer = [0u8; 256];
        let der = self.pair.to_pkcs8_der(&mut buffer).expect("PKCS#8");
        PrivateKeyDer::try_from(der.to_vec()).expect("PKCS#8 form")
    }
}

impl PublicKeyData for TestKey {
    fn der_bytes(&self) -> &[u8] {
        &self.point
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

impl SigningKey for TestKey {
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        let mut buffer = [0u8; 80];
        self.pair
            .sign_asn1::<graviola::hashing::Sha256>(&[msg], &mut buffer)
            .map(<[u8]>::to_vec)
            .map_err(|_| rcgen::Error::RemoteKeyError)
    }
}

/// A serial number that no other certificate of the process has (`rcgen` makes none without its
/// crypto backend, which these tests do not build).
fn serial() -> SerialNumber {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(0));
    SerialNumber::from(nanos.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed)) >> 1)
}

/// A CA and a leaf certificate for `127.0.0.1`, both made now.
fn certificates() -> (
    CertificateDer<'static>,
    Vec<CertificateDer<'static>>,
    PrivateKeyDer<'static>,
) {
    let ca_key = TestKey::generate();
    let mut ca_params = CertificateParams::new(Vec::new()).expect("CA parameters");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Kropa test CA");
    ca_params.serial_number = Some(serial());
    let ca = ca_params.self_signed(&ca_key).expect("CA certificate");
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = TestKey::generate();
    let mut leaf_params = CertificateParams::new(Vec::new()).expect("leaf parameters");
    leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "127.0.0.1");
    leaf_params.serial_number = Some(serial());
    let leaf = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("leaf certificate");
    (ca.der().clone(), vec![leaf.der().clone()], leaf_key.pkcs8())
}
