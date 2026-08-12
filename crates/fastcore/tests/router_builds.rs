//! The router can be constructed.
//!
//! Axum panics at construction on a duplicate or malformed path, not at
//! compile time — so a route table can be green across the whole test
//! suite and still take the process down on the next boot. 4,483
//! workspace tests were passing while nothing built this router
//! (`rules/api-update-checklist.md`), and splitting the route table
//! across files is exactly the change that would exercise that gap.

use std::sync::Arc;

#[test]
fn the_route_table_can_be_built() {
    // In-memory: the route table does not depend on stored data, and a
    // temp dir would only add a way for this to fail for another reason.
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    // The panic this guards against happens inside `build_router`.
    let _router = mailrs_fastcore::build_router(state);
}
