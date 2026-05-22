//! TLS state + config loading re-exported from
//! [`mailrs-tls-reload`](https://crates.io/crates/mailrs-tls-reload).
//!
//! Carved out into its own crate (1.0.0) because the
//! `Arc<ArcSwap<ServerConfig>>` hot-reload pattern is universally
//! useful for any rustls-terminating Rust server, not just mail.
//!
//! The public surface is unchanged — anything that previously worked
//! against `crate::tls::*` continues to work via this re-export.

pub use mailrs_tls_reload::{load_tls_config, TlsState};
