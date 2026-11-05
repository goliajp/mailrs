//! What this process's parse workers have gone through, for the health
//! probe: failures in the last day, the last one, restarts, and the count
//! per parser and kind that `parser_failure_total` also carries.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::proto::Parser;

const DAY: i64 = 24 * 3600;

/// How a worker failed on a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// The parser panicked (the worker aborted).
    Panic,
    /// It ran past the wall-clock limit and was killed for it.
    Timeout,
    /// Something else ended it: a signal, its own memory or CPU limit.
    Killed,
    /// It answered with an error, or with something that is not an answer.
    Error,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Panic => "panic",
            Kind::Timeout => "timeout",
            Kind::Killed => "killed",
            Kind::Error => "error",
        }
    }
}

/// A snapshot for the health probe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Whether parsing runs in workers at all; `false` in a process that
    /// never installed them, which parses in process.
    pub worker: bool,
    pub failures_24h: u64,
    /// Unix seconds.
    pub last_failure_at: Option<i64>,
    /// Workers started again after one died or was killed.
    pub worker_restarts: u64,
    /// `(parser, kind) -> count` since this process started.
    pub failures: BTreeMap<(&'static str, &'static str), u64>,
}

struct Failures {
    recent: VecDeque<i64>,
    by: BTreeMap<(&'static str, &'static str), u64>,
}

static FAILURES: Mutex<Failures> = Mutex::new(Failures {
    recent: VecDeque::new(),
    by: BTreeMap::new(),
});
static RESTARTS: AtomicU64 = AtomicU64::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
static PROCESS: Mutex<&'static str> = Mutex::new("unknown");

pub(crate) fn set_installed(process: &'static str) {
    INSTALLED.store(true, Ordering::Relaxed);
    if let Ok(mut p) = PROCESS.lock() {
        *p = process;
    }
}

pub(crate) fn record_failure(parser: Parser, kind: Kind) {
    let process = PROCESS.lock().map(|p| *p).unwrap_or("unknown");
    metrics::counter!(
        "parser_failure_total",
        "process" => process,
        "parser" => parser.as_str(),
        "kind" => kind.as_str()
    )
    .increment(1);
    let now = now();
    if let Ok(mut f) = FAILURES.lock() {
        f.recent.push_back(now);
        while f.recent.front().is_some_and(|t| *t <= now - DAY) {
            f.recent.pop_front();
        }
        *f.by.entry((parser.as_str(), kind.as_str())).or_default() += 1;
    }
}

pub(crate) fn record_restart() {
    RESTARTS.fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot() -> Snapshot {
    let now = now();
    let (failures_24h, last_failure_at, failures) = match FAILURES.lock() {
        Ok(f) => (
            f.recent.iter().filter(|t| **t > now - DAY).count() as u64,
            f.recent.back().copied(),
            f.by.clone(),
        ),
        Err(_) => (0, None, BTreeMap::new()),
    };
    Snapshot {
        worker: INSTALLED.load(Ordering::Relaxed),
        failures_24h,
        last_failure_at,
        worker_restarts: RESTARTS.load(Ordering::Relaxed),
        failures,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
