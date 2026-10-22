//! The in-process `kevy_embedded::Store` this process opens beside the SQL
//! store, for the account and permission caches. Side state both cores share
//! is on the network kevy-server (`MAILRS_KEVY_URL`), not here.
//!
//! [`open_store`] logs kevy's replay and rewrite events on target
//! `kevy.metric`.

use std::io;
use std::path::Path;
use std::sync::Arc;

use kevy_embedded::{Config as KevyConfig, KevyMetric, Store};

/// Shareable handle to the in-process kevy embedded store.
pub type KevyStore = Arc<Store>;

/// Number of shards for the in-process kevy keyspace. Picked to match the
/// 4-core prod host (Config::with_shards uses a power-of-two routing mask
/// when n is a power of two). Reads/writes to different shards hold
/// independent locks → multi-core GET scales. First start with `n > 1`
/// reshards the existing single `aof-0.aof` into `aof-0..n-1.aof` and
/// backs the original up as `aof-0.aof.premigration.<unix_ns>`.
const KEVY_SHARDS: usize = 4;

/// Open the in-process kevy embedded store, optionally with AOF +
/// snapshot persistence at `data_dir`. Wraps the store in an `Arc` so
/// subsystems can clone cheaply, with kevy's metric sink logging through
/// `tracing`.
pub fn open_store(data_dir: Option<&Path>) -> io::Result<KevyStore> {
    let cfg = KevyConfig::default()
        .with_metric_sink(emit_kevy_metric)
        .with_shards(KEVY_SHARDS);
    let cfg = match data_dir {
        Some(dir) => cfg.with_persist(dir),
        None => cfg,
    };
    let store = Store::open(cfg)?;
    Ok(Arc::new(store))
}

/// Sink callback for `KevyMetric` events. Runs synchronously on whichever
/// thread emits the event (reaper thread for background rewrites, the
/// opening thread for startup replay), so it stays fast: a single tracing
/// record.
fn emit_kevy_metric(m: KevyMetric) {
    match m {
        // replay fires once on startup
        KevyMetric::Replay {
            commands,
            bytes,
            elapsed_ms,
            ..
        } => {
            tracing::info!(
                target: "kevy.metric",
                event = "replay",
                commands,
                bytes,
                elapsed_ms,
                "kevy AOF replay complete"
            );
        }
        // rewrite fires repeatedly (auto 100%/64MiB by default)
        KevyMetric::Rewrite {
            keys,
            before_bytes,
            after_bytes,
            elapsed_ms,
        } => {
            let reclaimed = before_bytes.saturating_sub(after_bytes);
            tracing::info!(
                target: "kevy.metric",
                event = "rewrite",
                keys,
                before_bytes,
                after_bytes,
                reclaimed_bytes = reclaimed,
                elapsed_ms,
                "kevy AOF rewrite complete"
            );
        }
        // KevyMetric is #[non_exhaustive] — forward-compatible with new
        // variants kevy may add in future versions.
        _ => {}
    }
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
