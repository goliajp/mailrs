//! One cycle's reading of everything, with the little memory some rules
//! need across cycles (whether the drain moved, how fast failures grow).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use mailrs_ops_report::wire::Detail;

use super::pull::Puller;
use super::{ChannelView, InstanceView, Limits, ProcessView, Reading, ReportBook, Snapshot};
use crate::config::Config;
use crate::store::{Maintenance, Store, StoredReport};

/// The processes whose kevy-server contact time says whether it answers:
/// both work against it every few seconds.
const KEVY_USERS: &[&str] = &["receiver", "sender"];
/// A contact older than this does not count as "it answers now".
const KEVY_CONTACT_SECS: i64 = 180;
/// Instances not heard from in a day are no longer listed.
const INSTANCE_KEPT_SECS: i64 = 86_400;

/// Three heartbeat intervals.
fn fresh_within(process: &str) -> Option<i64> {
    match process {
        "receiver" | "sender" => Some(90),
        "fetch" | "backup" => Some(180),
        _ => None,
    }
}

pub struct Collector {
    puller: Puller,
    book: Arc<ReportBook>,
    store: Arc<Store>,
    drain: Option<(u64, i64)>,
    core_trend: VecDeque<(i64, u64)>,
    receiver_trend: VecDeque<(i64, u64)>,
    push_sources: Vec<String>,
    limits: Limits,
    heartbeat_configured: bool,
}

/// Growth over the last hour of a count that is read once per cycle.
fn trend(samples: &mut VecDeque<(i64, u64)>, now: i64, value: Option<u64>) -> Option<u64> {
    let v = value?;
    samples.push_back((now, v));
    while samples.front().is_some_and(|(t, _)| *t < now - 3600) {
        samples.pop_front();
    }
    match samples.len() {
        0 | 1 => None,
        _ => samples.front().map(|(_, first)| v.saturating_sub(*first)),
    }
}

impl Collector {
    pub fn new(cfg: &Config, book: Arc<ReportBook>, store: Arc<Store>) -> Self {
        Collector {
            puller: Puller::new(&cfg.fastcore_url, cfg.ops_token.as_deref(), &cfg.webapi_url),
            book,
            store,
            drain: None,
            core_trend: VecDeque::new(),
            receiver_trend: VecDeque::new(),
            push_sources: cfg.push_sources.clone(),
            limits: Limits {
                maintenance_alert_secs: cfg.maintenance_alert_secs,
                new_domains_per_day: cfg.new_domains_per_day,
                ready_secs: 600,
                fastcore_ready_secs: cfg.fastcore_ready_secs,
            },
            heartbeat_configured: cfg.heartbeat_url.is_some(),
        }
    }

    pub async fn collect(
        &mut self,
        now: i64,
        channels: Vec<ChannelView>,
        maintenance: Option<Maintenance>,
    ) -> Snapshot {
        let ((fastcore, fastcore_ready, fastcore_ops), (webapi, webapi_ready)) =
            tokio::join!(self.puller.fastcore(), self.puller.webapi());
        let drain_still_secs = fastcore_ops.got().and_then(|o| {
            let delivered = o.drain.delivered;
            match self.drain {
                Some((was, since)) if was == delivered => Some(now - since),
                Some(_) => {
                    self.drain = Some((delivered, now));
                    Some(0)
                }
                None => {
                    self.drain = Some((delivered, now));
                    None
                }
            }
        });
        let processes = self.processes(now);
        let core_failures = fastcore
            .got()
            .and_then(|h| h.parsing.as_ref())
            .map(|p| p.failures_24h);
        let receiver_failures =
            processes
                .get("receiver")
                .filter(|p| p.fresh)
                .and_then(|p| match &p.latest.detail {
                    Some(Detail::Receiver(b)) => b.parsing.as_ref().map(|x| x.failures_24h),
                    _ => None,
                });
        let kevy = kevy_reading(webapi.got(), &processes, now);
        Snapshot {
            now,
            core_parse_increase: trend(&mut self.core_trend, now, core_failures),
            receiver_parse_increase: trend(&mut self.receiver_trend, now, receiver_failures),
            fastcore,
            fastcore_ready,
            fastcore_ops,
            webapi,
            webapi_ready,
            drain_still_secs,
            processes,
            push_sources: self.push_sources.clone(),
            kevy,
            channels,
            heartbeat_configured: self.heartbeat_configured,
            maintenance,
            limits: self.limits.clone(),
        }
    }

    fn processes(&self, now: i64) -> BTreeMap<String, ProcessView> {
        let mut by: BTreeMap<String, Vec<StoredReport>> = BTreeMap::new();
        for r in self.book.latest() {
            by.entry(r.report.process.clone()).or_default().push(r);
        }
        let mut out = BTreeMap::new();
        for (name, mut reports) in by {
            reports.sort_by_key(|r| std::cmp::Reverse(r.received_at));
            let latest = reports[0].clone();
            let age = now - latest.received_at;
            let instances = reports
                .iter()
                .enumerate()
                .filter(|(i, r)| *i == 0 || now - r.received_at < INSTANCE_KEPT_SECS)
                .map(|(i, r)| self.instance(r, i == 0, now))
                .collect();
            out.insert(
                name.clone(),
                ProcessView {
                    fresh: fresh_within(&name).is_some_and(|w| age <= w),
                    age_secs: age,
                    latest: latest.report,
                    instances,
                },
            );
        }
        out
    }

    fn instance(&self, r: &StoredReport, current: bool, now: i64) -> InstanceView {
        let (p, i) = (&r.report.process, &r.report.instance);
        let window = now - 900;
        let starts_15m = match self.store.starts_since(p, i, window) {
            Ok(rows) => rows
                .iter()
                .filter(|s| match s.how.as_str() {
                    "reported" => s.at >= window,
                    "unseen" => s.between_from >= window,
                    _ => false,
                })
                .map(|s| s.starts)
                .sum(),
            Err(e) => {
                tracing::error!(error = %e, "start rows not read");
                0
            }
        };
        let waiting = match self.store.ready_seen(p, i, r.report.start_count) {
            Ok(ready) => current && !ready,
            Err(e) => {
                tracing::error!(error = %e, "ready reports not read");
                false
            }
        };
        InstanceView {
            instance: i.clone(),
            starts_15m,
            waiting_ready_secs: waiting.then(|| now - r.report.build.started_at),
        }
    }
}

/// Whether kevy-server answers, from webapi's ping and from the contact
/// times in receiver and sender heartbeats. Down only when every path that
/// can be read says so; unreadable only when none can.
fn kevy_reading(
    webapi: Option<&serde_json::Value>,
    processes: &BTreeMap<String, ProcessView>,
    now: i64,
) -> Reading<bool> {
    let by_webapi = webapi.and_then(|h| h["kevy"].as_bool());
    let fresh: Vec<&ProcessView> = KEVY_USERS
        .iter()
        .filter_map(|p| processes.get(*p))
        .filter(|p| p.fresh)
        .collect();
    let by_reports = match fresh.is_empty() {
        true => None,
        false => Some(fresh.iter().any(|p| {
            p.latest
                .kevy_contact_at
                .is_some_and(|t| now - t <= KEVY_CONTACT_SECS)
        })),
    };
    match (by_webapi, by_reports) {
        (None, None) => {
            Reading::Unreadable("neither webapi nor a receiver or sender heartbeat says".into())
        }
        (Some(a), None) | (None, Some(a)) => Reading::Got(a),
        (Some(a), Some(b)) => Reading::Got(a || b),
    }
}
