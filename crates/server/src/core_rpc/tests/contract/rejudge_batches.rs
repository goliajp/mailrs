//! A round that would hold more than one observation may waits for a
//! decision on both cores alike: discarded, it holds nothing; confirmed, it
//! holds its conversations; and the held round is put back whole.

use mailrs_core_api::method::admin::RejudgeBatchWire;

use super::harness::{Cores, Diffs};

const ROOT: &str = "admin@example.test";

/// Fields that are times or ids each core picks for itself.
const OWN: &[&str] = &[
    "observation",
    "at",
    "released_at",
    "discarded_at",
    "confirmed_at",
];

/// The batches on both cores, alike apart from [`OWN`].
async fn listed(c: &Cores, d: &mut Diffs) -> (Vec<RejudgeBatchWire>, Vec<RejudgeBatchWire>) {
    d.both(
        "rejudge_batches",
        c.fastcore
            .rejudge_batches()
            .await
            .map(|r| by_state(r.items)),
        c.pg_core.rejudge_batches().await.map(|r| by_state(r.items)),
        OWN,
    )
    .expect("the batch list on both cores")
}

/// Ordered by what happened to each batch, not by time: two rounds in the
/// same second on one core and a second apart on the other list in
/// different orders.
fn by_state(mut items: Vec<RejudgeBatchWire>) -> Vec<RejudgeBatchWire> {
    items.sort_by_key(|b| {
        (
            b.pending,
            b.discarded_at > 0,
            b.confirmed_at > 0,
            b.released_at > 0,
            b.would_hold,
        )
    });
    items
}

/// The one waiting batch on each core.
fn waiting(items: &[RejudgeBatchWire]) -> String {
    let w: Vec<&RejudgeBatchWire> = items.iter().filter(|b| b.pending).collect();
    assert_eq!(w.len(), 1, "one waiting batch: {items:?}");
    w[0].observation.clone()
}

/// The first round waits; discard it and nothing is held.
pub(super) async fn discard_the_waiting_round(c: &Cores, d: &mut Diffs) {
    let (a, b) = listed(c, d).await;
    let (oa, ob) = (waiting(&a), waiting(&b));
    assert!(a[0].would_hold > 1, "over the cap of one: {a:?}");
    let r = d.both(
        "discard_rejudge_batch",
        c.fastcore.discard_rejudge_batch(&oa, ROOT).await,
        c.pg_core.discard_rejudge_batch(&ob, ROOT).await,
        &[],
    );
    assert_eq!(r.map(|(x, _)| x.would_hold), Some(a[0].would_hold));
    let (a, b) = listed(c, d).await;
    for items in [&a, &b] {
        assert!(
            items
                .iter()
                .all(|i| !i.pending && i.discarded_at > 0 && i.conversations == 0),
            "{items:?}"
        );
    }
}

/// The second round waits too; confirm it and its conversations are held,
/// then put the round back.
pub(super) async fn confirm_then_release_the_next(c: &Cores, d: &mut Diffs) {
    let (a, b) = listed(c, d).await;
    let (oa, ob) = (waiting(&a), waiting(&b));
    let held = d
        .both(
            "confirm_rejudge_batch",
            c.fastcore.confirm_rejudge_batch(&oa, ROOT).await,
            c.pg_core.confirm_rejudge_batch(&ob, ROOT).await,
            &[],
        )
        .map(|(x, _)| x.held);
    assert_eq!(held, Some(2), "both conversations held");
    let (a, b) = listed(c, d).await;
    for (items, o) in [(&a, &oa), (&b, &ob)] {
        let one = items
            .iter()
            .find(|i| &i.observation == o)
            .expect("the confirmed batch");
        assert!(
            !one.pending && one.confirmed_at > 0 && one.conversations == 2,
            "{one:?}"
        );
    }
    let released = d
        .both(
            "release_rejudge_batch",
            c.fastcore.release_rejudge_batch(&oa, ROOT).await,
            c.pg_core.release_rejudge_batch(&ob, ROOT).await,
            &[],
        )
        .map(|(x, _)| x.released);
    assert_eq!(released, Some(2), "both conversations put back");
    let (a, b) = listed(c, d).await;
    for (items, o) in [(&a, &oa), (&b, &ob)] {
        let one = items
            .iter()
            .find(|i| &i.observation == o)
            .expect("the released batch");
        assert!(one.released_at > 0, "{one:?}");
    }
}
