//! The first steps of a boot: logging, the crypto provider, and the
//! line that says what this process is about to be.

use crate::*;

pub(super) fn install_tracing_and_crypto() {
    // initialize structured logging via tracing-subscriber
    // respect RUST_LOG env var; default to info level
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
}

pub(super) fn log_startup(cfg: &ServerConfig) {
    for warning in cfg.validate() {
        tracing::warn!(warning, "config warning");
    }

    let domains_str = if cfg.local_domains.is_empty() {
        "(none)".into()
    } else {
        cfg.local_domains.join(", ")
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        hostname = cfg.hostname.as_str(),
        maildir = cfg.maildir_root.as_str(),
        domains = domains_str.as_str(),
        tls = ?cfg.tls_mode(),
        antispam = cfg.antispam_enabled,
        dkim = cfg.dkim_selector.as_deref().unwrap_or("(disabled)"),
        "mailrs starting"
    );
}
