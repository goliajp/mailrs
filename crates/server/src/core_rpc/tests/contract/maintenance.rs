//! Read-only operator checks both cores answer from the network kevy and
//! their store: the audit chain, and what the forwarding rules make of the
//! stored Sieve scripts.

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

/// Fields that say when a check ran.
const WHEN: &[&str] = &["verified_at", "checked_at", "at"];

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    // Alice forwards out of our domains, to an address she has not verified
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        let mut conn = kevy_client::Connection::connect(url).expect("kevy");
        conn.set(
            mailrs_core_sidestate::sieve_key(ALICE).as_bytes(),
            b"redirect \"someone@outside.example\";",
        )
        .expect("sieve script");
    }
    if let Some((a, _)) = d.both(
        "sieve_survey",
        c.fastcore.sieve_survey().await,
        c.pg_core.sieve_survey().await,
        &[],
    ) {
        assert!(
            a["accounts"].as_u64().unwrap_or(0) > 0,
            "surveyed nothing: {a}"
        );
    }
    d.both(
        "audit_verify",
        c.fastcore.audit_verify().await,
        c.pg_core.audit_verify().await,
        WHEN,
    );
}
