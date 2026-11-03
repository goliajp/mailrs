//! Quota limits, alike on both cores: every account a core creates has the
//! default limit where the receiver reads it, a limit an operator sets
//! lands there too, and the pass that fills in missing limits reports and
//! writes the same thing on both.

use mailrs_core_api::method::admin::{DEFAULT_QUOTA_BYTES, QuotaLimitAction};
use mailrs_core_sidestate::families::quota_limits::limit;
use mailrs_core_sidestate::keys::quota::limit_key;

use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};
use super::{ALICE, BOB};

const STORES: [(&str, &str); 2] = [("fastcore", FASTCORE_KEVY), ("pg-core", PG_CORE_KEVY)];

fn limit_in(url: &str, user: &str) -> Option<i64> {
    let mut c = kevy_client::Connection::connect(url).expect("side state");
    limit(&mut c, user).expect("read limit")
}

/// Runs after `accounts::run`, which set Alice's quota to 1 GiB.
pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    for (core, url) in STORES {
        assert_eq!(
            limit_in(url, BOB),
            Some(DEFAULT_QUOTA_BYTES),
            "{core} created an account without the default limit"
        );
        assert_eq!(
            limit_in(url, ALICE),
            Some(1 << 30),
            "{core} set a quota the receiver never sees"
        );
    }

    // an account from before the default: no limit at all
    for (_, url) in STORES {
        let mut conn = kevy_client::Connection::connect(url).expect("side state");
        conn.del(&[limit_key(BOB).as_bytes()]).expect("drop limit");
    }
    let dry = d.both(
        "quota_limits dry run",
        c.fastcore.quota_limits(true).await,
        c.pg_core.quota_limits(true).await,
        &[],
    );
    let (dry, _) = dry.expect("a dry run answers on both cores");
    let row = |r: &mailrs_core_api::method::admin::QuotaLimitsResponse, who: &str| {
        r.rows.iter().find(|x| x.address == who).map(|x| x.action)
    };
    assert_eq!(row(&dry, BOB), Some(QuotaLimitAction::WouldWrite));
    assert_eq!(row(&dry, ALICE), Some(QuotaLimitAction::HasLimit));
    for (core, url) in STORES {
        assert_eq!(limit_in(url, BOB), None, "{core} wrote on a dry run");
    }

    let done = d.both(
        "quota_limits write",
        c.fastcore.quota_limits(false).await,
        c.pg_core.quota_limits(false).await,
        &[],
    );
    let (done, _) = done.expect("the pass answers on both cores");
    assert_eq!(row(&done, BOB), Some(QuotaLimitAction::Written));
    for (core, url) in STORES {
        assert_eq!(
            limit_in(url, BOB),
            Some(DEFAULT_QUOTA_BYTES),
            "{core} did not write the default"
        );
        assert_eq!(
            limit_in(url, ALICE),
            Some(1 << 30),
            "{core} overwrote a set limit"
        );
    }
}
