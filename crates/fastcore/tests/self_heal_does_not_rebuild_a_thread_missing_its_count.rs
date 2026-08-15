//! Self-heal asks whether a thread exists, not what it counts.
//!
//! `heal_membership_rows` decides whether to rebuild a thread from every
//! message in its maildir, and it used to make that decision by probing
//! the `count` field. That put a counter in charge of an existence
//! question, in the one place where getting it wrong is unbounded: the
//! sweep runs every 30 seconds, so a thread whose counter was absent
//! would be re-created — `record_message_arrival` per message, six
//! `hincrby`s apiece — on every tick, forever.
//!
//! This is the gate that has to hold before the counters can be retired
//! at all, and it is a live hazard on its own: nothing guarantees a
//! thread hash carries `count`.

use std::sync::Arc;

use mailrs_mailbox_kevy::{KevyMailboxStore, MessageArrival, keys};

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s
}

/// The probe answers "yes, it exists" for a thread with no counter.
///
/// Asserted against the constant the production path uses, so the two
/// cannot drift apart: a change to `THREAD_EXISTS_FIELD` that picks a
/// field arrivals do not write would fail here rather than on a live
/// mailbox at one sweep per thirty seconds.
#[test]
fn a_thread_that_lost_its_count_still_reads_as_existing() {
    let s = store();
    let u = "u@x.com";
    s.record_message_arrival(&MessageArrival {
        thread_id: "t1",
        user: u,
        subject: "Subj",
        senders_csv: "other@z.com",
        latest_date: 100,
        latest_preview: "",
        category: "inbox",
        unread: true,
        is_own: false,
    })
    .unwrap();

    let key = keys::thread("t1");
    assert!(
        s.store_ref()
            .hexists(key.as_bytes(), keys::THREAD_EXISTS_FIELD)
            .unwrap(),
        "an arrival has to write the field the existence probe asks for"
    );

    s.store_ref()
        .hdel(
            key.as_bytes(),
            &[b"count".as_slice(), b"unread_count".as_slice()],
        )
        .unwrap();

    assert!(
        s.store_ref()
            .hexists(key.as_bytes(), keys::THREAD_EXISTS_FIELD)
            .unwrap(),
        "the thread stopped existing because a counter did"
    );
    assert!(
        !s.store_ref().hexists(key.as_bytes(), b"count").unwrap(),
        "the fixture has to actually remove the old probe's field"
    );
}
