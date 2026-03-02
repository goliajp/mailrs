use std::io;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

#[derive(Clone)]
pub struct TlsState {
    inner: Arc<ArcSwap<ServerConfig>>,
}

impl TlsState {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
        }
    }

    pub fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(self.inner.load_full())
    }

    pub fn swap(&self, new: ServerConfig) {
        self.inner.store(Arc::new(new));
    }
}

pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> io::Result<Arc<ServerConfig>> {
    let cert_data = std::fs::read(cert_path)?;
    let key_data = std::fs::read(key_path)?;

    let certs = rustls_pemfile::certs(&mut &cert_data[..])
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let key = rustls_pemfile::private_key(&mut &key_data[..])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found"))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Arc::new(config))
}

