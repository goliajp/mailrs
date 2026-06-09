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
//!
//! [`open_store`] wires kevy's push-style metric sink to two outputs:
//! (1) the existing metrics-rs facade — counters/gauges/histograms named
//! `mailrs_kevy_*`, which the `/metrics` endpoint already renders via
//! `WebState.metrics_handle`; (2) a `tracing::info!` event on target
//! `kevy.metric`, so `docker logs` keeps a grep-able trail in parallel.
//! Current-state snapshot fields (live keys / used_memory / aof_bytes /
//! expire_pending / evictions / expired_keys) are read on every
//! `/metrics` scrape via `Store::info()` — see `web::admin::health`.

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
/// subsystems can clone cheaply. Also wires kevy's metric sink to
/// `metrics::*!` + `tracing::info!` — no separate handle needed.
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
/// opening thread for startup replay), so it stays fast: only updates
/// counter/gauge/histogram state and emits a single tracing record.
fn emit_kevy_metric(m: KevyMetric) {
    match m {
        // Replay fires once on startup. Gauges (rather than histograms)
        // are the right type because the value is point-in-time for the
        // current process lifetime — Prometheus `rate()` over a single
        // sample isn't useful, but the absolute "last replay took N ms"
        // is exactly what we want to watch as AOF grows.
        KevyMetric::Replay {
            commands,
            bytes,
            elapsed_ms,
        } => {
            metrics::gauge!("mailrs_kevy_replay_commands").set(commands as f64);
            metrics::gauge!("mailrs_kevy_replay_bytes").set(bytes as f64);
            metrics::gauge!("mailrs_kevy_replay_elapsed_ms").set(elapsed_ms as f64);
            tracing::info!(
                target: "kevy.metric",
                event = "replay",
                commands,
                bytes,
                elapsed_ms,
                "kevy AOF replay complete"
            );
        }
        // Rewrite fires repeatedly (auto 100%/64MiB by default). Counter
        // for cardinality, histogram for the distribution of each
        // rewrite's effect.
        KevyMetric::Rewrite {
            keys,
            before_bytes,
            after_bytes,
            elapsed_ms,
        } => {
            let reclaimed = before_bytes.saturating_sub(after_bytes);
            metrics::counter!("mailrs_kevy_rewrite_total").increment(1);
            metrics::histogram!("mailrs_kevy_rewrite_elapsed_ms").record(elapsed_ms as f64);
            metrics::histogram!("mailrs_kevy_rewrite_reclaimed_bytes").record(reclaimed as f64);
            metrics::gauge!("mailrs_kevy_rewrite_keys").set(keys as f64);
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
