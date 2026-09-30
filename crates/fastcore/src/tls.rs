//! The certificate IMAPS and POP3S present: loaded once from
//! `MAILRS_TLS_CERT` / `MAILRS_TLS_KEY` and followed on disk, so a renewed
//! certificate is served without restarting the process.

use std::path::Path;
use std::time::Duration;

use mailrs_tls_reload::{CertWatcher, TlsState, load_tls_config, spawn_watch};

/// How often the files are compared. A renewal is weeks ahead of expiry,
/// so a minute costs nothing and is still immediate for a person
/// replacing an expired certificate by hand.
const CHECK_EVERY: Duration = Duration::from_secs(60);

/// `None` when either variable is unset — the TLS listeners are then
/// skipped — or when the files cannot be used, which is logged.
pub(crate) fn from_env() -> Option<TlsState> {
    let (Ok(cert_path), Ok(key_path)) = (
        std::env::var("MAILRS_TLS_CERT"),
        std::env::var("MAILRS_TLS_KEY"),
    ) else {
        tracing::debug!("MAILRS_TLS_CERT / MAILRS_TLS_KEY unset — skipping IMAPS and POP3S");
        return None;
    };
    let (cert, key) = (Path::new(&cert_path), Path::new(&key_path));
    let loaded = load_tls_config(cert, key).and_then(|config| {
        let watcher = CertWatcher::new(cert, key)?;
        Ok((config, watcher))
    });
    match loaded {
        Ok((config, watcher)) => {
            let state = TlsState::new(std::sync::Arc::unwrap_or_clone(config));
            spawn_watch(state.clone(), watcher, CHECK_EVERY, "fastcore");
            Some(state)
        }
        Err(e) => {
            tracing::error!(error = %e, %cert_path, %key_path, "tls: config load failed; IMAPS and POP3S not started");
            None
        }
    }
}
