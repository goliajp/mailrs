//! The list of addresses each user sent mail to, rebuilt alike on both
//! cores: a dry run writes nothing, the pass adds only addresses with a
//! `sent_count` above zero, and a second pass finds nothing missing.

use mailrs_core_sidestate::families::{contact_sent, contacts};

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

const STORES: [(&str, &str); 2] = [("fastcore", FASTCORE_KEVY), ("pg-core", PG_CORE_KEVY)];
const WROTE_TO: &str = "partner@sent-to.example";
const HEARD_FROM: &str = "stranger@sent-to.example";

fn store(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("side state")
}

fn listed(url: &str) -> Vec<String> {
    contact_sent::sent_to(&mut store(url), ALICE).expect("read the list")
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    for (_, url) in STORES {
        let mut s = store(url);
        // counters as they stood before the list existed
        for (addr, field) in [(WROTE_TO, "sent_count"), (HEARD_FROM, "received_count")] {
            s.hset(
                contacts::contact_key(ALICE, addr).as_bytes(),
                &[(field.as_bytes(), b"2" as &[u8])],
            )
            .expect("seed a counter");
        }
    }

    let dry = d.both(
        "backfill_sent_to dry",
        c.fastcore.backfill_sent_to(true).await,
        c.pg_core.backfill_sent_to(true).await,
        &[],
    );
    let (dry, _) = dry.expect("the dry run answers on both cores");
    assert!(dry.dry_run && dry.missing >= 1, "{dry:?}");
    for (core, url) in STORES {
        assert!(
            !listed(url).iter().any(|a| a == WROTE_TO),
            "{core} wrote on a dry run"
        );
    }

    let done = d.both(
        "backfill_sent_to",
        c.fastcore.backfill_sent_to(false).await,
        c.pg_core.backfill_sent_to(false).await,
        &[],
    );
    let (done, _) = done.expect("the pass answers on both cores");
    assert_eq!(done.missing, dry.missing, "{done:?}");
    for (core, url) in STORES {
        let list = listed(url);
        assert!(list.iter().any(|a| a == WROTE_TO), "{core}: {list:?}");
        assert!(!list.iter().any(|a| a == HEARD_FROM), "{core}: {list:?}");
    }

    let again = d.both(
        "backfill_sent_to again",
        c.fastcore.backfill_sent_to(false).await,
        c.pg_core.backfill_sent_to(false).await,
        &[],
    );
    let (again, _) = again.expect("a second pass answers on both cores");
    assert_eq!(again.missing, 0, "{again:?}");
}
