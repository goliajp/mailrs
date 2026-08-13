//! Enumeration must not lose threads at a page boundary inside a tie.
//!
//! `list_conversations` pages on `before_ts`, which means **strictly** less
//! than `last_date`. So a page that ends in the middle of a group of threads
//! sharing one second, and then asks for "older than that second", excludes the
//! rest of the group from every later page. Those threads are never enumerated,
//! so the migration never delivers them — and the only visible trace is a
//! thread count slightly lower than it should be, which nobody can recognise as
//! wrong.
//!
//! `last_date` is whole seconds. Production measured 929 ties over 30,000 rows
//! against a default page size of 200, so a boundary landing inside a tie is
//! ordinary.
//!
//! This drives a real kevy core over HTTP rather than a mock, because the thing
//! under test is the interaction between the cursor's semantics and the
//! enumeration loop — a fake list_conversations would encode whichever
//! semantics the author had in mind.

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use mailrs_core_sync::{SyncOpts, sync};

const USER: &str = "tie@test";
/// One second shared by more threads than a page holds.
const TIED_SECOND: i64 = 1_700_000_000;
const PAGE: u32 = 3;
/// Enough to straddle two page boundaries at the tied second.
const TIED_THREADS: usize = 7;

fn spawn_core() -> String {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(store),
    ));
    let router = mailrs_fastcore::build_router(state);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn deliver(message_id: &str, uid: u32, thread_id: &str, date: i64) -> DeliverMessageRequest {
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid,
        "blob_ref": format!("{message_id}.host"),
        "sender": "remote@x.y", "recipients": USER, "subject": "Hi",
        "date": date, "internal_date": date,
        "size": 42, "flags": 1, "message_id": message_id,
        "in_reply_to": "", "thread_id": thread_id, "modseq": 0,
        "user_address": USER,
    });
    DeliverMessageRequest {
        message_id: message_id.into(),
        subject: "Hi".into(),
        senders_csv: "remote@x.y".into(),
        latest_date: date,
        latest_preview: String::new(),
        category: "inbox".into(),
        unread: true,
        uid,
        payload_wire_json: wire.to_string(),
    }
}

#[tokio::test]
async fn a_tie_spanning_page_boundaries_is_fully_enumerated() {
    let src = Client::new(spawn_core(), String::new());
    let dst = Client::new(spawn_core(), String::new());

    src.add_account(&AddAccountRequest {
        address: USER.into(),
        display_name: "Tie".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");

    // Every thread on the same second, so every page boundary is inside the tie.
    for t in 0..TIED_THREADS {
        let thread = format!("tie-{t}@test");
        src.deliver_message(
            USER,
            &thread,
            &deliver(&format!("tie-{t}@test"), t as u32 + 1, &thread, TIED_SECOND),
        )
        .await
        .expect("seed");
    }

    // Bounded, like the other test. The regression this guards against is an
    // enumeration cursor that stops advancing, and without a bound that fails
    // as a hung suite rather than a red test — which is how it cost ten
    // minutes of a gate run the first time.
    let report = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        sync(
            &src,
            &dst,
            &SyncOpts {
                page_size: PAGE,
                ..Default::default()
            },
        ),
    )
    .await
    .expect("enumeration must terminate")
    .expect("sync");

    assert_eq!(
        report.threads as usize, TIED_THREADS,
        "the walk enumerated {} of {TIED_THREADS} threads. Advancing the cursor \
         to the page's oldest second excludes every thread that shares it, and \
         the dedup set cannot recover them because they were never seen.",
        report.threads
    );

    // And they are actually on the destination, not merely counted.
    for t in 0..TIED_THREADS {
        let thread = format!("tie-{t}@test");
        let msgs = dst
            .list_thread_messages(USER, &thread)
            .await
            .expect("dst list");
        assert_eq!(
            msgs.items.len(),
            1,
            "{thread} did not arrive on the destination"
        );
    }
}

#[tokio::test]
async fn a_tie_larger_than_a_page_widens_the_window_rather_than_skipping() {
    // The case the overlap alone cannot resolve: one second holding more
    // threads than a page, where re-reading with `<=` returns the same rows
    // forever because the query has no offset. The loop widens the window
    // instead of stepping past the second, because stepping past it drops
    // every thread in it that has not been seen — four of seven, in the test
    // above, and reported only as a lower count.
    let src = Client::new(spawn_core(), String::new());
    let dst = Client::new(spawn_core(), String::new());

    src.add_account(&AddAccountRequest {
        address: USER.into(),
        display_name: "Tie".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");

    for t in 0..5 {
        let thread = format!("big-{t}@test");
        src.deliver_message(
            USER,
            &thread,
            &deliver(&format!("big-{t}@test"), t + 1, &thread, TIED_SECOND),
        )
        .await
        .expect("seed");
    }

    // Page of 1 against 5 threads on one second: the overlap can never yield a
    // new thread at that width, so this only completes if the window widens.
    let report = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        sync(
            &src,
            &dst,
            &SyncOpts {
                page_size: 1,
                ..Default::default()
            },
        ),
    )
    .await
    .expect("sync must terminate rather than spin on a tie it cannot page")
    .expect("sync");

    assert_eq!(
        report.threads, 5,
        "widening must reach every thread in the tied second, not just the \
         first page's"
    );
}
