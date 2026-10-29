//! A read of someone else's mail that cannot be recorded does not happen:
//! with the network kevy (where the audit log lives) unreachable, every
//! audit read answers 503 and returns no mail, although the mail is there.

mod live;

use axum::extract::{Extension, Path, Query, State};
use mailrs_webapi::handlers::audit_read::{
    AuditConvMessagesQuery, AuditConversationsQuery, read_conversation_detail,
    read_conversation_messages, read_conversations, read_message_raw,
};
use mailrs_webapi::handlers::conversations::AuthedUser;

const UNREACHABLE: &str = "kevy://127.0.0.1:1";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@partner.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audit_read_that_cannot_be_recorded_answers_503_and_no_mail() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", UNREACHABLE) };
    let live = live::start(UNREACHABLE).await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "hello",
            senders_csv: "Friend <friend@partner.example>",
            subject: "hello",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let there = live
        .core
        .list_conversations(
            ALICE,
            &mailrs_core_api::method::conversation::ListConversationsRequest {
                filter: mailrs_core_api::types::ConversationFilter {
                    limit: 10,
                    ..Default::default()
                },
            },
        )
        .await
        .expect("core list");
    assert_eq!(there.items.len(), 1, "the mail is there to be read");

    let st = || State(live.state.clone());
    let admin = || Extension(AuthedUser(ADMIN.into()));
    let of_alice = || Query(AuditConvMessagesQuery { user: ALICE.into() });

    let list = read_conversations(
        st(),
        admin(),
        Query(AuditConversationsQuery {
            user: Some(ALICE.into()),
            limit: 10,
        }),
    )
    .await;
    assert_eq!(
        list.err(),
        Some(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    );
    let detail = read_conversation_detail(st(), admin(), Path(TID.into()), of_alice()).await;
    assert_eq!(
        detail.err(),
        Some(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    );
    let messages = read_conversation_messages(st(), admin(), Path(TID.into()), of_alice()).await;
    assert_eq!(
        messages.err(),
        Some(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    );
    let raw = read_message_raw(st(), admin(), Path(1), of_alice()).await;
    assert_eq!(raw.err(), Some(axum::http::StatusCode::SERVICE_UNAVAILABLE));
}
