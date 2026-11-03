//! A conversation nobody holds is listed for the super administrator and
//! deleted one at a time, after a recent password confirmation, leaving an
//! audit row. One somebody holds is refused with 409; nobody else reaches
//! either route.

mod live;

use mailrs_mailbox_kevy::{MessageArrival, UserMessageFacts, keys};
use reqwest::Method;

const KEVY: &str = "mem://webapi-ownerless-threads";
const ROOT: &str = "root@example.test";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

fn deliver(live: &live::Live, tid: &str, mid: &str) {
    let arrival = MessageArrival {
        thread_id: tid,
        user: ALICE,
        subject: "Subj",
        senders_csv: "other@z.test",
        latest_date: 1_790_000_000,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: false,
    };
    let payload =
        format!(r#"{{"message_id":"{mid}","sender":"other@z.test","thread_id":"{tid}"}}"#);
    let facts = UserMessageFacts {
        blob_ref: "1.M1.host",
        uid: 1,
        flags: 0,
        modseq: 1,
    };
    live.mailbox
        .deliver_message(&arrival, mid, payload.as_bytes(), &facts)
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_super_administrator_deletes_what_nobody_holds() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ROOT, ADMIN, ALICE] {
        live.add_account(who, PW).await;
    }
    live.make_super(ROOT);
    live.grant(ADMIN, "admin.accounts").await;
    live.mailbox.ensure_thread_table();
    deliver(&live, "t-held@x.test", "m1@x.test");
    deliver(&live, "t-orphan@x.test", "m2@x.test");
    // what production has: the shared record outlived the account's rows
    let store = live.mailbox.store_ref();
    store
        .del(&[keys::user_message(ALICE, "m2@x.test").as_bytes()])
        .unwrap();
    store
        .del(&[keys::thread_user(ALICE, "t-orphan@x.test").as_bytes()])
        .unwrap();

    let root = live.login(ROOT, PW).await;
    let admin = live.login(ADMIN, PW).await;
    let alice = live.login(ALICE, PW).await;
    let list = "/api/admin/ownerless-threads";
    let orphan = "/api/admin/ownerless-threads/t-orphan@x.test";
    let held = "/api/admin/ownerless-threads/t-held@x.test";

    for who in [&admin, &alice] {
        assert_eq!(live.call(Method::GET, list, who, None).await.0, 403);
        assert_eq!(live.call(Method::DELETE, orphan, who, None).await.0, 403);
    }

    let (status, got) = live.call(Method::GET, list, &root, None).await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["count"], 1, "{got}");
    assert_eq!(got["items"][0]["thread_id"], "t-orphan@x.test");
    assert_eq!(got["items"][0]["id_prefix"], "t-orphan");
    assert_eq!(got["items"][0]["messages"], 1);

    let (status, body) = live.call(Method::DELETE, orphan, &root, None).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("confirm_required")),
        "{body}"
    );
    live.confirm(&root, PW).await;
    assert_eq!(live.call(Method::DELETE, held, &root, None).await.0, 409);
    let (status, body) = live.call(Method::DELETE, orphan, &root, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["messages"], 1);
    assert_eq!(live.call(Method::DELETE, orphan, &root, None).await.0, 404);

    let (_, got) = live.call(Method::GET, list, &root, None).await;
    assert_eq!(got["count"], 0, "{got}");
    let (_, log) = live
        .call(
            Method::GET,
            "/api/admin/audit-log?action=ownerless_thread.delete",
            &root,
            None,
        )
        .await;
    let rows: Vec<_> = log["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|r| r["target"] == "t-orphan@x.test")
        .collect();
    assert_eq!(rows.len(), 1, "{log}");
    assert_eq!(rows[0]["actor"], ROOT);
    assert_eq!(rows[0]["detail"], "messages=1");
    assert!(
        live.mailbox
            .get_thread_for_user(ALICE, "t-held@x.test")
            .unwrap()
            .is_some(),
        "the held conversation is untouched"
    );
}
