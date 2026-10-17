//! "Not junk" whitelists the thread's senders for the person who said it,
//! and for nobody else: the admin's sender lists, which apply to every
//! recipient, are left as they were.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-not-junk-stays-personal";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@partner.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn marking_not_junk_leaves_the_admin_sender_lists_alone() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "junk",
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

    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/conversations/{TID}/mark-not-junk"),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 204);

    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let personal = mailrs_core_sidestate::families::sender_lists::whitelist(&mut kv, ALICE)
        .expect("whitelist");
    assert!(
        personal.contains("friend@partner.example"),
        "alice's own whitelist: {personal:?}"
    );
    let global = kv
        .hgetall(b"admin:greylist:local-lists")
        .expect("local lists");
    assert!(
        global.is_empty(),
        "the admin's lists were written: {global:?}"
    );
}
