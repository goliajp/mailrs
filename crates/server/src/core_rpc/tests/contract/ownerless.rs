//! Conversations nobody holds: neither core has any after ordinary mail,
//! neither deletes one somebody holds (409), and a missing one is 404.

use super::harness::{Cores, Diffs};

fn status<T>(r: &Result<T, mailrs_core_api::error::CoreApiError>) -> u16 {
    match r {
        Ok(_) => 200,
        Err(e) => e.status_code(),
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    if let Some((a, _)) = d.both(
        "ownerless_threads",
        fc.ownerless_threads().await,
        pg.ownerless_threads().await,
        // each store counts its own kind of conversation record
        &["scanned"],
    ) {
        assert!(a.items.is_empty(), "{:?}", a.items);
        assert!(a.scanned > 0, "the listing looked at nothing: {a:?}");
    }
    for (tid, want) in [("t-hello@partner.test", 409), ("no-such@nowhere.test", 404)] {
        let (a, b) = (
            fc.delete_ownerless_thread(tid).await,
            pg.delete_ownerless_thread(tid).await,
        );
        assert_eq!((status(&a), status(&b)), (want, want), "{tid}: {a:?} {b:?}");
    }
}
