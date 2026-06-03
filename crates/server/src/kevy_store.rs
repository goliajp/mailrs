//! kevy KV store — both network (legacy) and in-process embedded paths.
//!
//! mailrs is in the middle of migrating from the network kevy container
//! (`redis://kevy:6379`) to the in-process `kevy_embedded::Store`.
//! Until every subsystem moves over, this module exposes both factories
//! and the bootstrap creates both stores side-by-side.
//!
//! - `create_connection(url)` / `validate_url(url)` — legacy network
//!   path, kept until every `redis::cmd(...)` call site is migrated.
//! - `open_store(data_dir)` / [`KevyStore`] — new in-process path. Hand
//!   the returned `Arc<Store>` into subsystems that want kevy without
//!   the network round-trip.
//!
//! Embedded mode performance ≈ 10× over the network path (no syscall,
//! no RESP serialization, no socket round-trip) and adds persistence
//! (AOF + snapshot) for free.

use std::io;
use std::path::Path;
use std::sync::Arc;

use kevy_embedded::{Config as KevyConfig, Store};
use redis::Client;
use redis::aio::ConnectionManager;

/// Shareable handle to the in-process kevy embedded store.
pub type KevyStore = Arc<Store>;

/// Open a network kevy connection (legacy path).
pub async fn create_connection(url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = Client::open(url)?;
    ConnectionManager::new(client).await
}

/// Validate a kevy network URL without connecting (legacy path).
pub fn validate_url(url: &str) -> Result<(), String> {
    Client::open(url).map(|_| ()).map_err(|e| e.to_string())
}

/// Open the in-process kevy embedded store, optionally with AOF +
/// snapshot persistence at `data_dir`. Wraps the store in an `Arc` so
/// subsystems can clone cheaply.
pub fn open_store(data_dir: Option<&Path>) -> io::Result<KevyStore> {
    let cfg = KevyConfig::default();
    let cfg = match data_dir {
        Some(dir) => cfg.with_persist(dir),
        None => cfg,
    };
    let store = Store::open(cfg)?;
    Ok(Arc::new(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_valid() {
        assert!(validate_url("redis://localhost:6379").is_ok());
        assert!(validate_url("redis://127.0.0.1:6379/0").is_ok());
    }

    #[test]
    fn validate_url_invalid() {
        assert!(validate_url("not-a-url").is_err());
    }

    #[test]
    fn validate_url_with_password() {
        assert!(validate_url("redis://:password@localhost:6379").is_ok());
    }

    #[test]
    fn open_memory_only_store() {
        let store = open_store(None).unwrap();
        store.set(b"k", b"v").unwrap();
        assert_eq!(store.get(b"k").unwrap().as_deref(), Some(&b"v"[..]));
    }
}
