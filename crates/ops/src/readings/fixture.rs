//! A healthy deployment as one snapshot, for the rule and engine tests to
//! break one thing at a time.

use std::collections::BTreeMap;

use mailrs_core_api::method::ops::{DrainProgressWire, OpsStatusWire};
use mailrs_core_api::types::{
    BackendKind, HealthResponse, MemoryWire, ParsingHealthWire, SetAsideWire, SpoolHealthWire,
};
use mailrs_ops_report::build::BuildInfo;
use mailrs_ops_report::wire::{
    BackupBeat, Detail, FetchBeat, ParsingBeat, ReceiverBeat, Report, ReportKind, SenderBeat,
};

use super::{ChannelView, InstanceView, ProcessView, Reading, Snapshot};

pub fn build() -> BuildInfo {
    BuildInfo {
        version: "2.214.0".into(),
        commit: Some("abcdef1234567890".into()),
        started_at: 1_000,
    }
}

pub fn health() -> HealthResponse {
    HealthResponse {
        version: "v1".into(),
        backend: BackendKind::Kevy,
        ready: true,
        memory: Some(MemoryWire {
            used_bytes: 1 << 30,
            limit_bytes: Some(4 << 30),
            kevy_used_bytes: None,
            kevy_keys: None,
        }),
        spool: Some(SpoolHealthWire {
            quarantined: 0,
            oldest_incoming_age_seconds: 0,
        }),
        set_aside: Some(SetAsideWire {
            attachment_text: Some(0),
            fraud_backfill: None,
        }),
        parsing: Some(ParsingHealthWire {
            worker: true,
            failures_24h: 0,
            last_failure_at: None,
            worker_restarts: 0,
            failures: vec![],
            paused_since: None,
        }),
    }
}

pub fn webapi(now: i64) -> serde_json::Value {
    serde_json::json!({
        "status": "healthy",
        "kevy": true,
        "kevy_server_memory": { "used_bytes": 100, "limit_bytes": 1000, "high": false },
        "quota": { "accounts": 5, "accounts_without_limit": 0, "over_80": 0, "computed_at": now, "stale": false },
        "new_domains": { "keys": 100, "added_24h": 3, "computed_at": now, "stale": false },
        "jmap_uploads": { "volume_free_bytes": 500u64 << 30 },
        "set_aside": { "attachment_text": 0, "fraud_backfill": null, "sender": 0 },
    })
}

pub fn report(process: &str, now: i64, detail: Option<Detail>) -> Report {
    Report {
        process: process.into(),
        instance: format!("{process}-1"),
        build: build(),
        start_count: Some(1),
        seq: 5,
        kind: ReportKind::Heartbeat,
        sent_at: now,
        kevy_contact_at: Some(now - 5),
        detail,
    }
}

fn view(process: &str, now: i64, fresh: bool, detail: Option<Detail>) -> ProcessView {
    ProcessView {
        fresh,
        age_secs: 10,
        latest: report(process, now, detail),
        instances: vec![InstanceView {
            instance: format!("{process}-1"),
            starts_15m: 0,
            waiting_ready_secs: None,
        }],
    }
}

pub fn healthy(now: i64) -> Snapshot {
    let mut processes = BTreeMap::new();
    let receiver = Detail::Receiver(ReceiverBeat {
        active_connections: 0,
        total_connections: 10,
        total_messages: 3,
        parsing: Some(ParsingBeat {
            failures_24h: 0,
            paused_since: None,
        }),
    });
    let sender = Detail::Sender(SenderBeat {
        last_pop_at: Some(now - 3),
        last_job_at: Some(now - 100),
        pending: Some(0),
        oldest_pending_age_secs: None,
        outcomes: BTreeMap::new(),
        ops_alerts_delivered: vec![],
    });
    let fetch = Detail::Fetch(FetchBeat {
        fetched: 1,
        refused: 0,
        in_flight: 0,
    });
    let backup = Detail::Backup(BackupBeat {
        last_run_at: None,
        last_ok: None,
        last_success_at: None,
    });
    processes.insert(
        "receiver".into(),
        view("receiver", now, true, Some(receiver)),
    );
    processes.insert("sender".into(), view("sender", now, true, Some(sender)));
    processes.insert("fetch".into(), view("fetch", now, true, Some(fetch)));
    processes.insert("backup".into(), view("backup", now, true, Some(backup)));
    processes.insert("webapi".into(), view("webapi", now, false, None));
    processes.insert(
        "fastcore".into(),
        view(
            "fastcore",
            now,
            false,
            Some(Detail::CoreReady { boot_intact: true }),
        ),
    );
    let mut s = Snapshot::empty(now);
    s.fastcore = Reading::Got(health());
    s.fastcore_ready = Reading::Got(health());
    s.fastcore_ops = Reading::Got(OpsStatusWire {
        build: build(),
        fetch_starts_witnessed: 1,
        drain: DrainProgressWire {
            ticks: 10,
            delivered: 4,
            last_tick_at: Some(now),
        },
        quarantined_total: 0,
        quarantined: vec![],
    });
    s.webapi = Reading::Got(webapi(now));
    s.webapi_ready = Reading::Got(true);
    s.drain_still_secs = Some(0);
    s.core_parse_increase = Some(0);
    s.receiver_parse_increase = Some(0);
    s.processes = processes;
    s.push_sources = ["receiver", "sender", "fetch", "backup"]
        .map(String::from)
        .to_vec();
    s.kevy = Reading::Got(true);
    s.channels = vec![ChannelView {
        name: "webhook".into(),
        consecutive_failures: 0,
    }];
    s
}
