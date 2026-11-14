//! An administrator dismisses a report: the reporter's copy stays held, no
//! other reader's is taken, and the waiting list empties, alike on both
//! cores.

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::reports::{MINTED, held_threads, scam};
use super::{ALICE, BOB, accounts};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dismissed_report_takes_nothing_back_on_either_core() {
    let c = harness::start_on(
        "mem://reports-dismiss-fastcore",
        "mem://reports-dismiss-pg-core",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    let spools = Spools::new(&c);
    for (user, name, mid) in [
        (ALICE, "Alice", "d1@parcel-desk.example"),
        (BOB, "Bob", "d2@parcel-desk.example"),
    ] {
        let mut env = envelope(None);
        env.forward_paths = vec![user.into()];
        spools.arrive(&c, &env, &scam(name, user, mid)).await;
    }
    let reported = d.both(
        "report_message before a dismissal",
        c.fastcore
            .report_message(ALICE, "d1@parcel-desk.example", false)
            .await,
        c.pg_core
            .report_message(ALICE, "d1@parcel-desk.example", false)
            .await,
        MINTED,
    );
    let Some((fc, pg)) = reported else {
        panic!("{}", d.0.join("\n\n"));
    };
    d.both(
        "dismiss_report",
        c.fastcore
            .dismiss_report(&fc.report_id, "admin@example.test")
            .await,
        c.pg_core
            .dismiss_report(&pg.report_id, "admin@example.test")
            .await,
        MINTED,
    );
    d.both(
        "pending_reports after dismissing",
        c.fastcore.pending_reports().await,
        c.pg_core.pending_reports().await,
        MINTED,
    );
    assert!(
        held_threads(&c, &mut d, BOB, "Bob's held list after a dismissal")
            .await
            .is_empty(),
        "a dismissed report took Bob's copy"
    );
    d.0.extend(super::coverage::require("reports-dismiss", &c));
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
}
