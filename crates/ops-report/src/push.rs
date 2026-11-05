//! The sender a process pushes its reports with.
//!
//! Best effort by design: a report that cannot be delivered is retried in
//! memory for at most two minutes and then dropped. Nothing waits on it —
//! not the process's start, not fastcore's replay.
//!
//! Reports go out one at a time, in the order they were made: the daemon
//! refuses a sequence number lower than one it has seen, so a start report
//! overtaken by the heartbeat right after it would be refused as a replay.
//!
//! Configuration, all optional:
//! - `MAILRS_OPS_URL`: the daemon's base URL (`http://ops:3600`)
//! - `MAILRS_OPS_REPORT_KEY`: this process's key (`mailrs-ops report-key`)
//! - `MAILRS_OPS_STATE_DIR`: where the start counter and liveness file live
//! - `MAILRS_OPS_REPORTS_URL`: the full URL to post to, in place of
//!   `MAILRS_OPS_URL` + `/v1/reports`; fetch posts to the core's relay
//!
//! Without the URL or the key nothing is sent; the counter and the liveness
//! file are still kept when the directory is set, so starts made while the
//! daemon was not configured are counted all the same.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::counter::{self, Started};
use crate::wire::{Detail, REPORTS_PATH, Report, ReportKind, SIGNATURE_HEADER};

/// How long one report is retried for.
const GIVE_UP_AFTER: Duration = Duration::from_secs(120);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

/// Where reports go and what they are signed with.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub url: Option<String>,
    /// Posted to as it is, instead of `url` + `/v1/reports`.
    pub reports_url: Option<String>,
    pub key: Option<String>,
    pub state_dir: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            url: var("MAILRS_OPS_URL"),
            reports_url: var("MAILRS_OPS_REPORTS_URL"),
            key: var("MAILRS_OPS_REPORT_KEY"),
            state_dir: var("MAILRS_OPS_STATE_DIR").map(PathBuf::from),
        }
    }
}

struct Target {
    url: String,
    key: String,
    client: reqwest::Client,
    /// The one task delivering this reporter's reports, started with the
    /// first report.
    queue: OnceLock<tokio::sync::mpsc::UnboundedSender<(reqwest::RequestBuilder, ReportKind)>>,
}

struct Inner {
    process: &'static str,
    started: Started,
    state_dir: Option<PathBuf>,
    target: Option<Target>,
    seq: AtomicU64,
}

/// One process's reporter. Cheap to clone.
#[derive(Clone)]
pub struct Reporter(Arc<Inner>);

impl Reporter {
    /// Count this start and push the start report, configured from the
    /// environment.
    pub fn start(process: &'static str) -> Self {
        Self::start_with(process, Config::from_env())
    }

    pub fn start_with(process: &'static str, cfg: Config) -> Self {
        crate::build::mark_start();
        let started = counter::start(cfg.state_dir.as_deref());
        let to = cfg.reports_url.or_else(|| {
            cfg.url
                .map(|u| format!("{}{REPORTS_PATH}", u.trim_end_matches('/')))
        });
        let target = match (to, cfg.key) {
            (Some(url), Some(key)) => Some(Target {
                url,
                key,
                client: reqwest::Client::new(),
                queue: OnceLock::new(),
            }),
            _ => {
                tracing::info!(
                    process,
                    "ops reports off: MAILRS_OPS_URL (or _REPORTS_URL) or MAILRS_OPS_REPORT_KEY is not set"
                );
                None
            }
        };
        let r = Reporter(Arc::new(Inner {
            process,
            started,
            state_dir: cfg.state_dir,
            target,
            seq: AtomicU64::new(0),
        }));
        tracing::info!(
            process,
            instance = %r.0.started.instance,
            start_count = ?r.0.started.start_count,
            "ops start counted"
        );
        r.send(ReportKind::Start, None);
        r
    }

    pub fn instance(&self) -> &str {
        &self.0.started.instance
    }

    pub fn start_count(&self) -> Option<u64> {
        self.0.started.start_count
    }

    pub fn ready(&self, detail: Option<Detail>) {
        self.send(ReportKind::Ready, detail);
    }

    pub fn result(&self, detail: Detail) {
        self.send(ReportKind::Result, Some(detail));
    }

    /// Touch the liveness file and push a heartbeat.
    pub fn heartbeat(&self, detail: Option<Detail>) {
        if let Some(dir) = &self.0.state_dir
            && let Err(e) = counter::touch_alive(dir)
        {
            tracing::warn!(error = %e, "ops liveness file not written");
        }
        self.send(ReportKind::Heartbeat, detail);
    }

    /// Heartbeat every `every`, with the detail `detail` builds each time.
    pub fn spawn_heartbeat<F>(&self, every: Duration, detail: F)
    where
        F: Fn() -> Option<Detail> + Send + 'static,
    {
        let me = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                me.heartbeat(detail());
            }
        });
    }

    /// The report as it would be sent now, advancing the sequence number.
    pub fn build(&self, kind: ReportKind, detail: Option<Detail>) -> Report {
        Report {
            process: self.0.process.to_string(),
            instance: self.0.started.instance.clone(),
            build: crate::build::build(),
            start_count: self.0.started.start_count,
            seq: self.0.seq.fetch_add(1, Ordering::Relaxed) + 1,
            kind,
            sent_at: crate::build::now(),
            kevy_contact_at: crate::kevy_contact_at(),
            detail,
        }
    }

    fn send(&self, kind: ReportKind, detail: Option<Detail>) {
        let Some(target) = &self.0.target else {
            return;
        };
        let report = self.build(kind, detail);
        let Ok(body) = serde_json::to_vec(&report) else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(kind = kind.as_str(), "ops report not sent: no runtime");
            return;
        };
        let signature = crate::key::sign(&target.key, &body);
        let request = target
            .client
            .post(&target.url)
            .header(SIGNATURE_HEADER, signature)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(ATTEMPT_TIMEOUT)
            .body(body);
        let queue = target.queue.get_or_init(|| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            handle.spawn(async move {
                while let Some((request, kind)) = rx.recv().await {
                    deliver(request, kind).await;
                }
            });
            tx
        });
        if queue.send((request, kind)).is_err() {
            tracing::warn!(
                kind = kind.as_str(),
                "ops report not sent: its delivery task has ended"
            );
        }
    }
}

async fn deliver(request: reqwest::RequestBuilder, kind: ReportKind) {
    let deadline = tokio::time::Instant::now() + GIVE_UP_AFTER;
    let mut wait = Duration::from_secs(2);
    loop {
        let Some(attempt) = request.try_clone() else {
            return;
        };
        match attempt.send().await {
            Ok(resp) if resp.status().is_success() => return,
            // refused for what it says, not for being early: sending it
            // again would be refused again
            Ok(resp) if resp.status().is_client_error() => {
                tracing::warn!(kind = kind.as_str(), status = %resp.status(), "ops report refused");
                return;
            }
            Ok(_) | Err(_) => {}
        }
        if tokio::time::Instant::now() + wait > deadline {
            tracing::debug!(kind = kind.as_str(), "ops report dropped after two minutes");
            return;
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests;
