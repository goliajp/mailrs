//! Spool consumer drivers (P6-S7/S8): the two ways a spool file reaches
//! [`consume_spool_file`] in the split topology.
//!
//! - **notify subscriber** — the fast path: the receiver emits `SpoolDelivered`
//!   over the shared kevy-server, the cross-process bridge re-injects it into
//!   this core's bus, and we consume immediately.
//! - **reconcile sweep** — the durability backstop: a periodic scan of the
//!   spool drains anything the notify dropped or that landed while the core was
//!   down. Both share one [`SpoolConsumeDeps`]; the in-flight set dedups overlap.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core::event_bus::{EventBus, SmtpEvent};

use super::consume_spool::{SpoolConsumeDeps, consume_spool_file};

/// Spawn the SpoolDelivered subscriber + the periodic reconcile sweep, both
/// feeding [`consume_spool_file`] against the shared `deps`. `reconcile_secs`
/// is the sweep cadence (faster than the maildir reconcile — the spool is the
/// live path).
pub(crate) fn spawn_spool_consumer(
    deps: Arc<SpoolConsumeDeps>,
    bus: EventBus,
    reconcile_secs: u64,
) {
    // fast path: consume on every SpoolDelivered notify.
    let sub_deps = deps.clone();
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if let SmtpEvent::SpoolDelivered { spool_id, .. } = &ev.event {
                consume_spool_file(spool_id.clone(), &sub_deps).await;
            }
        }
    });

    // durability backstop: drain the spool on a timer.
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(reconcile_secs));
        // skip the immediate first tick — let the subscriber handle the live path.
        tick.tick().await;
        loop {
            tick.tick().await;
            match deps
                .spool_store
                .list_unprocessed(&deps.spool_incoming_path)
                .await
            {
                Ok(entries) => {
                    for entry in entries {
                        consume_spool_file(entry.id.to_string(), &deps).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(event = "spool_reconcile_scan_failed", error = %e, "spool reconcile scan failed");
                }
            }
        }
    });
}
