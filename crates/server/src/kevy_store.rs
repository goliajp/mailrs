//! kevy KV store — in-process `kevy_embedded::Store` only.
//!
//! Phase C completed the migration off the network kevy container.
//! Every subsystem now uses [`KevyStore`] (= `Arc<kevy_embedded::Store>`)
//! either directly or via the trait implementations in mailrs-shield /
//! mailrs-intelligence / mailrs-outbound-queue, which take the same
//! `kevy_embedded::Store` handle.
//!
//! Embedded mode performance ≈ 10× over the network path (no syscall,
//! no RESP serialization, no socket round-trip) and AOF + snapshot
//! persistence comes for free when [`open_store`] is called with a
//! `data_dir`.

use std::io;
use std::path::Path;
use std::sync::Arc;

use kevy_embedded::{Config as KevyConfig, Store};

/// Shareable handle to the in-process kevy embedded store.
pub type KevyStore = Arc<Store>;

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
    fn open_memory_only_store() {
        let store = open_store(None).unwrap();
        store.set(b"k", b"v").unwrap();
        assert_eq!(store.get(b"k").unwrap().as_deref(), Some(&b"v"[..]));
    }
}
