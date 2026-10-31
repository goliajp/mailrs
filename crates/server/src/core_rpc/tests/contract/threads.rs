//! Conversations: the lists and views webapi renders, search, and every
//! per-thread action, each followed by the views it changes.

use mailrs_core_api::method::conversation::{
    ConversationsByIdsRequest, ListConversationsRequest, SearchConversationsRequest,
};
use mailrs_core_api::method::thread::SnoozeRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{Cores, Diffs};
use super::{ALICE, BOB};

/// Ids each store mints, and the counters each keeps for itself: a modseq
/// orders changes within one store and means nothing across two, and an
/// action's `affected` counts threads on one store and messages on the
/// other. No client reads either.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "created_at",
    "modseq",
    "new_modseq",
    "affected",
];

/// The views a client switches between.
fn views() -> Vec<(&'static str, ConversationFilter)> {
    let base = ConversationFilter {
        limit: 50,
        ..Default::default()
    };
    vec![
        ("default", base.clone()),
        (
            "inbox",
            ConversationFilter {
                folder: Some("Inbox".into()),
                ..base.clone()
            },
        ),
        (
            "notifications and promotions",
            ConversationFilter {
                folder: Some("NP".into()),
                ..base.clone()
            },
        ),
        (
            "non-junk",
            ConversationFilter {
                folder: Some("NonJunk".into()),
                ..base.clone()
            },
        ),
        (
            "junk",
            ConversationFilter {
                folder: Some("Junk".into()),
                ..base.clone()
            },
        ),
        (
            "sent",
            ConversationFilter {
                folder: Some("Sent".into()),
                ..base.clone()
            },
        ),
        (
            "unread",
            ConversationFilter {
                unread: Some(true),
                ..base.clone()
            },
        ),
        (
            "starred",
            ConversationFilter {
                starred: Some(true),
                ..base.clone()
            },
        ),
        (
            "archived",
            ConversationFilter {
                archived: true,
                ..base.clone()
            },
        ),
        (
            "category notification",
            ConversationFilter {
                category: Some("notification".into()),
                ..base.clone()
            },
        ),
        (
            "first page of two",
            ConversationFilter {
                limit: 2,
                ..base.clone()
            },
        ),
        (
            "older than the third",
            ConversationFilter {
                before_ts: Some(1_700_000_400),
                ..base
            },
        ),
    ]
}

/// Every view, the unread count and the category counts, after `step`.
async fn compare_views(c: &Cores, d: &mut Diffs, step: &str) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    for (name, filter) in views() {
        let req = ListConversationsRequest { filter };
        d.both(
            &format!("list_conversations [{name}] after {step}"),
            fc.list_conversations(ALICE, &req).await,
            pg.list_conversations(ALICE, &req).await,
            MINTED,
        );
    }
    d.both(
        &format!("unseen_count after {step}"),
        fc.unseen_count(ALICE).await,
        pg.unseen_count(ALICE).await,
        &[],
    );
    d.both(
        &format!("conversation_categories after {step}"),
        fc.conversation_categories(ALICE).await,
        pg.conversation_categories(ALICE).await,
        &[],
    );
}

const HELLO: &str = "t-hello@partner.test";
const INVOICE: &str = "t-invoice@billing.test";
const SALE: &str = "t-sale@shop.test";
const SPAM: &str = "t-spam@spam.test";
const SHARED: &str = "t-shared@example.test";

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    compare_views(c, d, "delivery").await;

    for q in ["hello", "invoice", "partner.test", "nothing-matches"] {
        let req = SearchConversationsRequest {
            query: q.into(),
            category: None,
            limit: 20,
            folder: None,
            unread: None,
            starred: None,
            archived: false,
        };
        // Which conversations a search finds must agree; their order is each
        // store's own ranking, and a text index and Postgres rank differently.
        let by_thread = |r: mailrs_core_api::error::ApiResult<
            mailrs_core_api::method::conversation::SearchConversationsResponse,
        >| {
            r.map(|mut r| {
                r.items.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
                r
            })
        };
        d.both(
            &format!("search_conversations {q}"),
            by_thread(fc.search_conversations(ALICE, &req).await),
            by_thread(pg.search_conversations(ALICE, &req).await),
            MINTED,
        );
    }
    let req = ConversationsByIdsRequest {
        thread_ids: vec![HELLO.into(), SALE.into(), "t-missing@x".into()],
        folder: None,
    };
    d.both(
        "conversations_by_thread_ids",
        fc.conversations_by_thread_ids(ALICE, &req).await,
        pg.conversations_by_thread_ids(ALICE, &req).await,
        MINTED,
    );
    d.both(
        "list_thread_messages",
        fc.list_thread_messages(ALICE, HELLO).await,
        pg.list_thread_messages(ALICE, HELLO).await,
        MINTED,
    );
    d.both(
        "list_sent_messages",
        fc.list_sent_messages(ALICE).await,
        pg.list_sent_messages(ALICE).await,
        MINTED,
    );
    for mid in ["m2@partner.test", "m7@example.test", "missing@x"] {
        d.both(
            &format!("fraud_verdict {mid}"),
            fc.fraud_verdict(ALICE, mid)
                .await
                .map_err(|e| e.to_string()),
            pg.fraud_verdict(ALICE, mid)
                .await
                .map_err(|e| e.to_string()),
            &[],
        );
        // another account's message is not found, in both cores
        d.both(
            &format!("fraud_verdict {mid} as bob"),
            fc.fraud_verdict(BOB, mid).await.map_err(|e| e.to_string()),
            pg.fraud_verdict(BOB, mid).await.map_err(|e| e.to_string()),
            &[],
        );
    }
    d.both(
        "list_quarantined",
        fc.list_quarantined(ALICE, 50, None).await,
        pg.list_quarantined(ALICE, 50, None).await,
        MINTED,
    );

    // filing somebody else's thread finds nothing of the caller's in it
    d.both(
        "mark_junk as bob",
        fc.mark_junk(BOB, HELLO).await.map_err(|e| e.to_string()),
        pg.mark_junk(BOB, HELLO).await.map_err(|e| e.to_string()),
        &[],
    );

    // flagging somebody else's thread answers as a thread that is not there
    for name in [
        "star_thread",
        "unstar_thread",
        "pin_thread",
        "unpin_thread",
        "archive_thread",
        "unarchive_thread",
        "mark_thread_read",
        "mark_thread_unread",
        "release_quarantined",
    ] {
        let fc_answer = act_as(fc, BOB, name, HELLO)
            .await
            .map_err(|e| e.status_code());
        let pg_answer = act_as(pg, BOB, name, HELLO)
            .await
            .map_err(|e| e.status_code());
        assert_eq!(
            fc_answer.as_ref().err(),
            Some(&404),
            "fastcore: {name} as bob"
        );
        d.both(&format!("{name} as bob"), fc_answer, pg_answer, &[]);
    }
    let fc_answer = fc
        .delete_thread(BOB, HELLO)
        .await
        .map_err(|e| e.status_code());
    let pg_answer = pg
        .delete_thread(BOB, HELLO)
        .await
        .map_err(|e| e.status_code());
    assert_eq!(
        fc_answer.as_ref().err(),
        Some(&404),
        "fastcore: delete_thread as bob"
    );
    d.both("delete_thread as bob", fc_answer, pg_answer, &[]);
    compare_views(c, d, "flagging as bob").await;

    let steps = [
        ("mark_thread_read", HELLO),
        ("mark_thread_unread", HELLO),
        ("star_thread", INVOICE),
        ("pin_thread", SALE),
        ("archive_thread", SALE),
        ("unarchive_thread", SALE),
        ("unpin_thread", SALE),
        ("unstar_thread", INVOICE),
        ("mark_junk", INVOICE),
        ("mark_not_junk", SPAM),
        ("mark_notification", HELLO),
        ("mark_promotion", HELLO),
        ("move_to_inbox", HELLO),
        ("release_quarantined", SHARED),
    ];
    for (name, thread) in steps {
        d.both(
            &format!("{name} {thread}"),
            act(fc, name, thread).await,
            act(pg, name, thread).await,
            MINTED,
        );
        compare_views(c, d, &format!("{name} {thread}")).await;
    }

    let req = SnoozeRequest {
        snoozed_until: 4_000_000_000,
    };
    d.both(
        "snooze_thread",
        fc.snooze_thread(ALICE, SHARED, &req).await,
        pg.snooze_thread(ALICE, SHARED, &req).await,
        &[],
    );
    compare_views(c, d, "snooze").await;
    d.both(
        "unsnooze_thread",
        fc.unsnooze_thread(ALICE, SHARED).await,
        pg.unsnooze_thread(ALICE, SHARED).await,
        &[],
    );
    compare_views(c, d, "unsnooze").await;

    let filter = ConversationFilter {
        limit: 50,
        folder: Some("NP".into()),
        ..Default::default()
    };
    d.both(
        "mark_list_conversations_read NP",
        fc.mark_list_conversations_read(ALICE, &filter).await,
        pg.mark_list_conversations_read(ALICE, &filter).await,
        &[],
    );
    compare_views(c, d, "mark_list_conversations_read").await;
    d.both(
        "mark_all_conversations_read",
        fc.mark_all_conversations_read(ALICE).await,
        pg.mark_all_conversations_read(ALICE).await,
        &[],
    );
    compare_views(c, d, "mark_all_conversations_read").await;

    d.both(
        "delete_thread",
        fc.delete_thread(ALICE, SPAM).await,
        pg.delete_thread(ALICE, SPAM).await,
        &[],
    );
    compare_views(c, d, "delete_thread").await;
    // an empty Trash empties to nothing on both; what a full one does is
    // `trash::the_trash_answers_alike_on_both_cores`
    d.both(
        "empty_trash (nothing in it)",
        fc.empty_trash(BOB).await,
        pg.empty_trash(BOB).await,
        &[],
    );
}

async fn act(
    c: &mailrs_core_api::client::Client,
    name: &str,
    thread: &str,
) -> mailrs_core_api::error::ApiResult<mailrs_core_api::method::thread::ThreadActionResponse> {
    act_as(c, ALICE, name, thread).await
}

async fn act_as(
    c: &mailrs_core_api::client::Client,
    user: &str,
    name: &str,
    thread: &str,
) -> mailrs_core_api::error::ApiResult<mailrs_core_api::method::thread::ThreadActionResponse> {
    match name {
        "mark_thread_read" => c.mark_thread_read(user, thread).await,
        "mark_thread_unread" => c.mark_thread_unread(user, thread).await,
        "star_thread" => c.star_thread(user, thread).await,
        "unstar_thread" => c.unstar_thread(user, thread).await,
        "pin_thread" => c.pin_thread(user, thread).await,
        "unpin_thread" => c.unpin_thread(user, thread).await,
        "archive_thread" => c.archive_thread(user, thread).await,
        "unarchive_thread" => c.unarchive_thread(user, thread).await,
        "mark_junk" => c.mark_junk(user, thread).await,
        "mark_not_junk" => c.mark_not_junk(user, thread).await,
        "mark_notification" => c.mark_notification(user, thread).await,
        "mark_promotion" => c.mark_promotion(user, thread).await,
        "move_to_inbox" => c.move_to_inbox(user, thread).await,
        "release_quarantined" => c.release_quarantined(user, thread).await,
        other => unreachable!("no action {other}"),
    }
}
