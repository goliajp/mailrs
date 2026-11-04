//! Rounds of re-judging: after ordinary mail neither core has taken anything
//! back, and putting back a round that does not exist is 404 on both.

use super::harness::{Cores, Diffs};

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    if let Some((a, _)) = d.both(
        "rejudge_batches",
        fc.rejudge_batches().await,
        pg.rejudge_batches().await,
        &[],
    ) {
        assert!(a.items.is_empty(), "{:?}", a.items);
    }
    let (a, b) = (
        fc.release_rejudge_batch("r999", "admin@example.test").await,
        pg.release_rejudge_batch("r999", "admin@example.test").await,
    );
    let status = |r: &Result<_, mailrs_core_api::error::CoreApiError>| match r {
        Ok(_) => 200,
        Err(e) => e.status_code(),
    };
    assert_eq!((status(&a), status(&b)), (404, 404), "{a:?} {b:?}");

    // a conversation nobody released has no release facts on either core
    // (the facts a release writes are compared in `intake.rs`)
    if let Some((a, _)) = d.both(
        "thread_releases",
        fc.thread_releases(super::ALICE, "never-held@outside.example")
            .await,
        pg.thread_releases(super::ALICE, "never-held@outside.example")
            .await,
        &[],
    ) {
        assert!(a.items.is_empty(), "{:?}", a.items);
    }
}
