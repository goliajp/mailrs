//! Switch there, switch back, and compare both times.
//!
//! Rehearse the rollback before needing it. The criterion: the same differences appearing on both passes is
//! what shows the write path did not pollute anything on the way through, while
//! differences that appear only on the second pass are new damage.
//!
//! This is that rehearsal, at the level a test can reach: seed one core, sync to
//! the other, compare field by field, sync back to a third, compare again.
//!
//! `crates/core-sync/tests/roundtrip.rs` already round-trips, and its own header
//! claims a kevy↔kevy pass "exercises every line the kevy↔pg switch does except
//! the PG store internals". That claim did not survive the day: the three
//! defects the cross-lane comparison found — a dropped category, an inverted
//! unread count, a conversation list with no tie-break — were all in exactly
//! that exception, and a same-backend round trip cannot see any of them. It also
//! compares an order-insensitive set of strings, so it could not have seen the
//! ordering ones either.
//!
//! The reverse direction is the point of doing it here. Forward exercises kevy's
//! enumeration and the SQL core's ingest; backward exercises the SQL core's
//! enumeration and kevy's ingest, and nothing else in the suite does that
//! second pair.

#![cfg(feature = "spg")]

use mailrs_core_api::client::Client;
use mailrs_core_sync::{SyncOpts, sync};

use super::pg_core::{spawn_fastcore, spawn_pg_core};
use super::two_lane::{USER, diffs, page, seed};

#[tokio::test]
async fn a_switch_and_a_rollback_leave_the_same_differences() {
    let origin = Client::new(spawn_fastcore(), String::new());
    let sql = Client::new(spawn_pg_core().await, String::new());
    // A third, empty kevy core: rolling back means syncing to the core the
    // switch came from, and reusing `origin` would let its existing rows mask a
    // reverse pass that transferred nothing.
    let back = Client::new(spawn_fastcore(), String::new());

    seed(&origin).await;

    // ── forward: the switch ──────────────────────────────────────────
    let fwd = sync(&origin, &sql, &SyncOpts::default())
        .await
        .expect("forward sync");
    assert!(fwd.messages_delivered > 0, "the switch must move something");

    let after_switch = diffs(
        &page(&origin, 100, None).await,
        &page(&sql, 100, None).await,
        "after the switch",
    );

    // ── backward: the rollback ───────────────────────────────────────
    let rev = sync(&sql, &back, &SyncOpts::default())
        .await
        .expect("reverse sync");
    assert_eq!(
        rev.messages_delivered, fwd.messages_delivered,
        "the rollback must carry back what the switch carried over — a smaller \
         number here is mail that reached the SQL core and could not leave it, \
         which is the failure a rehearsal exists to find while it is still a \
         rehearsal"
    );

    let after_rollback = diffs(
        &page(&origin, 100, None).await,
        &page(&back, 100, None).await,
        "after the rollback",
    );

    // Same both times, or the write path polluted something in transit.
    assert_eq!(
        after_switch.len(),
        after_rollback.len(),
        "the rehearsal's criterion: identical differences on both passes.\n\
         after the switch ({}):\n   {}\n\
         after the rollback ({}):\n   {}",
        after_switch.len(),
        after_switch.join("\n   "),
        after_rollback.len(),
        after_rollback.join("\n   "),
    );
    assert!(
        after_rollback.is_empty(),
        "a kevy → SQL → kevy round trip should come back identical, since both \
         ends are the same store: {}",
        after_rollback.join("; ")
    );

    // And re-running the rollback is a no-op, as the runbook promises.
    let again = sync(&sql, &back, &SyncOpts::default())
        .await
        .expect("re-run the rollback");
    assert_eq!(
        again.messages_delivered, 0,
        "re-running must deliver nothing new"
    );
    assert_eq!(
        again.messages_skipped_dupe, fwd.messages_delivered,
        "…and must recognise every message as already present"
    );
}

#[tokio::test]
async fn a_switch_carries_read_state() {
    // Read state travels per message: marking a thread read on the kevy core
    // sets `\Seen` on each of its messages, and the SQL core counts unread
    // from those flags, so a read thread arrives read.
    let origin = Client::new(spawn_fastcore(), String::new());
    let sql = Client::new(spawn_pg_core().await, String::new());

    seed(&origin).await;
    origin
        .mark_thread_read(USER, "tl-0@test")
        .await
        .expect("read");

    let msgs = origin
        .list_thread_messages(USER, "tl-0@test")
        .await
        .expect("source messages");
    assert!(
        msgs.items.iter().all(|m| m.flags & 1 == 1),
        "every message of a read thread carries \\Seen on the source"
    );

    sync(&origin, &sql, &SyncOpts::default())
        .await
        .expect("forward");

    let after = page(&sql, 100, None).await;
    let dst = after
        .iter()
        .find(|r| r.thread_id == "tl-0@test")
        .expect("thread on the destination");
    assert_eq!(dst.unread_count, 0, "a read thread arrives read");
    let unread = after
        .iter()
        .find(|r| r.thread_id != "tl-0@test" && r.unread_count > 0);
    assert!(unread.is_some(), "and an unread one arrives unread");
}
