//! Conversations: the lists and views webapi renders, search, and every
//! per-thread action, each followed by the views it changes.

use mailrs_core_api::method::conversation::{
    ConversationsByIdsRequest, ListConversationsRequest, SearchConversationsRequest,
};
use mailrs_core_api::method::thread::SnoozeRequest;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

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
            &format!("find_thread_by_message_id {mid}"),
            fc.find_thread_by_message_id(ALICE, mid).await,
            pg.find_thread_by_message_id(ALICE, mid).await,
            &[],
        );
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
    }
    d.both(
        "list_quarantined",
        fc.list_quarantined(ALICE, 50, None).await,
        pg.list_quarantined(ALICE, 50, None).await,
        MINTED,
    );

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
}

async fn act(
    c: &mailrs_core_api::client::Client,
    name: &str,
    thread: &str,
) -> mailrs_core_api::error::ApiResult<mailrs_core_api::method::thread::ThreadActionResponse> {
    match name {
        "mark_thread_read" => c.mark_thread_read(ALICE, thread).await,
        "mark_thread_unread" => c.mark_thread_unread(ALICE, thread).await,
        "star_thread" => c.star_thread(ALICE, thread).await,
        "unstar_thread" => c.unstar_thread(ALICE, thread).await,
        "pin_thread" => c.pin_thread(ALICE, thread).await,
        "unpin_thread" => c.unpin_thread(ALICE, thread).await,
        "archive_thread" => c.archive_thread(ALICE, thread).await,
        "unarchive_thread" => c.unarchive_thread(ALICE, thread).await,
        "mark_junk" => c.mark_junk(ALICE, thread).await,
        "mark_not_junk" => c.mark_not_junk(ALICE, thread).await,
        "mark_notification" => c.mark_notification(ALICE, thread).await,
        "mark_promotion" => c.mark_promotion(ALICE, thread).await,
        "move_to_inbox" => c.move_to_inbox(ALICE, thread).await,
        "release_quarantined" => c.release_quarantined(ALICE, thread).await,
        other => unreachable!("no action {other}"),
    }
}
