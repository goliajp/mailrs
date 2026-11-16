//! Each core's `SearchStore` under the shared search route, held to the
//! trait's own check: the same mail, arriving through each core's intake,
//! is found by the same queries in the same order.

use super::harness;
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_stores_find_what_the_search_check_asks() {
    let c = harness::start_on("mem://search-store-fastcore", "mem://search-store-pg-core").await;
    let mut d = harness::Diffs::default();
    accounts::seed(&c, &mut d).await;
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
    let spools = Spools::new(&c);
    for letter in mailrs_core_routes::store::search_fixtures(ALICE) {
        spools.arrive(&c, &envelope(None), &letter).await;
    }
    eprintln!("checking fastcore");
    mailrs_core_routes::store::check_search_store(&*c.fastcore_state, ALICE).await;
    eprintln!("checking pg-core");
    mailrs_core_routes::store::check_search_store(&*c.pg_core_state, ALICE).await;
}
