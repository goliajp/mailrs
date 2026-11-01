//! The thread webhook events, raised alike by both cores: the same calls on
//! the same conversation queue the same payloads in each core's outbox.

use mailrs_core_api::method::label::CreateLabelRequest;
use mailrs_core_api::webhook_events as ev;
use mailrs_core_sidestate::families::{webhook_outbox, webhooks};

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

const THREAD_EVENTS: [&str; 5] = [
    ev::THREAD_READ,
    ev::THREAD_ARCHIVED,
    ev::THREAD_TRASHED,
    ev::THREAD_STARRED,
    ev::THREAD_LABELLED,
];

fn conn(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("kevy")
}

/// What is in the outbox now, taken off it: each payload without its
/// timestamp, sorted, so two cores' queues compare.
fn drain(url: &str) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = webhook_outbox::claim_due(&mut conn(url), i64::MAX, 1000)
        .expect("claim")
        .into_iter()
        .map(|e| {
            let mut p: serde_json::Value = serde_json::from_str(&e.payload).expect("json");
            p["timestamp"] = serde_json::Value::Null;
            p
        })
        .collect();
    out.sort_by_key(|p| p.to_string());
    out
}

/// The events are queued off the request path; wait for `want` of them.
async fn drained(url: &str, want: usize) -> Vec<serde_json::Value> {
    let mut got = Vec::new();
    for _ in 0..50 {
        got.extend(drain(url));
        if got.len() >= want {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    got.sort_by_key(|p| p.to_string());
    got
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    // a conversation delivered to alice in `mail::seed`, still hers
    let tid = "t-hello@partner.test".to_string();
    let label = CreateLabelRequest {
        name: "hooked".into(),
        color: "blue".into(),
    };
    let made = d.both(
        "create_label for webhooks",
        fc.create_label(ALICE, &label).await,
        pg.create_label(ALICE, &label).await,
        // ids are each store's counter, and earlier steps made labels on one
        &["created_at", "id"],
    );
    // each core's own id: the stores mint them independently
    let Some((fc_label, pg_label)) = made else {
        return;
    };
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        drain(url);
        for kind in THREAD_EVENTS {
            webhooks::create(
                &mut conn(url),
                webhooks::NewWebhook {
                    account_address: ALICE.into(),
                    url: "https://hooks.example/in".into(),
                    event_type: kind.into(),
                    filter_sender: None,
                    filter_thread_id: None,
                },
            )
            .expect("subscribe");
        }
    }
    for (core, label_id) in [(fc, &fc_label.id), (pg, &pg_label.id)] {
        core.mark_thread_read(ALICE, &tid).await.expect("read");
        core.mark_thread_unread(ALICE, &tid).await.expect("unread");
        core.archive_thread(ALICE, &tid).await.expect("archive");
        core.unarchive_thread(ALICE, &tid).await.expect("unarchive");
        core.star_thread(ALICE, &tid).await.expect("star");
        core.unstar_thread(ALICE, &tid).await.expect("unstar");
        core.trash_thread(ALICE, &tid).await.expect("trash");
        core.untrash_thread(ALICE, &tid).await.expect("untrash");
        core.set_thread_label(ALICE, &tid, label_id, true)
            .await
            .expect("label");
        core.set_thread_label(ALICE, &tid, label_id, false)
            .await
            .expect("unlabel");
    }
    let (a, b) = (
        drained(FASTCORE_KEVY, 10).await,
        drained(PG_CORE_KEVY, 10).await,
    );
    d.same("thread webhook events", &a, &b, &["label_id"]);
    assert_eq!(a.len(), 10, "{a:?}");
    for kind in THREAD_EVENTS {
        let of_kind: Vec<_> = a.iter().filter(|p| p["event"] == kind).collect();
        assert_eq!(of_kind.len(), 2, "{kind}: one each way");
        assert!(
            of_kind
                .iter()
                .all(|p| p["data"]["thread_id"] == tid.as_str())
        );
    }
    for (queued, label) in [(&a, &fc_label), (&b, &pg_label)] {
        let labelled = queued
            .iter()
            .find(|p| p["event"] == ev::THREAD_LABELLED)
            .expect("labelled");
        assert_eq!(labelled["data"]["label_id"], label.id.as_str());
    }
}
