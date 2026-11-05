//! Stored dates that ignored the sender's zone, or are 0, go back to what
//! ingest computes from the file; each reader's row time follows.
//!
//! A dry run writes nothing, a run in batches ends where one run does, the
//! run after a real one finds nothing, a reader's flags, unread state and
//! membership do not move, and a `-0000` message stored nine hours early,
//! whose writer is unknown, is skipped and counted.
//!
//! One test per binary: the route reads `MAILRS_MAILDIR` from the process
//! environment.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";
// 2026-07-02 12:00:00 UTC
const NOON: i64 = 1_782_993_600;
const H: i64 = 3600;
const ARRIVED: i64 = NOON + 600;

struct Msg {
    tid: &'static str,
    mid: &'static str,
    date_header: &'static str,
    stored: i64,
    holders: &'static [&'static str],
}

const MAIL: &[Msg] = &[
    // stored with the zone ignored, each direction
    Msg {
        tid: "t1",
        mid: "tokyo",
        date_header: "Thu, 2 Jul 2026 21:00:00 +0900",
        stored: NOON + 9 * H,
        holders: &[ALICE],
    },
    Msg {
        tid: "t2",
        mid: "denver",
        date_header: "Thu, 2 Jul 2026 05:00:00 -0700",
        stored: NOON - 7 * H,
        holders: &[ALICE],
    },
    Msg {
        tid: "t3",
        mid: "zero",
        date_header: "Thu, 2 Jul 2026 11:58:20 +0000",
        stored: 0,
        holders: &[ALICE],
    },
    Msg {
        tid: "t4",
        mid: "minuszero",
        date_header: "Thu, 2 Jul 2026 12:00:00 -0000",
        stored: NOON - 9 * H,
        holders: &[ALICE],
    },
    Msg {
        tid: "t5",
        mid: "chicago",
        date_header: "Thu, 2 Jul 2026 07:00:00 -0500",
        stored: NOON - 5 * H,
        holders: &[ALICE, BOB],
    },
    Msg {
        tid: "t6",
        mid: "agrees",
        date_header: "Thu, 2 Jul 2026 11:59:10 +0000",
        stored: NOON - 50,
        holders: &[BOB],
    },
    Msg {
        tid: "t7",
        mid: "odd",
        date_header: "Thu, 2 Jul 2026 21:00:00 +0900",
        stored: NOON + 3 * H,
        holders: &[ALICE],
    },
];

fn file_name(mid: &str) -> String {
    format!("{ARRIVED}.M1P1Q{mid}.host:2,")
}

fn maildir(root: &std::path::Path) {
    for user in [ALICE, BOB] {
        let (local, domain) = user.split_once('@').expect("addr");
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(root.join(domain).join(local).join(leaf)).expect("maildir");
        }
    }
    for m in MAIL {
        for user in m.holders {
            let (local, domain) = user.split_once('@').expect("addr");
            let raw = format!(
                "From: carol@y.com\r\nDate: {}\r\nSubject: {}\r\n\r\nbody\r\n",
                m.date_header, m.mid
            );
            std::fs::write(
                root.join(domain)
                    .join(local)
                    .join("cur")
                    .join(file_name(m.mid)),
                raw,
            )
            .expect("file");
        }
    }
}

fn state() -> Arc<mailrs_fastcore::FastcoreState> {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    for user in [ALICE, BOB] {
        s.upsert_account(user, &format!(r#"{{"address":"{user}","active":true}}"#))
            .expect("account");
    }
    for m in MAIL {
        let payload = format!(
            r#"{{"message_id":"{}","sender":"carol@y.com","thread_id":"{}","internal_date":{},"date":{},"subject":"{}","flags":0}}"#,
            m.mid, m.tid, m.stored, m.stored, m.mid
        );
        for user in m.holders {
            let file = file_name(m.mid);
            s.deliver_message(
                &mailrs_mailbox_kevy::MessageArrival {
                    thread_id: m.tid,
                    user,
                    subject: m.mid,
                    senders_csv: "carol@y.com",
                    latest_date: m.stored,
                    latest_preview: "",
                    category: "inbox",
                    unread: true,
                    is_own: false,
                },
                m.mid,
                payload.as_bytes(),
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref: &file,
                    uid: 7,
                    flags: 0,
                    modseq: 1,
                },
            )
            .expect("deliver");
        }
    }
    s.mark_seen(BOB, "t5").expect("read");
    Arc::new(mailrs_fastcore::FastcoreState::new(s))
}

/// Every stored date: each message, each reader's row, each shared hash.
fn dates(s: &mailrs_fastcore::FastcoreState) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for m in MAIL {
        let blob = s.mailbox.get_message(m.mid).expect("read").expect("blob");
        let v: serde_json::Value = serde_json::from_slice(&blob).expect("json");
        out.push((
            m.mid.to_string(),
            v["internal_date"].as_i64().expect("date"),
        ));
        let shared = s.mailbox.get_thread(m.tid).expect("read").expect("thread");
        out.push((format!("{} shared", m.tid), shared.latest_date));
        for user in m.holders {
            out.push((format!("{user} {}", m.tid), row(s, user, m.tid)));
        }
    }
    out
}

fn row(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str) -> i64 {
    s.mailbox
        .get_thread_for_user(user, tid)
        .expect("read")
        .expect("row")
        .latest_date
}

/// What each reader owns: their message rows, unread counts, membership.
fn owned(s: &mailrs_fastcore::FastcoreState) -> Vec<String> {
    let mut out = Vec::new();
    for m in MAIL {
        for user in m.holders {
            let mut facts = s
                .mailbox
                .store_ref()
                .hgetall(mailrs_mailbox_kevy::keys::user_message(user, m.mid).as_bytes())
                .expect("facts");
            facts.sort();
            let r = s
                .mailbox
                .get_thread_for_user(user, m.tid)
                .expect("read")
                .expect("row");
            out.push(format!(
                "{user} {} {facts:?} unread={} count={}",
                m.mid, r.unread_count, r.count
            ));
        }
        let mut owners = s.mailbox.thread_message_owners(m.tid).expect("owners");
        owners.sort();
        out.push(format!("{owners:?}"));
    }
    out
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-message-dates{query}"
                ))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body"),
    )
    .expect("json")
}

#[tokio::test]
async fn misdated_mail_is_corrected_once_and_nothing_else_moves() {
    let tmp = tempfile::tempdir().expect("tmp");
    maildir(tmp.path());
    // SAFETY-adjacent: own process, set before any router is built.
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    let s = state();
    let stored = dates(&s);
    let before = owned(&s);

    let dry = call(&s, "").await;
    assert_eq!(dates(&s), stored, "a dry run wrote: {dry}");
    assert_eq!(dry["messages_walked"], 7, "{dry}");
    assert_eq!(dry["agree"], 1, "{dry}");
    assert_eq!(dry["messages_to_correct"], 4, "{dry}");
    assert_eq!(dry["skipped_minus_zero"], 1, "{dry}");
    assert_eq!(dry["skipped_unexplained"], 1, "{dry}");
    let alice = &dry["by_user"][ALICE];
    assert_eq!(alice["messages_time_changes"], 4, "{dry}");
    assert_eq!(alice["zone_ignored"], 3, "{dry}");
    assert_eq!(alice["zero_date"], 1, "{dry}");
    assert_eq!(alice["rows_time_changes"], 4, "{dry}");
    let bob = &dry["by_user"][BOB];
    assert_eq!(bob["messages_time_changes"], 1, "{dry}");
    // Bob's t5 passes his t6, which stays where it was
    assert_eq!(bob["rows_time_changes"], 1, "{dry}");
    assert_eq!(bob["rows_move"], 1, "{dry}");
    assert_eq!(bob["rows_move_in_newest_200"], 1, "{dry}");

    // the same mail corrected two conversations at a time
    let batched = state();
    let mut skip = 0;
    loop {
        let r = call(&batched, &format!("?dry_run=false&limit=2&skip={skip}")).await;
        skip = r["next_skip"].as_u64().expect("next") as usize;
        if r["done"] == true {
            break;
        }
    }

    let real = call(&s, "?dry_run=false").await;
    assert_eq!(real["threads_owned_changed"], 0, "{real}");
    assert_eq!(real["owned_before"], real["owned_after"], "{real}");
    // t1, t2, t3 and Alice's and Bob's t5
    assert_eq!(real["owned_before"]["rows"], 5, "{real}");
    assert_eq!(real["threads_written"], 4, "{real}");
    assert_eq!(real["errors"], 0, "{real}");
    let now = dates(&s);
    assert_eq!(now, dates(&batched), "batches ended elsewhere than one run");
    let at = |k: &str| now.iter().find(|(n, _)| n == k).expect(k).1;
    for mid in ["tokyo", "denver", "chicago"] {
        assert_eq!(at(mid), NOON, "{mid}");
    }
    assert_eq!(at("zero"), NOON - 100);
    assert_eq!(
        at("minuszero"),
        NOON - 9 * H,
        "the -0000 message was touched"
    );
    assert_eq!(at("odd"), NOON + 3 * H, "an unexplained date was touched");
    assert_eq!(at("agrees"), NOON - 50);
    for (k, want) in [
        ("alice@x.com t1", NOON),
        ("alice@x.com t2", NOON),
        ("alice@x.com t3", NOON - 100),
        ("alice@x.com t5", NOON),
        ("bob@x.com t5", NOON),
        ("t5 shared", NOON),
        ("t1 shared", NOON),
    ] {
        assert_eq!(at(k), want, "{k}");
    }
    assert_eq!(owned(&s), before, "flags, unread state or membership moved");

    let again = call(&s, "").await;
    assert_eq!(again["messages_walked"], 7, "{again}");
    assert_eq!(again["messages_to_correct"], 0, "{again}");
    assert_eq!(again["rows_time_changes"], 0, "{again}");
    assert_eq!(again["skipped_minus_zero"], 1, "{again}");
}
