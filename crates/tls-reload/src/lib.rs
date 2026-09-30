#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::io;
use std::path::{Path, PathBuf};
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
    config_from_pem(&cert_data, &key_data)
}

/// Build a server config from PEM bytes already in memory. Same rules
/// as [`load_tls_config`].
pub fn config_from_pem(cert_data: &[u8], key_data: &[u8]) -> io::Result<Arc<ServerConfig>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_data)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;

    let key = PrivateKeyDer::from_pem_slice(key_data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Arc::new(config))
}

/// Check the files every `every` for as long as the process runs,
/// logging each swap and each failure under `service`.
///
/// Must be called inside a tokio runtime.
pub fn spawn_watch(
    state: TlsState,
    mut watcher: CertWatcher,
    every: std::time::Duration,
    service: &'static str,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.tick().await;
        loop {
            tick.tick().await;
            match watcher.check(&state) {
                Ok(true) => {
                    tracing::info!(service, "tls: certificate on disk changed; now serving it")
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(
                    service,
                    error = %e,
                    "tls: certificate on disk changed but cannot be used; still serving the previous one"
                ),
            }
        }
    });
}

/// Follows a cert/key pair on disk and swaps new contents into a
/// [`TlsState`], so a renewed certificate is served without a restart.
///
/// It compares file contents, not timestamps: an atomic `mv` over the
/// old file, a copy that keeps the source's mtime and an in-place
/// rewrite all look the same. Reading two small PEM files is the whole
/// cost of a check that finds nothing.
pub struct CertWatcher {
    cert_path: PathBuf,
    key_path: PathBuf,
    loaded: (Vec<u8>, Vec<u8>),
}

impl CertWatcher {
    /// Start from the files as they are now, which is what the
    /// [`TlsState`] this will be checked against was built from.
    pub fn new(cert_path: &Path, key_path: &Path) -> io::Result<Self> {
        Ok(Self {
            cert_path: cert_path.to_path_buf(),
            key_path: key_path.to_path_buf(),
            loaded: (std::fs::read(cert_path)?, std::fs::read(key_path)?),
        })
    }

    /// Re-read both files and, if either changed, build a config from
    /// what was read and swap it in. `Ok(true)` means a new certificate
    /// is now being served.
    ///
    /// An error leaves the old certificate in place and is returned
    /// again on every later check until the files are usable — which is
    /// also what a cert and key replaced one after the other look like
    /// for the moment in between.
    pub fn check(&mut self, state: &TlsState) -> io::Result<bool> {
        let cert = std::fs::read(&self.cert_path)?;
        let key = std::fs::read(&self.key_path)?;
        if cert == self.loaded.0 && key == self.loaded.1 {
            return Ok(false);
        }
        let config = config_from_pem(&cert, &key)?;
        state.swap(Arc::try_unwrap(config).unwrap_or_else(|arc| (*arc).clone()));
        self.loaded = (cert, key);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Once;

    /// Make sure rustls's default crypto provider is installed exactly
    /// once for the test process. Required by rustls 0.23 before any
    /// ServerConfig::builder() call.
    fn install_crypto_provider() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    /// Generate a fresh self-signed cert + key, written as PEM to two
    /// temp files. Returns (cert_path, key_path).
    ///
    /// Uses an `AtomicU64` counter for the temp filename so concurrent
    /// tests don't race on the same path.
    fn make_self_signed_pem_files() -> (std::path::PathBuf, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("rcgen self-signed");
        let cert_pem = cert.cert.pem();
        let key_pem = cert.key_pair.serialize_pem();
        let pid = std::process::id();
        let cert_path =
            std::env::temp_dir().join(format!("mailrs-tls-reload-{pid}-{nonce}-cert.pem"));
        let key_path =
            std::env::temp_dir().join(format!("mailrs-tls-reload-{pid}-{nonce}-key.pem"));
        std::fs::write(&cert_path, cert_pem).unwrap();
        std::fs::write(&key_path, key_pem).unwrap();
        (cert_path, key_path)
    }

    fn fresh_pem() -> (String, String) {
        let c = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("rcgen");
        (c.cert.pem(), c.key_pair.serialize_pem())
    }

    #[test]
    fn a_watcher_swaps_only_when_the_files_change() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let state = TlsState::new((*load_tls_config(&cert_path, &key_path).unwrap()).clone());
        let mut watcher = CertWatcher::new(&cert_path, &key_path).unwrap();
        let before = state.current();

        assert!(!watcher.check(&state).unwrap(), "nothing changed");
        assert!(Arc::ptr_eq(&before, &state.current()));

        let (cert, key) = fresh_pem();
        std::fs::write(&cert_path, cert).unwrap();
        std::fs::write(&key_path, key).unwrap();
        assert!(watcher.check(&state).unwrap(), "renewed pair is swapped in");
        assert!(!Arc::ptr_eq(&before, &state.current()));
        assert!(!watcher.check(&state).unwrap(), "and only once");
    }

    /// The moment between replacing the certificate and replacing the
    /// key: the new certificate with the old key. The old pair must stay
    /// in service, and the next check after the key lands must swap.
    #[test]
    fn a_half_replaced_pair_keeps_the_old_one_until_the_key_lands() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let state = TlsState::new((*load_tls_config(&cert_path, &key_path).unwrap()).clone());
        let mut watcher = CertWatcher::new(&cert_path, &key_path).unwrap();
        let before = state.current();

        let (cert, key) = fresh_pem();
        std::fs::write(&cert_path, &cert).unwrap();
        assert!(watcher.check(&state).is_err(), "new cert with the old key");
        assert!(Arc::ptr_eq(&before, &state.current()));

        std::fs::write(&key_path, &key).unwrap();
        assert!(watcher.check(&state).unwrap());
        assert!(!Arc::ptr_eq(&before, &state.current()));
    }

    #[test]
    fn load_tls_config_succeeds_with_valid_self_signed() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let _cfg = load_tls_config(&cert_path, &key_path).expect("valid PEMs should load");
        // Successful load is the assertion. ServerConfig doesn't expose
        // its inner cert chain via a public API we can introspect here;
        // the fact that load returned Ok means rustls accepted the
        // (chain, key) pair.
    }

    #[test]
    fn tls_state_new_returns_acceptor() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let cfg = load_tls_config(&cert_path, &key_path).unwrap();
        let state = TlsState::new((*cfg).clone());
        let _acceptor = state.acceptor();
        // Just verifying it constructs and yields an acceptor without
        // panicking. Actual TLS handshake covered by mailrs-server bin
        // tests.
    }

    #[test]
    fn tls_state_swap_changes_current_pointer() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let cfg_a = load_tls_config(&cert_path, &key_path).unwrap();
        let state = TlsState::new((*cfg_a).clone());
        let before = state.current();
        // Fresh cert → different Arc identity
        let (cert_path_b, key_path_b) = make_self_signed_pem_files();
        let cfg_b = load_tls_config(&cert_path_b, &key_path_b).unwrap();
        state.swap((*cfg_b).clone());
        let after = state.current();
        assert!(
            !Arc::ptr_eq(&before, &after),
            "swap should produce a fresh Arc"
        );
    }

    #[test]
    fn tls_state_acceptor_snapshots_at_call_time() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let cfg = load_tls_config(&cert_path, &key_path).unwrap();
        let state = TlsState::new((*cfg).clone());
        let _acc1 = state.acceptor();
        // Swap mid-flight:
        let (cert_path_b, key_path_b) = make_self_signed_pem_files();
        let cfg_b = load_tls_config(&cert_path_b, &key_path_b).unwrap();
        state.swap((*cfg_b).clone());
        // _acc1 was snapshotted before the swap, but TlsAcceptor doesn't
        // expose the inner config for us to assert ptr equality here.
        // The test is documentation-by-construction: it compiles + runs,
        // demonstrating the snapshot path doesn't break under concurrent
        // swap.
    }

    #[test]
    fn tls_state_current_returns_arc() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let cfg = load_tls_config(&cert_path, &key_path).unwrap();
        let state = TlsState::new((*cfg).clone());
        let a = state.current();
        let b = state.current();
        // Without a swap in between, current() returns Arcs to the
        // same underlying config.
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn tls_state_clone_shares_inner() {
        install_crypto_provider();
        let (cert_path, key_path) = make_self_signed_pem_files();
        let cfg = load_tls_config(&cert_path, &key_path).unwrap();
        let state1 = TlsState::new((*cfg).clone());
        let state2 = state1.clone();
        // After clone, both share the same inner ArcSwap, so a swap
        // through one is visible via the other.
        let (cert_path_b, key_path_b) = make_self_signed_pem_files();
        let cfg_b = load_tls_config(&cert_path_b, &key_path_b).unwrap();
        let before = state2.current();
        state1.swap((*cfg_b).clone());
        let after_via_state2 = state2.current();
        assert!(
            !Arc::ptr_eq(&before, &after_via_state2),
            "clone should observe swap"
        );
    }

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
        path.push(format!("mailrs-tls-reload-{}-{name}", std::process::id()));
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
