//! Search, words and operators: the two cores find the same conversations.

use mailrs_core_api::method::conversation::SearchConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

/// Ids each store mints for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const HELLO: &str = "t-hello@partner.test";
const INVOICE: &str = "t-invoice@billing.test";
const SPAM: &str = "t-spam@spam.test";
const SHARED: &str = "t-shared@example.test";

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    // operators: each one, negation, `in:`, and mixed with words
    let operators = [
        "from:partner.test",
        "-from:partner.test",
        "to:alice",
        "subject:invoice",
        "has:attachment",
        "-has:attachment",
        "is:unread",
        "is:read",
        "is:starred",
        "after:2023-11-14",
        "before:2023-11-15",
        "in:anywhere",
        "in:junk",
        "in:archive",
        "in:sent",
        "hello from:friend",
    ];
    for q in ["hello", "invoice", "partner.test", "nothing-matches"]
        .into_iter()
        .chain(operators)
    {
        let req = SearchConversationsRequest {
            query: q.into(),
            filter: ConversationFilter {
                limit: 20,
                ..Default::default()
            },
            quarantined: false,
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
    // agreeing is not enough: both could find nothing
    for (q, want) in [
        ("from:partner.test", vec![HELLO, SHARED]),
        ("has:attachment", vec![INVOICE]),
        ("subject:invoice -has:attachment", vec![]),
        ("in:junk", vec![SPAM]),
    ] {
        let req = SearchConversationsRequest {
            query: q.into(),
            filter: ConversationFilter {
                limit: 20,
                ..Default::default()
            },
            quarantined: false,
        };
        for (core, got) in [
            ("fastcore", fc.search_conversations(ALICE, &req).await),
            ("pg", pg.search_conversations(ALICE, &req).await),
        ] {
            let mut ids: Vec<String> = got
                .expect("search")
                .items
                .into_iter()
                .map(|i| i.thread_id)
                .collect();
            ids.sort();
            assert_eq!(ids, want, "{core}: {q}");
        }
    }
}
