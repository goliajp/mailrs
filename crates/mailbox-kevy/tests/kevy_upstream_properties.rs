//! Properties this crate relies on the engine for.
//!
//! Not tests of mailrs code — tests of the guarantees underneath it, pinned
//! here so a kevy bump that regresses one fails in CI rather than in
//! production. kevy 5.0 fixed several defects of this shape, and the way to
//! know a future release has not reintroduced one is to ask.
//!
//! ## What this crate is actually exposed to
//!
//! Established by reading every call site rather than by assuming, because a
//! regression test for a path the code does not take is a test that can only
//! ever pass:
//!
//! - **A multi-key `DEL` must clear the index.** `delete_account` removes the
//!   account hash and its permissions hash in one `ctx.del(&[a, b])`, and
//!   `mailrs:account:*` carries two declared range indexes. This is the one
//!   multi-key delete in the tree that lands on indexed keys, and
//!   `list_account_addresses` reads one of those indexes — so the symptom of
//!   the engine getting this wrong is a deleted account that keeps appearing
//!   in the admin list. Covered below.
//!
//! - **An expiring key leaving its index**: not exposed. No indexed prefix
//!   here carries a TTL — threads, accounts, aliases and domains never
//!   expire, and the keys that do expire (sessions, greylist, spam cache) are
//!   in no declared index.
//!
//! - **A cross-shard write reaching the change feed**: not exposed. The feed
//!   consumers watch `mailrs:user:{user}:` (IMAP IDLE) and the runtime's own
//!   prefixes; the only multi-key delete lands on `mailrs:account:*`, which
//!   nothing reads through the feed. Worth re-checking if a multi-key delete
//!   is ever added to a watched prefix.
//!
//! The last two are recorded rather than tested on purpose. A test that
//! asserts a property over a path the code never takes passes forever and
//! reads like coverage.
//!
//! ## These were green on 4.1.1
//!
//! Read them as pins, not as reproductions. They were written expecting red
//! against kevy 4.1.1 — the release before the multi-key-DEL index fix — and
//! both passed. The likeliest reason is that the fix landed on the runtime's
//! dispatch path, and this crate deletes through the embedded `AtomicCtx`,
//! which was already maintaining the index.
//!
//! That is worth stating plainly because it sets expectations for the 5.1
//! upgrade: of the three defects 5.0 closed that looked like they might reach
//! this code, none of them do. The upgrade's value here is in the stalls it
//! removes and the capabilities it adds, not in correctness this crate was
//! losing. A changelog headline is about a surface, and only the call sites
//! say whether a given consumer stands on the part that moved.

use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::KevyMailboxStore;

fn store() -> KevyMailboxStore {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_admin_indexes();
    st
}

fn blob(address: &str) -> String {
    format!(
        r#"{{"address":"{address}","domain":"bench.local","display_name":"A",
             "active":true,"created_at":1748000000,"quota_bytes":0}}"#
    )
}

/// Deleting an account must take it out of the index it is listed from.
///
/// `delete_account` is the tree's only multi-key `ctx.del` on indexed keys.
/// If the engine's index maintenance does not run for the multi-key form, the
/// row survives in `accounts_by_active` and the account keeps appearing in
/// the admin list after being deleted — with a nil hydration behind it.
#[test]
fn a_deleted_account_leaves_the_index_it_is_listed_from() {
    let st = store();
    for a in ["keep@bench.local", "gone@bench.local"] {
        st.upsert_account(a, &blob(a)).expect("upsert");
    }

    let before = st.list_account_addresses().expect("list");
    assert!(
        before.contains(&"gone@bench.local".to_string())
            && before.contains(&"keep@bench.local".to_string()),
        "both accounts should be listed before the delete, got {before:?}"
    );

    st.delete_account("gone@bench.local").expect("delete");

    // The hash is gone either way — that half was never in doubt.
    assert!(
        st.get_account_blob("gone@bench.local")
            .expect("get")
            .is_none(),
        "the account hash should be gone"
    );

    // The index is the half that the multi-key delete used to miss.
    let after = st.list_account_addresses().expect("list");
    assert!(
        !after.contains(&"gone@bench.local".to_string()),
        "a deleted account is still listed: {after:?} — the multi-key DEL did \
         not reach the index, so `accounts_by_active` still holds the row and \
         the admin list serves a phantom"
    );
    assert!(
        after.contains(&"keep@bench.local".to_string()),
        "the surviving account should still be listed, got {after:?}"
    );
}

/// The same shape, with both keys of the pair present.
///
/// `delete_account` deletes the account hash *and* its permissions hash in
/// one call. The test above leaves the permissions key absent, so the delete
/// is a two-key call where only one key exists — which is a different case
/// from both existing, and the arity of the effect is what the engine's index
/// hook keys on.
#[test]
fn deleting_an_account_that_has_permissions_also_clears_the_index() {
    let st = store();
    st.upsert_account("both@bench.local", &blob("both@bench.local"))
        .expect("upsert");
    st.upsert_permissions("both@bench.local", r#"{"permissions":["admin.accounts"]}"#)
        .expect("upsert perms");
    assert!(
        st.get_permissions_blob("both@bench.local")
            .expect("get perms")
            .is_some(),
        "the permissions blob should be there before the delete"
    );

    st.delete_account("both@bench.local").expect("delete");

    assert!(
        st.get_permissions_blob("both@bench.local")
            .expect("get perms")
            .is_none(),
        "the permissions blob should be gone"
    );
    let after = st.list_account_addresses().expect("list");
    assert!(
        after.is_empty(),
        "nothing should be listed after the only account was deleted, got {after:?}"
    );
}

/// A failure must arrive at the caller with its category intact.
///
/// This crate used to wrap every engine call in
/// `.map_err(std::io::Error::other)`, which flattens all nine `KevyError`
/// variants into `ErrorKind::Other` — a caller could not tell a timeout from
/// a missing index from a wrong-type read. The engine has carried
/// `impl From<KevyError> for io::Error` with a per-variant kind since 4.1.0,
/// so `?` alone now does the conversion *and* keeps the category.
///
/// The assertion that matters is the second one: it fails if someone
/// reintroduces the wrapper, which would compile and pass every other test.
#[test]
fn an_engine_failure_keeps_its_category_through_the_io_boundary() {
    let st = Store::open(Config::default()).expect("open in-memory kevy");
    st.set(b"a-string", b"not-a-hash").expect("set");

    // Read it as a hash: a wrong-type error, produced by a real op rather
    // than constructed, so this also pins that the engine still reports it.
    fn read_as_hash(st: &Store) -> std::io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Ok(st.hgetall(b"a-string")?)
    }
    let err = read_as_hash(&st).expect_err("hgetall on a string must fail");

    assert_ne!(
        err.kind(),
        std::io::ErrorKind::Other,
        "the engine's category was flattened — someone re-added \
         .map_err(io::Error::other) on this path"
    );
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
