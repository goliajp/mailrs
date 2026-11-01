//! What one message index costs for a mailbox of 30,000 messages in
//! 20,000 conversations: time built in full and answered "not modified",
//! and this process's peak memory for one request and for four at once
//! (four other users). The first request of a user builds; later ones are
//! answered from the kept index until the user's rows change. A measurement, run by hand in release on Linux:
//! `cargo test --release -p mailrs-fastcore --test message_index_cost -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Instant;

const THREADS: u32 = 20_000;
const MESSAGES: u32 = 30_000;
/// One user measured alone, four at once.
const USERS: usize = 5;

fn user(n: usize) -> String {
    format!("u{n}@x.test")
}

fn seed(mailbox: &mailrs_mailbox_kevy::KevyMailboxStore, user: &str) {
    for n in 0..MESSAGES {
        let t = n % THREADS;
        let tid = format!("t-{t:06}-{user}");
        let mid = format!("m-{n:06}-{user}");
        let date = 1_786_000_000 + i64::from(n);
        mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: date,
                latest_preview: "hello",
                senders_csv: "Friend <friend@partner.example>",
                subject: "hello",
                thread_id: &tid,
                unread: n % 3 == 0,
                user,
            })
            .expect("arrival");
        let payload = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": n + 1, "blob_ref": mid,
            "sender": "Friend <friend@partner.example>", "recipients": user,
            "subject": "hello", "date": date, "internal_date": date, "size": 10,
            "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 1,
        });
        mailbox
            .upsert_user_message(
                user,
                &tid,
                &mid,
                date,
                payload.to_string().as_bytes(),
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref: &mid,
                    uid: n + 1,
                    flags: u32::from(n % 3 != 0),
                    modseq: 1,
                },
            )
            .expect("message");
    }
}

/// `VmRSS` and `VmHWM` of this process, in KiB.
fn memory() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let field = |name: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .expect("a field of /proc/self/status")
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// Start the peak over from the current resident size.
fn reset_peak() {
    std::fs::write("/proc/self/clear_refs", "5").expect("reset VmHWM");
}

async fn fetch(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    tag: Option<&str>,
) -> (u16, usize, String) {
    let mut req = http
        .get(format!("{base}/v1/users/{user}/messages/index"))
        .bearer_auth("secret");
    if let Some(t) = tag {
        req = req.header("if-none-match", t);
    }
    let r = req.send().await.expect("request");
    let status = r.status().as_u16();
    let etag = r
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let len = r.bytes().await.expect("body").len();
    (status, len, etag)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement, run by hand in release"]
async fn the_index_of_thirty_thousand_messages() {
    // the feed as production opens it: an unchanged mailbox is answered
    // from the index kept at the last request
    let store = Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default().with_feed(16 * 1024 * 1024))
            .expect("kevy"),
    );
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    let t = Instant::now();
    for n in 0..USERS {
        seed(&mailbox, &user(n));
    }
    println!(
        "seeded {USERS} users x {MESSAGES} messages in {:?}",
        t.elapsed()
    );
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, mailrs_fastcore::build_router(state)).await });
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let mut tag = String::new();
    for round in 0..3 {
        reset_peak();
        let (before, _) = memory();
        let t = Instant::now();
        let (status, len, etag) = fetch(&http, &base, &user(0), None).await;
        let took = t.elapsed();
        let (after, peak) = memory();
        assert_eq!(status, 200);
        println!(
            "user 0, request {round} (0 builds, then kept): {took:?}, {len} bytes; RSS before {before} KiB, after {after} KiB, peak {peak} KiB (+{} KiB)",
            peak - before
        );
        tag = etag;
    }
    for round in 0..3 {
        let t = Instant::now();
        let (status, len, _) = fetch(&http, &base, &user(0), Some(&tag)).await;
        assert_eq!((status, len), (304, 0));
        println!("not modified {round}: {:?}", t.elapsed());
    }

    reset_peak();
    let (before, _) = memory();
    let t = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for n in 1..USERS {
        let (http, base) = (http.clone(), base.clone());
        set.spawn(async move { fetch(&http, &base, &user(n), None).await });
    }
    while let Some(done) = set.join_next().await {
        assert_eq!(done.expect("join").0, 200);
    }
    let took = t.elapsed();
    let (after, peak) = memory();
    println!(
        "4 builds at once, one per user: {took:?}; RSS before {before} KiB, after {after} KiB, peak {peak} KiB (+{} KiB)",
        peak - before
    );
}
