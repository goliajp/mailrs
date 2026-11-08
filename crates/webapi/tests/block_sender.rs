//! Blocking a sender from their message: the address goes on the reader's
//! blacklist and off their whitelist, the conversation goes to Junk, and
//! the next message from that address is filed in Junk on arrival. The
//! undo takes all of it back.

mod live;

use std::collections::HashSet;

use mailrs_core_sidestate::families::sender_lists;
use mailrs_inbound::{
    AuthResults, DeliveryDecision, DmarcPolicy, PipelineInput, make_delivery_decision,
};
use reqwest::Method;

const KEVY: &str = "mem://webapi-block-sender";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@partner.example";
const FRIEND: &str = "friend@partner.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_a_sender_files_their_next_message_in_junk() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "hello",
            senders_csv: "Friend <Friend@Partner.example>",
            subject: "hello",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    // an earlier "not junk" left the sender on the whitelist, which
    // delivery reads before the blacklist
    sender_lists::allow(&mut kv, ALICE, FRIEND).expect("allow");

    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/block-sender"),
            &alice,
            Some(serde_json::json!({ "address": ALICE })),
        )
        .await;
    assert_eq!(status, 422, "blocking her own address");
    assert_ne!(category(&live), "spam", "refused, and moved anyway");

    let (status, _) = live
        .call(
            Method::POST,
            "/api/conversations/t-nobody/block-sender",
            &alice,
            Some(serde_json::json!({ "address": "x@nowhere.example" })),
        )
        .await;
    assert_eq!(status, 404, "a thread that is not hers");
    let blocked = sender_lists::blacklist(&mut kv, ALICE).expect("blacklist");
    assert!(blocked.is_empty(), "written for a 404: {blocked:?}");

    let (status, body) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/block-sender"),
            &alice,
            Some(serde_json::json!({ "address": " Friend@Partner.example" })),
        )
        .await;
    assert_eq!(status, 200, "block: {body}");
    assert_eq!(body["address"], FRIEND);
    assert_eq!(body["was_allowed"], true);
    assert_eq!(category(&live), "spam", "the conversation is in Junk");
    let (allowed, blocked) = lists(&mut kv);
    assert!(blocked.contains(FRIEND), "blacklist: {blocked:?}");
    assert!(!allowed.contains(FRIEND), "still allowed: {allowed:?}");
    assert!(
        matches!(arrival_from(FRIEND, &mut kv), DeliveryDecision::Junk { .. }),
        "the next message from the sender is not filed in Junk"
    );
    // someone else's mail is untouched
    assert!(matches!(
        arrival_from("other@partner.example", &mut kv),
        DeliveryDecision::Accept { .. }
    ));

    // the undo: back where it was, on the whitelist it came from
    let (status, body) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/unblock-sender"),
            &alice,
            Some(serde_json::json!({ "address": FRIEND, "allow": true })),
        )
        .await;
    assert_eq!(status, 204, "unblock: {body}");
    assert_ne!(category(&live), "spam", "still in Junk after the undo");
    let (allowed, blocked) = lists(&mut kv);
    assert!(!blocked.contains(FRIEND), "still blocked: {blocked:?}");
    assert!(allowed.contains(FRIEND), "not allowed again: {allowed:?}");
    assert!(matches!(
        arrival_from(FRIEND, &mut kv),
        DeliveryDecision::Accept { .. }
    ));

    // blocked again and undone without `allow`: on neither list, and the
    // undo whitelists nobody in the thread, as `mark-not-junk` would
    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/block-sender"),
            &alice,
            Some(serde_json::json!({ "address": FRIEND })),
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/unblock-sender"),
            &alice,
            Some(serde_json::json!({ "address": FRIEND })),
        )
        .await;
    assert_eq!(status, 204);
    let (allowed, blocked) = lists(&mut kv);
    assert!(blocked.is_empty(), "blacklist: {blocked:?}");
    assert!(allowed.is_empty(), "whitelist: {allowed:?}");
}

fn category(live: &live::Live) -> String {
    live.mailbox
        .get_thread_for_user(ALICE, TID)
        .expect("row")
        .expect("alice's thread")
        .category
}

fn lists(kv: &mut kevy_client::Connection) -> (HashSet<String>, HashSet<String>) {
    (
        sender_lists::whitelist(kv, ALICE).expect("whitelist"),
        sender_lists::blacklist(kv, ALICE).expect("blacklist"),
    )
}

/// The delivery decision for a clean message from `from` to alice, with
/// her lists read the way the receiver reads them.
fn arrival_from(from: &str, kv: &mut kevy_client::Connection) -> DeliveryDecision {
    let (recipient_whitelist, recipient_blacklist) = lists(kv);
    make_delivery_decision(&PipelineInput {
        greylisted: false,
        auth: AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: DmarcPolicy::Pass,
            dkim_signatures: Vec::new(),
        },
        virus_found: None,
        content_score: 0.0,
        format_score: 0.0,
        list_mail: false,
        matched_rules: vec![],
        ptr_score: 0.0,
        ai_score: 0.0,
        deception: Default::default(),
        fraud: mailrs_fraud::Findings::new(),
        spam_threshold: 5.0,
        hostname: "mx.example.test".into(),
        from_addr: from.into(),
        header_from_addr: from.into(),
        recipient_whitelist,
        recipient_blacklist,
        local_domains: HashSet::new(),
        sender_allowed: false,
        own_mail: false,
    })
}
