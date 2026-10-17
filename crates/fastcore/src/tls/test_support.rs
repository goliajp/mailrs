//! A self-signed certificate on both ends, a line client that can switch
//! to TLS mid-session, and a state holding one real account.

use std::sync::Arc;

use mailrs_tls_reload::{TlsState, config_from_pem};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::FastcoreState;

pub(crate) const USER: &str = "alice@golia.example.test";
pub(crate) const PASSWORD: &str = "correct horse";

pub(crate) struct Cert {
    pub(crate) server: TlsState,
    connector: TlsConnector,
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

pub(crate) fn state() -> Arc<FastcoreState> {
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let hash = mailrs_core::users::UserStore::hash_password(PASSWORD).unwrap();
    mailbox
        .upsert_account(
            USER,
            &serde_json::json!({ "password_hash": hash }).to_string(),
        )
        .unwrap();
    Arc::new(FastcoreState::new(mailbox))
}

pub(crate) async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    (l, addr)
}

pub(crate) struct Client<S> {
    io: BufReader<S>,
}

impl Client<TcpStream> {
    pub(crate) async fn connect(addr: std::net::SocketAddr) -> Self {
        Self {
            io: BufReader::new(TcpStream::connect(addr).await.unwrap()),
        }
    }

    /// The handshake, on whatever the server has not answered yet.
    pub(crate) async fn upgrade(self, cert: &Cert) -> Client<TlsStream<TcpStream>> {
        assert!(self.io.buffer().is_empty(), "server spoke before TLS");
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tls = cert
            .connector
            .connect(name, self.io.into_inner())
            .await
            .unwrap();
        Client {
            io: BufReader::new(tls),
        }
    }
}

impl Client<TlsStream<TcpStream>> {
    pub(crate) async fn connect_tls(addr: std::net::SocketAddr, cert: &Cert) -> Self {
        Client::connect(addr).await.upgrade(cert).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    pub(crate) async fn send(&mut self, raw: &str) {
        self.io.get_mut().write_all(raw.as_bytes()).await.unwrap();
        self.io.get_mut().flush().await.unwrap();
    }

    pub(crate) async fn line(&mut self) -> String {
        let mut l = String::new();
        self.io.read_line(&mut l).await.unwrap();
        assert!(!l.is_empty(), "connection closed");
        l
    }

    /// Lines up to and including the first one starting with `end`.
    pub(crate) async fn until(&mut self, end: &str) -> String {
        let mut out = String::new();
        loop {
            let l = self.line().await;
            out.push_str(&l);
            if l.starts_with(end) {
                return out;
            }
        }
    }
}
