//! The daemon's parts, and one check cycle.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use crate::clock::Clock;
use crate::config::Config;
use crate::engine::Engine;
use crate::notify::Dispatcher;
use crate::readings::{Collector, Polls, ReportBook, Snapshot};
use crate::store::{CycleRow, Store};

/// A test message goes to every channel at least this often.
const CHANNEL_TEST_EVERY: i64 = 7 * 86_400;

/// What the last cycle read that the state view shows beside the alerts.
#[derive(Debug, Clone, Default)]
struct Seen {
    polls: Polls,
    /// fetch starts the core relayed, and since when it has been counting.
    fetch_witnessed: Option<(u64, i64)>,
}

pub struct App {
    pub cfg: Config,
    pub clock: Clock,
    pub store: Arc<Store>,
    pub book: Arc<ReportBook>,
    pub engine: std::sync::Mutex<Engine>,
    pub dispatcher: tokio::sync::Mutex<Dispatcher>,
    collector: tokio::sync::Mutex<Collector>,
    /// From the last cycle, for the state view without waiting on a read.
    seen: std::sync::Mutex<Seen>,
    http: reqwest::Client,
    /// Unix seconds the current or last cycle started and finished, for
    /// the watchdog.
    pub cycle_started: AtomicI64,
    pub cycle_finished: AtomicI64,
}

/// With `MAILRS_OPS_TEST_STALL` set, every cycle after the first never
/// finishes: what a cycle stuck on a read looks like to the watchdog.
#[cfg(feature = "stall-hook")]
async fn stall_hook() {
    static CYCLES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let first = CYCLES.fetch_add(1, Ordering::Relaxed) == 0;
    if !first && std::env::var_os("MAILRS_OPS_TEST_STALL").is_some() {
        std::future::pending::<()>().await;
    }
}

impl App {
    pub fn open(cfg: Config, clock: Clock, store: Store) -> Result<Arc<App>, String> {
        let store = Arc::new(store);
        let now = clock.now();
        let book =
            Arc::new(ReportBook::load(store.clone(), cfg.master_key.as_deref()).map_err(|e| e.0)?);
        let engine = Engine::load(store.clone(), cfg.unknown_wait_secs, now).map_err(|e| e.0)?;
        let dispatcher = Dispatcher::new(&cfg, store.clone());
        let collector = Collector::new(&cfg, book.clone(), store.clone());
        Ok(Arc::new(App {
            cfg,
            clock,
            store,
            book,
            engine: std::sync::Mutex::new(engine),
            dispatcher: tokio::sync::Mutex::new(dispatcher),
            collector: tokio::sync::Mutex::new(collector),
            seen: std::sync::Mutex::new(Seen::default()),
            http: reqwest::Client::new(),
            cycle_started: AtomicI64::new(mailrs_ops_report::build::now()),
            cycle_finished: AtomicI64::new(mailrs_ops_report::build::now()),
        }))
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// One complete check: read everything, advance every alert, queue and
    /// send what is due, record the cycle, then the external heartbeat.
    pub async fn cycle(&self) -> Snapshot {
        let started = Instant::now();
        let now = self.clock.now();
        self.cycle_started
            .store(mailrs_ops_report::build::now(), Ordering::Relaxed);
        #[cfg(feature = "stall-hook")]
        stall_hook().await;
        self.close_expired(now).await;
        self.test_channels_when_due(now).await;
        let maintenance = self.store.maintenance(now).unwrap_or_else(|e| {
            tracing::error!(error = %e, "maintenance state not read");
            None
        });
        let silences = self.store.silences(now).unwrap_or_else(|e| {
            tracing::error!(error = %e, "silences not read");
            Vec::new()
        });
        let channels = self.dispatcher.lock().await.channel_views();
        let (snap, polls) = {
            let mut c = self.collector.lock().await;
            let snap = c.collect(now, channels, maintenance.clone()).await;
            (snap, c.polls())
        };
        {
            let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
            seen.polls = polls;
            if let Some(o) = snap.fastcore_ops.got() {
                seen.fetch_witnessed = Some((o.fetch_starts_witnessed, o.build.started_at));
            }
        }
        let (notices, critical) = {
            let mut engine = self.engine();
            let notices = engine.evaluate(&snap);
            (notices, engine.critical_firing())
        };
        {
            let mut d = self.dispatcher.lock().await;
            d.mark_accepted(&snap.ops_alerts_accepted, now);
            d.accept(&notices, &silences, maintenance.is_some(), now);
            d.tick(now).await;
        }
        let row = CycleRow {
            started_at: now,
            duration_ms: started.elapsed().as_millis() as i64,
            sources: serde_json::json!({
                "fastcore": snap.fastcore.got().is_some(),
                "webapi": snap.webapi.got().is_some(),
                "reports": snap.processes.iter().filter(|(_, p)| p.fresh).map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            }),
        };
        if let Err(e) = self.store.record_cycle(&row) {
            tracing::error!(error = %e, "cycle not recorded");
        }
        if let Err(e) = self.store.prune(now) {
            tracing::error!(error = %e, "store not pruned");
        }
        self.cycle_finished
            .store(mailrs_ops_report::build::now(), Ordering::Relaxed);
        self.heartbeat(&row, critical).await;
        snap
    }

    /// A test message to every channel, recorded with the configuration it
    /// went to.
    pub async fn test_channels(&self, now: i64) -> Vec<(String, String)> {
        let mut d = self.dispatcher.lock().await;
        let results = d.test(now).await;
        if let Some(config) = d.config_fingerprint() {
            let json = serde_json::json!(results);
            if let Err(e) = self.store.record_channel_test(&config, &json, now) {
                tracing::error!(error = %e, "channel test not recorded");
            }
        }
        results
    }

    /// Test the channels when their configuration changed since the last
    /// test, and otherwise once a week.
    async fn test_channels_when_due(&self, now: i64) {
        let Some(config) = self.dispatcher.lock().await.config_fingerprint() else {
            return;
        };
        let due = match self.store.last_channel_test() {
            Ok(Some((was, at))) => was != config || now - at >= CHANNEL_TEST_EVERY,
            Ok(None) => true,
            Err(e) => {
                tracing::error!(error = %e, "last channel test not read");
                false
            }
        };
        if due {
            let results = self.test_channels(now).await;
            tracing::info!(?results, "notification channels tested");
        }
    }

    /// Send what is queued once its grouping window is over.
    pub async fn flush(&self) {
        let now = self.clock.now();
        self.dispatcher.lock().await.tick(now).await;
    }

    async fn close_expired(&self, now: i64) {
        match self.store.expire_controls(now) {
            Ok(Some(_)) => self.dispatcher.lock().await.maintenance_ended(now).await,
            Ok(None) => {}
            Err(e) => tracing::error!(error = %e, "expired silences and maintenance not closed"),
        }
    }

    /// The heartbeat goes out only after a completed cycle, so a daemon
    /// that is stuck stops sending it.
    async fn heartbeat(&self, cycle: &CycleRow, critical: usize) {
        let Some(url) = &self.cfg.heartbeat_url else {
            return;
        };
        let status = match critical {
            0 => "ok",
            _ => "critical",
        };
        let body = serde_json::json!({
            "status": status,
            "critical_firing": critical,
            "cycle_started_at": cycle.started_at,
            "cycle_ms": cycle.duration_ms,
        });
        let sent = self
            .http
            .post(url)
            .timeout(std::time::Duration::from_secs(10))
            .json(&body)
            .send()
            .await;
        match sent {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => tracing::warn!(status = %r.status(), "heartbeat refused"),
            Err(e) => tracing::warn!(error = %e, "heartbeat not sent"),
        }
    }

    pub async fn state(&self) -> Result<serde_json::Value, String> {
        let now = self.clock.now();
        let alerts = self.engine().view();
        let seen = self.seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let processes: Vec<_> = self
            .book
            .latest()
            .into_iter()
            .map(|r| {
                // fetch has no counter of its own; the core counts the
                // start reports it relays, which is a weaker reading
                let (count, source) = match (r.report.process.as_str(), r.report.start_count) {
                    ("fetch", _) => (
                        seen.fetch_witnessed.map(|(n, _)| n),
                        "witnessed by fastcore since it started (unverified, weaker than self-reported)",
                    ),
                    (_, Some(n)) => (Some(n), "self-reported"),
                    (_, None) => (None, "not kept"),
                };
                serde_json::json!({
                    "process": r.report.process,
                    "instance": r.report.instance,
                    "kind": r.report.kind,
                    "start_count": count,
                    "start_count_source": source,
                    "build": r.report.build,
                    "received_at": r.received_at,
                    "age_secs": now - r.received_at,
                    "kevy_contact_at": r.report.kevy_contact_at,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "now": now,
            "build": mailrs_ops_report::build::build(),
            "last_cycle": self.store.last_cycle().map_err(|e| e.0)?,
            "unmonitored": self.cfg.heartbeat_url.is_none(),
            "maintenance": self.store.maintenance(now).map_err(|e| e.0)?,
            "silences": self.store.silences(now).map_err(|e| e.0)?,
            "alerts": alerts,
            "notifications": self.dispatcher.lock().await.status(),
            "warnings": self.warnings().await,
            "report_rejections": self.store.rejection_counts().map_err(|e| e.0)?,
            "admin_refusals": self.store.denial_counts().map_err(|e| e.0)?,
            "ops_status_polls": seen.polls,
            "processes": processes,
        }))
    }

    /// What the operator must know about how far alerts can reach.
    pub async fn warnings(&self) -> Vec<&'static str> {
        match self.dispatcher.lock().await.has_external() {
            true => Vec::new(),
            false => vec![crate::notify::NO_EXTERNAL_WARNING],
        }
    }

    pub fn history(&self, since: i64, limit: u32) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "since": since,
            "transitions": self.store.transitions_since(since, limit).map_err(|e| e.0)?,
            "notifications": self.store.notifications_since(since, limit).map_err(|e| e.0)?,
            "controls": self.store.control_events_since(since).map_err(|e| e.0)?,
            "starts": self.store.all_starts_since(since).map_err(|e| e.0)?,
        }))
    }
}
