//! Every rule holds on a deployment broken in its own way, and is clear on
//! a healthy one.

use mailrs_core_api::types::SpoolHealthWire;
use mailrs_ops_report::wire::{Detail, ParsingBeat};

use super::*;
use crate::readings::fixture::healthy;
use crate::readings::{Reading, Snapshot};
use crate::store::Maintenance;

const NOW: i64 = 1_800_000_000;

fn by_name(name: &str) -> &'static Rule {
    all()
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no rule {name}"))
}

fn eval_of(name: &str, s: &Snapshot, object: &str) -> Eval {
    (by_name(name).eval)(s)
        .into_iter()
        .find(|(o, _)| o == object)
        .map(|(_, e)| e)
        .unwrap_or_else(|| panic!("{name} has no object {object}"))
}

fn holds(name: &str, s: &Snapshot, object: &str) {
    let e = eval_of(name, s, object);
    assert!(
        matches!(e, Eval::Holds { .. }),
        "{name}/{object} should hold: {e:?}"
    );
}

fn clear(name: &str, s: &Snapshot, object: &str) {
    let e = eval_of(name, s, object);
    assert!(
        matches!(e, Eval::Clear { .. }),
        "{name}/{object} should be clear: {e:?}"
    );
}

fn webapi(s: &mut Snapshot, path: &[&str], v: serde_json::Value) {
    let Reading::Got(h) = &mut s.webapi else {
        panic!()
    };
    let mut at = h;
    for p in &path[..path.len() - 1] {
        at = &mut at[*p];
    }
    at[path[path.len() - 1]] = v;
}

fn fastcore(s: &mut Snapshot, f: impl Fn(&mut mailrs_core_api::types::HealthResponse)) {
    for r in [&mut s.fastcore, &mut s.fastcore_ready] {
        if let Reading::Got(h) = r {
            f(h);
        }
    }
}

#[test]
fn there_are_seventeen_phase_one_rules_and_each_has_text() {
    let symptoms: Vec<_> = all()
        .into_iter()
        .filter(|r| {
            !r.name.starts_with("source_unreadable.")
                && r.name != KEVY_SERVER_UNAVAILABLE
                && r.name != NO_REPORTS
        })
        .collect();
    assert_eq!(symptoms.len(), 17, "{symptoms:?}");
    for r in all() {
        assert!(
            [r.text.title_en, r.text.title_zh, r.text.title_ja]
                .iter()
                .all(|t| !t.is_empty()),
            "{}",
            r.name
        );
        assert!(
            [r.text.step_en, r.text.step_zh, r.text.step_ja]
                .iter()
                .all(|t| !t.is_empty()),
            "{}",
            r.name
        );
    }
}

#[test]
fn a_healthy_deployment_holds_nothing() {
    let s = healthy(NOW);
    for r in all() {
        for (object, e) in (r.eval)(&s) {
            assert!(
                matches!(e, Eval::Clear { .. }),
                "{}/{object}: {e:?}",
                r.name
            );
        }
    }
}

#[test]
fn quarantine() {
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| {
        h.spool = Some(SpoolHealthWire {
            quarantined: 1,
            oldest_incoming_age_seconds: 0,
        })
    });
    holds("spool_quarantined", &s, "spool");
}

#[test]
fn core_memory_at_85_percent_and_not_below() {
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| {
        h.memory.as_mut().unwrap().used_bytes = 85 * (4 << 30) / 100 + 1
    });
    holds("core_memory_high", &s, "fastcore");
    fastcore(&mut s, |h| {
        h.memory.as_mut().unwrap().used_bytes = 80 * (4 << 30) / 100
    });
    clear("core_memory_high", &s, "fastcore");
}

#[test]
fn kevy_server_memory() {
    let mut s = healthy(NOW);
    webapi(&mut s, &["kevy_server_memory", "used_bytes"], 900.into());
    holds("kevy_server_memory_high", &s, "kevy-server");
}

#[test]
fn accounts_without_quota_and_over_80() {
    let mut s = healthy(NOW);
    webapi(&mut s, &["quota", "accounts_without_limit"], 2.into());
    webapi(&mut s, &["quota", "over_80"], 1.into());
    holds("accounts_without_quota", &s, "accounts");
    holds("accounts_over_80_percent", &s, "accounts");
    webapi(&mut s, &["quota", "stale"], true.into());
    assert!(
        matches!(
            eval_of("accounts_without_quota", &s, "accounts"),
            Eval::Unreadable { .. }
        ),
        "stale counts are not a reading"
    );
}

#[test]
fn new_domains_over_the_configured_rate() {
    let mut s = healthy(NOW);
    webapi(&mut s, &["new_domains", "added_24h"], 5001.into());
    holds("new_domains_burst", &s, "domains");
    s.limits.new_domains_per_day = 6000;
    clear("new_domains_burst", &s, "domains");
}

#[test]
fn a_census_without_its_keys_or_marked_stale_is_unknown_not_zero() {
    let unknown = |s: &Snapshot, rule: &str, object: &str| {
        let e = eval_of(rule, s, object);
        assert!(
            matches!(
                e,
                Eval::Unreadable {
                    source_down: false,
                    ..
                }
            ),
            "{rule}: {e:?}"
        );
    };
    let mut s = healthy(NOW);
    if let Reading::Got(h) = &mut s.webapi {
        h["new_domains"].as_object_mut().unwrap().remove("keys");
    }
    unknown(&s, "new_domains_burst", "domains");
    let mut s = healthy(NOW);
    webapi(&mut s, &["new_domains", "stale"], true.into());
    unknown(&s, "new_domains_burst", "domains");
    let mut s = healthy(NOW);
    if let Reading::Got(h) = &mut s.webapi {
        h["new_domains"].as_object_mut().unwrap().remove("stale");
        h["quota"].as_object_mut().unwrap().remove("accounts");
        h["kevy_server_memory"]
            .as_object_mut()
            .unwrap()
            .remove("used_bytes");
        h["jmap_uploads"]["volume_free_bytes"] = serde_json::Value::Null;
        h["set_aside"]["sender"] = serde_json::Value::Null;
    }
    unknown(&s, "new_domains_burst", "domains");
    unknown(&s, "accounts_without_quota", "accounts");
    unknown(&s, "accounts_over_80_percent", "accounts");
    unknown(&s, "kevy_server_memory_high", "kevy-server");
    unknown(&s, "data_volume_low", "mailrs-data");
    unknown(&s, "work_set_aside", "sender");
    // the core's cgroup unread is no reading; a cgroup without a limit is one
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| h.memory = None);
    unknown(&s, "core_memory_high", "fastcore");
    fastcore(&mut s, |h| {
        h.memory = Some(mailrs_core_api::types::MemoryWire {
            used_bytes: 1,
            limit_bytes: None,
            kevy_used_bytes: None,
            kevy_keys: None,
        })
    });
    clear("core_memory_high", &s, "fastcore");
}

#[test]
fn volume_warns_under_10_gb_and_is_critical_under_2() {
    let mut s = healthy(NOW);
    webapi(
        &mut s,
        &["jmap_uploads", "volume_free_bytes"],
        (5u64 << 30).into(),
    );
    let e = eval_of("data_volume_low", &s, "mailrs-data");
    assert!(matches!(e, Eval::Holds { severity: None, .. }), "{e:?}");
    webapi(
        &mut s,
        &["jmap_uploads", "volume_free_bytes"],
        (1u64 << 30).into(),
    );
    let e = eval_of("data_volume_low", &s, "mailrs-data");
    assert!(
        matches!(
            e,
            Eval::Holds {
                severity: Some(Severity::Critical),
                ..
            }
        ),
        "{e:?}"
    );
}

#[test]
fn spool_stalled_needs_old_mail_and_no_delivery() {
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| {
        h.spool = Some(SpoolHealthWire {
            quarantined: 0,
            oldest_incoming_age_seconds: 400,
        })
    });
    s.drain_still_secs = Some(60);
    clear("spool_stalled", &s, "spool");
    s.drain_still_secs = Some(300);
    holds("spool_stalled", &s, "spool");
}

#[test]
fn set_aside_per_queue() {
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| {
        h.set_aside.as_mut().unwrap().attachment_text = Some(2)
    });
    webapi(&mut s, &["set_aside", "sender"], 1.into());
    holds("work_set_aside", &s, "attachment_text");
    holds("work_set_aside", &s, "sender");
    clear("work_set_aside", &s, "fraud_backfill");
}

#[test]
fn parse_failures_by_rate_and_by_pause() {
    let mut s = healthy(NOW);
    s.core_parse_increase = Some(20);
    holds("parse_failures", &s, "core");
    s.core_parse_increase = Some(19);
    clear("parse_failures", &s, "core");
    if let Some(Detail::Receiver(b)) = &mut s.processes.get_mut("receiver").unwrap().latest.detail {
        b.parsing = Some(ParsingBeat {
            failures_24h: 3,
            paused_since: Some(NOW - 301),
        });
    }
    holds("parse_failures", &s, "receiver");
}

#[test]
fn crash_loop_by_starts_and_by_never_ready() {
    let mut s = healthy(NOW);
    s.processes.get_mut("receiver").unwrap().instances[0].starts_15m = 3;
    holds("crash_loop", &s, "receiver/receiver-1");
    s.processes.get_mut("receiver").unwrap().instances[0].starts_15m = 2;
    clear("crash_loop", &s, "receiver/receiver-1");
    // fastcore is given longer to replay before it counts as not ready
    s.processes.get_mut("fastcore").unwrap().instances[0].waiting_ready_secs = Some(900);
    clear("crash_loop", &s, "fastcore/fastcore-1");
    s.processes.get_mut("sender").unwrap().instances[0].waiting_ready_secs = Some(900);
    holds("crash_loop", &s, "sender/sender-1");
}

#[test]
fn damaged_store_from_the_probe_or_the_ready_report() {
    let mut s = healthy(NOW);
    fastcore(&mut s, |h| h.ready = false);
    holds("core_store_damaged", &s, "fastcore");
    let mut s = healthy(NOW);
    s.fastcore_ready = Reading::Unreadable("refused".into());
    s.processes.get_mut("fastcore").unwrap().latest.detail =
        Some(Detail::CoreReady { boot_intact: false });
    holds("core_store_damaged", &s, "fastcore");
}

#[test]
fn a_process_on_another_build() {
    let mut s = healthy(NOW);
    s.processes.get_mut("sender").unwrap().latest.build.version = "2.213.0".into();
    holds("versions_differ", &s, "deployment");
    // webapi's build is read from its report, the core's from its ops status
    let mut s = healthy(NOW);
    s.processes.get_mut("webapi").unwrap().latest.build.commit = Some("0000000".into());
    holds("versions_differ", &s, "deployment");
    let mut s = healthy(NOW);
    if let Reading::Got(o) = &mut s.fastcore_ops {
        o.build.version = "2.213.0".into();
    }
    holds("versions_differ", &s, "deployment");
    assert!(by_name("versions_differ").for_secs >= 600);
}

#[test]
fn outbound_stuck_by_age_and_by_no_take() {
    let mut s = healthy(NOW);
    let set = |s: &mut Snapshot, pending, oldest, last_pop| {
        if let Some(Detail::Sender(b)) = &mut s.processes.get_mut("sender").unwrap().latest.detail {
            (b.pending, b.oldest_pending_age_secs, b.last_pop_at) = (pending, oldest, last_pop);
        }
    };
    set(&mut s, Some(3), Some(3601), Some(NOW));
    holds("outbound_stuck", &s, "sender");
    set(&mut s, Some(3), Some(60), Some(NOW - 601));
    holds("outbound_stuck", &s, "sender");
    set(&mut s, Some(0), None, Some(NOW - 6000));
    clear("outbound_stuck", &s, "sender");
}

#[test]
fn channel_failing_after_three() {
    let mut s = healthy(NOW);
    s.channels[0].consecutive_failures = 3;
    holds("channel_failing", &s, "webhook");
    s.channels[0].consecutive_failures = 2;
    clear("channel_failing", &s, "webhook");
}

#[test]
fn unwatched_daemon() {
    let mut s = healthy(NOW);
    s.heartbeat_configured = false;
    holds("daemon_unwatched", &s, "ops");
}

#[test]
fn maintenance_open_too_long() {
    let mut s = healthy(NOW);
    let m = |started_at| Maintenance {
        started_at,
        until: NOW + 60,
        reason: "upgrade".into(),
        by: "cli:a".into(),
    };
    s.maintenance = Some(m(NOW - 3600));
    clear("maintenance_too_long", &s, "maintenance");
    s.maintenance = Some(m(NOW - 4 * 3600 - 1));
    holds("maintenance_too_long", &s, "maintenance");
}

#[test]
fn source_and_dependency_rules() {
    let mut s = healthy(NOW);
    s.fastcore = Reading::Unreadable("refused".into());
    holds("source_unreadable.fastcore", &s, "fastcore");
    s.processes.get_mut("receiver").unwrap().fresh = false;
    holds("source_unreadable.receiver", &s, "receiver");
    s.kevy = Reading::Got(false);
    holds(KEVY_SERVER_UNAVAILABLE, &s, "kevy-server");
    for p in s.processes.values_mut() {
        p.fresh = false;
    }
    holds(NO_REPORTS, &s, "reports");
}
