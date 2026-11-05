//! How far the drain has got since the process started, and what a core
//! answers on `/v1/ops/status`.
//!
//! The counters only ever go up, so the daemon can tell "the drain is not
//! getting through" (the oldest waiting file keeps aging while `delivered`
//! stands still) from "nothing has arrived" (no file waits at all).

use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use mailrs_core_api::method::ops::{
    DrainProgressWire, MAX_QUARANTINE_RECORDS, OpsStatusWire, QuarantinedWire,
};

static TICKS: AtomicU64 = AtomicU64::new(0);
static DELIVERED: AtomicU64 = AtomicU64::new(0);
static LAST_TICK_AT: AtomicI64 = AtomicI64::new(0);

pub(crate) fn record_tick(delivered: usize) {
    DELIVERED.fetch_add(delivered as u64, Ordering::Relaxed);
    TICKS.fetch_add(1, Ordering::Relaxed);
    LAST_TICK_AT.store(mailrs_ops_report::build::now(), Ordering::Relaxed);
}

pub fn drain_progress() -> DrainProgressWire {
    DrainProgressWire {
        ticks: TICKS.load(Ordering::Relaxed),
        delivered: DELIVERED.load(Ordering::Relaxed),
        last_tick_at: match LAST_TICK_AT.load(Ordering::Relaxed) {
            0 => None,
            t => Some(t),
        },
    }
}

/// The ops status of the spool under `spool_root`, for either core.
pub fn ops_status(spool_root: &Path) -> OpsStatusWire {
    let (total, mut records) = super::quarantine::records(spool_root);
    records.sort_by_key(|q| std::cmp::Reverse(q.quarantined_at));
    records.truncate(MAX_QUARANTINE_RECORDS);
    OpsStatusWire {
        build: mailrs_ops_report::build::build(),
        fetch_starts_witnessed: mailrs_core_api::method::ops::fetch_starts_witnessed(),
        drain: drain_progress(),
        quarantined_total: total,
        quarantined: records
            .into_iter()
            .map(|q| QuarantinedWire {
                file: q.file,
                quarantined_at: q.quarantined_at,
                reverse_path: q.reverse_path,
                forward_paths: q.forward_paths,
                size: q.size,
            })
            .collect(),
    }
}
