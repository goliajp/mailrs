//! A self-signed certificate, trusted by the test client.

use std::sync::Arc;

use mailrs_tls_reload::{TlsState, config_from_pem};
use tokio_rustls::TlsConnector;

pub(crate) struct Cert {
    pub(crate) server: TlsState,
    pub(crate) connector: TlsConnector,
}

pub(crate) fn cert() -> Cert {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let config = config_from_pem(
        ck.cert.pem().as_bytes(),
        ck.signing_key.serialize_pem().as_bytes(),
    )
    .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ck.cert.der().clone()).unwrap();
    let client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Cert {
        server: TlsState::new(Arc::unwrap_or_clone(config)),
        connector: TlsConnector::from(Arc::new(client)),
    }
}
