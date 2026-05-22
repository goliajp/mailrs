#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::io;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// Wrapper around `Arc<ArcSwap<ServerConfig>>` that lets you swap the
/// active rustls server config atomically. In-flight TLS handshakes
/// keep the old config (each `acceptor()` call snapshots the current
/// pointer); new handshakes use the new config immediately after a
/// [`swap`](Self::swap).
///
/// Typical use: hold a `TlsState` in your server, derive a fresh
/// [`TlsAcceptor`] for each incoming connection via
/// [`TlsState::acceptor`], and call [`TlsState::swap`] from your
/// renewal hook (ACME, certbot reload signal, etc.) when new
/// certificates land on disk.
///
/// ```rust,no_run
/// use mailrs_tls_reload::{TlsState, load_tls_config};
/// use std::path::Path;
///
/// # async fn run() -> std::io::Result<()> {
/// let cfg = load_tls_config(Path::new("cert.pem"), Path::new("key.pem"))?;
/// let state = TlsState::new((*cfg).clone());
///
/// // ... in your accept loop:
/// let acceptor = state.acceptor();
/// // tokio::spawn(handle(acceptor, socket));
///
/// // ... later, certs got renewed:
/// let new_cfg = load_tls_config(Path::new("cert.pem"), Path::new("key.pem"))?;
/// state.swap((*new_cfg).clone());
/// // Subsequent acceptor() calls return the new config.
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TlsState {
    inner: Arc<ArcSwap<ServerConfig>>,
}

impl TlsState {
    /// Construct from an initial server config.
    pub fn new(config: ServerConfig) -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
        }
    }

    /// Snapshot the current config into a fresh `TlsAcceptor`. Each
    /// snapshot is independent — in-flight handshakes that took an
    /// acceptor before [`swap`](Self::swap) keep using the old config.
    pub fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(self.inner.load_full())
    }

    /// Replace the active config atomically. New `acceptor()` calls
    /// after this point return the new config. In-flight handshakes
    /// are unaffected.
    pub fn swap(&self, new: ServerConfig) {
        self.inner.store(Arc::new(new));
    }

    /// Read the current config (a fresh `Arc`). Cheap; one atomic load.
    /// Useful for inspecting the active certs in admin endpoints.
    pub fn current(&self) -> Arc<ServerConfig> {
        self.inner.load_full()
    }
}

/// Load a rustls `ServerConfig` from PEM-encoded cert + key files on
/// disk. Returns `Arc<ServerConfig>` ready to hand to [`TlsState::new`]
/// or [`TlsState::swap`] (after a clone).
///
/// The cert file may contain a chain (multiple PEM blocks); the key
/// file must contain exactly one PEM-encoded private key
/// (PKCS#1, PKCS#8, or SEC1). No client auth is configured.
///
/// Errors:
/// - `io::Error` (NotFound, PermissionDenied) if either file is missing
///   or unreadable
/// - `io::Error(InvalidData)` if either file is not valid PEM, the key
///   is unparseable, or rustls rejects the (certs, key) pair
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> io::Result<Arc<ServerConfig>> {
    let cert_data = std::fs::read(cert_path)?;
    let key_data = std::fs::read(key_path)?;

    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_data)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;

    let key = PrivateKeyDer::from_pem_slice(&key_data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Note: `TlsState`'s swap/current/acceptor mechanics are not unit-
    // tested here because constructing a `rustls::ServerConfig` in
    // isolation requires a crypto provider + a real cert (rustls 0.23
    // refuses empty chains). That integration is covered by
    // `mailrs-server`'s bin tests, which provide a configured cert.
    // The wrapper itself is ~6 lines around `arc-swap` — its shape is
    // covered by `arc-swap`'s own test suite.

    #[test]
    fn load_tls_config_rejects_missing_files() {
        let r = load_tls_config(
            Path::new("/nonexistent/path/cert.pem"),
            Path::new("/nonexistent/path/key.pem"),
        );
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn load_tls_config_rejects_invalid_pem() {
        // Write invalid PEM data to a temp file
        let cert_temp = tempfile_for("not_a_cert.pem", b"definitely not a PEM file");
        let key_temp = tempfile_for("not_a_key.pem", b"also not a PEM file");
        let r = load_tls_config(&cert_temp, &key_temp);
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn load_tls_config_rejects_empty_cert_file() {
        let cert_temp = tempfile_for("empty_cert.pem", b"");
        let key_temp = tempfile_for("empty_key.pem", b"");
        let r = load_tls_config(&cert_temp, &key_temp);
        assert!(r.is_err());
    }

    /// Helper: write `data` to a uniquely-named temp file, return the path.
    /// The file lives until process exit (test cleanup is OS's problem;
    /// `/tmp` gets reaped).
    fn tempfile_for(name: &str, data: &[u8]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "mailrs-tls-reload-{}-{name}",
            std::process::id()
        ));
        let mut f = std::fs::File::create(&path).expect("create temp");
        f.write_all(data).expect("write temp");
        path
    }

    #[test]
    fn tempfile_helper_works() {
        let p = tempfile_for("smoke.txt", b"hi");
        assert_eq!(std::fs::read(&p).unwrap(), b"hi");
    }
}
