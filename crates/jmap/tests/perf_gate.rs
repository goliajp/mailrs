//! Performance regression gates.
//!
//! Each test runs a hot path a fixed number of iterations and asserts the
//! median elapsed time is under a documented budget. Budgets are set with
//! ample CI headroom — the goal is to catch order-of-magnitude regressions
//! (10× slower than expected), not measure performance precisely.
//!
//! Run `cargo bench -p mailrs-jmap` for the full timing detail; perf_gate
//! is the cheap version that fails CI on regression.
//!
//! See [BUDGETS.md](../BUDGETS.md) for budget derivation and re-measurement
//! protocol.

use std::time::{Duration, Instant};

use mailrs_jmap::dispatch::{dispatch_method, dispatch_request};
use mailrs_jmap::fixtures::{EXAMPLE_USER, InMemoryStore, make_message, make_request};
use serde_json::json;

const ITERS: usize = 100;

/// Run `op` `ITERS` times and return the median elapsed.
fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

async fn time_median_async<F, Fut>(mut op: F) -> Duration
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op().await;
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

#[tokio::test]
async fn dispatch_mailbox_get_under_budget() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_mailbox(2, "Sent");

    let args = json!({});
    let median = time_median_async(|| async {
        let _ = dispatch_method("Mailbox/get", &args, EXAMPLE_USER, &store).await;
    })
    .await;

    // Budget: 1 ms. Observed P95 on dev: ~30 µs (criterion). 30× headroom.
    let budget = Duration::from_millis(1);
    assert!(
        median < budget,
        "dispatch_mailbox_get median {median:?} exceeded budget {budget:?}"
    );
}

#[tokio::test]
async fn dispatch_email_query_under_budget() {
    let mut store = InMemoryStore::new().with_mailbox(1, "INBOX");
    for i in 1..=10 {
        store = store.with_message(make_message(i, 1, EXAMPLE_USER));
    }

    let args = json!({"limit": 50});
    let median = time_median_async(|| async {
        let _ = dispatch_method("Email/query", &args, EXAMPLE_USER, &store).await;
    })
    .await;

    // Budget: 2 ms. Observed P95 on dev: ~120 µs (criterion). ~16× headroom.
    let budget = Duration::from_millis(2);
    assert!(
        median < budget,
        "dispatch_email_query median {median:?} exceeded budget {budget:?}"
    );
}

#[tokio::test]
async fn dispatch_request_multi_call_back_ref_under_budget() {
    let mut store = InMemoryStore::new().with_mailbox(1, "INBOX");
    for i in 1..=10 {
        store = store.with_message(make_message(i, 1, EXAMPLE_USER));
    }

    let request = make_request(&[
        ("Email/query", json!({"limit": 5}), "c1"),
        (
            "Email/get",
            json!({
                "#ids": {"resultOf": "c1", "name": "Email/query", "path": "/ids"},
                "properties": ["subject", "from"]
            }),
            "c2",
        ),
    ]);

    let median = time_median_async(|| async {
        let _ = dispatch_request(request.clone(), EXAMPLE_USER, &store).await;
    })
    .await;

    // Budget: 5 ms. Observed P95 on dev: ~300 µs (criterion). ~16× headroom.
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "dispatch_request_multi_call median {median:?} exceeded budget {budget:?}"
    );
}

#[test]
fn build_email_meta_include_all_under_budget() {
    use mailrs_jmap::build::build_email_meta;
    use mailrs_jmap::types::{FLAG_ANSWERED, FLAG_SEEN, Message};

    let msg = Message {
        id: 1,
        mailbox_id: 1,
        uid: 1,
        sender: "Alice <a@x>".into(),
        recipients: "b@x".into(),
        subject: "hello".into(),
        date: 1_700_000_000,
        size: 1024,
        flags: FLAG_SEEN | FLAG_ANSWERED,
        internal_date: 1_700_000_001,
        message_id: "msg-1@x".into(),
        in_reply_to: String::new(),
        thread_id: "t-1".into(),
        user_address: "b@x".into(),
        new_content: Some("preview".into()),
        blob_id: "blob-1".into(),
    };

    let median = time_median(|| {
        let _ = build_email_meta(&msg, "msg-1", &None);
    });

    // Budget: 100 µs. Observed P95: ~3 µs (criterion). ~30× headroom.
    let budget = Duration::from_micros(100);
    assert!(
        median < budget,
        "build_email_meta_include_all median {median:?} exceeded budget {budget:?}"
    );
}
