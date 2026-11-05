//! What the sender tells the ops daemon: its start (before it first talks
//! to kevy-server, so a sender that cannot reach it still shows its
//! restarts), that it is ready, and every 30 seconds how far it has got.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use mailrs_core_sidestate::families::outbound as ob;
use mailrs_ops_report::push::Reporter;
use mailrs_ops_report::wire::{Detail, SenderBeat};

use super::config::Cfg;
use super::{PENDING_IDX_KEY, kevy, now_secs};

const HEARTBEAT: Duration = Duration::from_secs(30);

static LAST_POP_AT: AtomicI64 = AtomicI64::new(0);
static LAST_JOB_AT: AtomicI64 = AtomicI64::new(0);
static OUTCOMES: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// The ops alerts delivered lately, oldest first.
static OPS_ALERTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Alert ids a heartbeat carries; ten minutes of alerts at most.
const OPS_ALERTS_KEPT: usize = 50;

pub(super) fn start() -> Reporter {
    Reporter::start("sender")
}

/// The main loop came back from the queue, with a job or without.
pub(super) fn popped(job: bool) {
    let now = now_secs();
    LAST_POP_AT.store(now, Ordering::Relaxed);
    if job {
        LAST_JOB_AT.store(now, Ordering::Relaxed);
    }
    mailrs_ops_report::kevy_contact();
}

/// One delivery attempt ended this way; a delivered ops alert is
/// remembered for the heartbeat.
pub(super) fn outcome(kind: &str, envelope: &serde_json::Value) {
    if let Ok(mut o) = OUTCOMES.lock() {
        *o.entry(kind.to_string()).or_default() += 1;
    }
    let alert = envelope[mailrs_core_sidestate::families::outbound::OPS_ALERT_ID].as_str();
    if let (Some(id), "delivered", Ok(mut kept)) = (alert, kind, OPS_ALERTS.lock()) {
        if kept.len() == OPS_ALERTS_KEPT {
            kept.pop_front();
        }
        kept.push_back(id.to_string());
    }
}

fn at(v: &AtomicI64) -> Option<i64> {
    match v.load(Ordering::Relaxed) {
        0 => None,
        t => Some(t),
    }
}

/// Jobs waiting, and the age of the next one to be taken (the list's
/// tail is popped first, so it is the oldest).
fn queue(cfg: &Cfg) -> std::io::Result<(u64, Option<u64>)> {
    let mut c = kevy(&cfg.kevy_url)?;
    let pending = c.llen(PENDING_IDX_KEY)? as u64;
    let oldest = match c.lrange(PENDING_IDX_KEY, -1, -1)?.first() {
        None => None,
        Some(id) => c
            .hget(ob::job_key(String::from_utf8_lossy(id)).as_bytes(), b"blob")?
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["created_at"].as_i64())
            .map(|created| (now_secs() - created).max(0) as u64),
    };
    Ok((pending, oldest))
}

pub(super) fn spawn_heartbeat(cfg: std::sync::Arc<Cfg>, reporter: &Reporter) {
    reporter.spawn_heartbeat(HEARTBEAT, move || {
        let read = tokio::task::block_in_place(|| queue(&cfg));
        let (pending, oldest) = match read {
            Ok((p, o)) => (Some(p), o),
            Err(e) => {
                tracing::warn!(err = %e, "ops heartbeat: queue not read");
                (None, None)
            }
        };
        let outcomes = OUTCOMES.lock().map(|o| o.clone()).unwrap_or_default();
        Some(Detail::Sender(SenderBeat {
            last_pop_at: at(&LAST_POP_AT),
            last_job_at: at(&LAST_JOB_AT),
            pending,
            oldest_pending_age_secs: oldest,
            outcomes,
            ops_alerts_delivered: OPS_ALERTS
                .lock()
                .map(|k| k.iter().cloned().collect())
                .unwrap_or_default(),
        }))
    });
}
