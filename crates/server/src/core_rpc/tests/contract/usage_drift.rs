//! The usage-counter correction, alike on both cores: a counter that is
//! wrong and one left for an address that is not an account are reported
//! the same way, corrected the same way, with an audit row each, and a
//! second pass finds nothing to do.

use mailrs_core_api::method::admin::UsageDriftAction;
use mailrs_core_sidestate::families::audit::AuditFact;
use mailrs_core_sidestate::families::usage_drift::USAGE_CORRECTED;
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use mailrs_core_sidestate::keys::quota::used_key;

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

const STORES: [(&str, &str); 2] = [("fastcore", FASTCORE_KEVY), ("pg-core", PG_CORE_KEVY)];
const GONE: &str = "gone@example.test";

fn store(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("side state")
}

fn counter(url: &str, user: &str) -> Option<i64> {
    store(url)
        .get(used_key(user).as_bytes())
        .expect("read counter")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
}

fn corrections(url: &str) -> Vec<String> {
    store(url)
        .lrange(AUDIT_LOG, 0, -1)
        .expect("audit log")
        .iter()
        .filter_map(|r| serde_json::from_slice::<AuditFact>(r).ok())
        .filter(|f| f.action == USAGE_CORRECTED)
        .map(|f| f.target)
        .collect()
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    for (_, url) in STORES {
        let mut s = store(url);
        s.set(used_key(ALICE).as_bytes(), b"1")
            .expect("skew a counter");
        s.set(used_key(GONE).as_bytes(), b"64")
            .expect("leave an orphan");
    }

    let dry = d.both(
        "usage_drift report",
        c.fastcore.usage_drift().await,
        c.pg_core.usage_drift().await,
        &[],
    );
    let (dry, _) = dry.expect("the report answers on both cores");
    let alice = dry
        .rows
        .iter()
        .find(|r| r.account == ALICE)
        .map(|r| r.action);
    assert_eq!(alice, Some(UsageDriftAction::WouldWrite));
    let orphans: Vec<&str> = dry
        .orphan_counters
        .iter()
        .map(|o| o.address.as_str())
        .collect();
    assert_eq!(orphans, [GONE]);
    for (core, url) in STORES {
        assert_eq!(counter(url, ALICE), Some(1), "{core} wrote on a report");
        assert_eq!(counter(url, GONE), Some(64), "{core} deleted on a report");
    }

    let done = d.both(
        "usage_drift correct",
        c.fastcore.correct_usage(false).await,
        c.pg_core.correct_usage(false).await,
        &[],
    );
    let (done, _) = done.expect("the correction answers on both cores");
    assert_eq!((done.orphans_deleted, done.skipped_raced), (1, 0));
    assert!(done.raced.is_empty());
    for (core, url) in STORES {
        assert_eq!(counter(url, GONE), None, "{core} kept the orphan counter");
        let audited = corrections(url);
        assert!(
            audited.iter().any(|t| t == ALICE) && audited.iter().any(|t| t == GONE),
            "{core} corrected without an audit row: {audited:?}"
        );
    }

    let after = d.both(
        "usage_drift after",
        c.fastcore.usage_drift().await,
        c.pg_core.usage_drift().await,
        &[],
    );
    let (after, _) = after.expect("the report answers on both cores");
    assert_eq!(after.differing, 0, "{after:?}");
    assert!(after.orphan_counters.is_empty());

    let again = d.both(
        "usage_drift again",
        c.fastcore.correct_usage(false).await,
        c.pg_core.correct_usage(false).await,
        &[],
    );
    let (again, _) = again.expect("a second pass answers on both cores");
    assert_eq!((again.written, again.orphans_deleted), (0, 0));
}
