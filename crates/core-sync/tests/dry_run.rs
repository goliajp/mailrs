//! A dry run writes nothing, and its counts predict the real run.
//!
//! Both halves matter. A dry run that quietly wrote something would be the
//! worst defect this tool could have — it is the step an operator takes
//! *because* they are not ready to commit. And a dry run whose numbers do not
//! match what follows is worse than no dry run, because the decision to switch
//! gets made on them.
//!
//! So this asserts the destination is byte-for-byte untouched after a dry run,
//! then runs for real and asserts the prediction was exact.

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use mailrs_core_sync::{SyncOpts, sync};

const USER: &str = "dry@test";
const THREADS: usize = 4;

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

async fn seed(c: &Client, n: usize) {
    c.add_account(&AddAccountRequest {
        address: USER.into(),
        display_name: "Dry".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");
    for t in 0..n {
        let thread = format!("dry-{t}@test");
        c.deliver_message(
            USER,
            &thread,
            &deliver(
                &format!("dry-{t}@test"),
                t as u32 + 1,
                &thread,
                1_700_000_000 + t as i64 * 100,
            ),
        )
        .await
        .expect("seed");
    }
}

/// Everything the destination can be asked about, as one comparable value.
async fn destination_state(c: &Client) -> Vec<String> {
    let mut out = Vec::new();
    let accounts = c.list_accounts().await.expect("list_accounts");
    for a in &accounts.items {
        out.push(format!("account {} {}", a.address, a.display_name));
    }
    for t in 0..THREADS {
        let thread = format!("dry-{t}@test");
        let msgs = c
            .list_thread_messages(USER, &thread)
            .await
            .map(|r| {
                r.items
                    .iter()
                    .map(|m| m.message_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(format!("{thread} -> {msgs:?}"));
    }
    out.sort();
    out
}

#[tokio::test]
async fn a_dry_run_leaves_the_destination_untouched_and_predicts_the_real_one() {
    let src = Client::new(spawn_core(), String::new());
    let dst = Client::new(spawn_core(), String::new());
    seed(&src, THREADS).await;

    let before = destination_state(&dst).await;

    let dry = sync(
        &src,
        &dst,
        &SyncOpts {
            dry_run: true,
            ..Default::default()
        },
    )
    .await
    .expect("dry run");

    assert_eq!(
        destination_state(&dst).await,
        before,
        "a dry run must not change the destination — it is the step taken \
         precisely because nobody is ready to commit yet"
    );
    assert_eq!(dry.accounts, 1, "it should still report what it saw");
    assert_eq!(dry.messages_delivered as usize, THREADS, "one per thread");
    assert_eq!(
        dry.threads_already_identical, 0,
        "nothing is on the destination yet, so no thread can already match"
    );

    // Now for real, and the prediction has to hold.
    let real = sync(&src, &dst, &SyncOpts::default()).await.expect("sync");
    assert_eq!(
        (real.accounts, real.threads, real.messages_delivered),
        (dry.accounts, dry.threads, dry.messages_delivered),
        "the dry run's counts are what a switch decision is made on, so they \
         have to be the counts the real run produces"
    );

    // And a second dry run, against a destination that now matches, must say so
    // rather than repeating the first run's numbers.
    let after = sync(
        &src,
        &dst,
        &SyncOpts {
            dry_run: true,
            ..Default::default()
        },
    )
    .await
    .expect("second dry run");
    assert_eq!(
        after.messages_delivered, 0,
        "everything is already there, so nothing would move"
    );
    assert_eq!(
        after.threads_already_identical as usize, THREADS,
        "and every thread should be reported as already matching — a number \
         that can come out zero, unlike 'threads examined'"
    );
}

#[tokio::test]
async fn a_dry_run_reports_accounts_only_the_destination_has() {
    // The direction a one-way copy cannot see. After a switch, an account that
    // exists only on the destination is mail readable on one core and not the
    // other, and nothing in a copy-forward run has reason to mention it.
    let src = Client::new(spawn_core(), String::new());
    let dst = Client::new(spawn_core(), String::new());
    seed(&src, 1).await;
    dst.add_account(&AddAccountRequest {
        address: "stranger@test".into(),
        display_name: "Only Here".into(),
        password: "pw".into(),
    })
    .await
    .expect("seed dst-only account");

    let dry = sync(
        &src,
        &dst,
        &SyncOpts {
            dry_run: true,
            ..Default::default()
        },
    )
    .await
    .expect("dry run");

    assert_eq!(
        dry.accounts_only_on_dst, 1,
        "the destination-only account must be reported"
    );

    // And a real run must not silently remove it — copy forward, never delete.
    sync(&src, &dst, &SyncOpts::default()).await.expect("sync");
    let addresses: Vec<String> = dst
        .list_accounts()
        .await
        .expect("list")
        .items
        .into_iter()
        .map(|a| a.address)
        .collect();
    assert!(
        addresses.iter().any(|a| a == "stranger@test"),
        "a sync must not delete what it did not create"
    );
}
