//! A list paged across a second several conversations share: by cursor,
//! every one of them comes back exactly once, on both cores, in the same
//! order; by time alone (`before_ts`) the rest of that second is skipped,
//! which is the defect the cursor exists for. Asking both ways at once is
//! refused.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{self, Cores, Diffs};

/// Seven conversations: one a minute newer, five in one second, one a
/// minute older. Imported mail keeps the time its Date header gives.
fn mail(n: usize, date: &str) -> Vec<u8> {
    format!(
        "Date: {date}\r\nFrom: Sender {n} <s{n}@outside.test>\r\nTo: {ALICE}\r\n\
         Subject: paging {n}\r\nMessage-ID: <paging-{n}@outside.test>\r\n\r\nhello {n}\r\n"
    )
    .into_bytes()
}

const NEWER: &str = "Wed, 01 Mar 2023 10:01:00 +0000";
const SAME: &str = "Wed, 01 Mar 2023 10:00:00 +0000";
const OLDER: &str = "Wed, 01 Mar 2023 09:59:00 +0000";

fn request(limit: u32, before_ts: Option<i64>, after: Option<String>) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Inbox".into()),
            limit,
            before_ts,
            after_cursor: after,
            ..Default::default()
        },
    }
}

/// Every thread id the list gives, page after page of `limit`, following
/// each page's last cursor; and the pages as they came.
async fn by_cursor(c: &mailrs_core_api::client::Client, limit: u32) -> Vec<String> {
    let mut seen = Vec::new();
    let mut after = None;
    for _ in 0..20 {
        let page = c
            .list_conversations(ALICE, &request(limit, None, after.clone()))
            .await
            .expect("page")
            .items;
        let Some(last) = page.last() else {
            break;
        };
        assert!(!last.cursor.is_empty(), "a listed row carries its cursor");
        after = Some(last.cursor.clone());
        seen.extend(page.into_iter().map(|r| r.thread_id));
    }
    seen
}

/// The same paging by time alone, the way clients paged before cursors.
async fn by_time(c: &mailrs_core_api::client::Client, limit: u32) -> Vec<String> {
    let mut seen = Vec::new();
    let mut before = None;
    for _ in 0..20 {
        let page = c
            .list_conversations(ALICE, &request(limit, before, None))
            .await
            .expect("page")
            .items;
        let Some(last) = page.last() else {
            break;
        };
        before = Some(last.last_date);
        seen.extend(page.into_iter().map(|r| r.thread_id));
    }
    seen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_shared_by_conversations_pages_whole_on_both_cores() {
    let cores = harness::start_on(
        "mem://paging-fastcore-side-state",
        "mem://paging-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    super::accounts::seed(&cores, &mut d).await;
    seed(&cores).await;

    let want: Vec<String> = (0..7).map(|n| format!("paging-{n}@outside.test")).collect();
    let mut orders = Vec::new();
    for (name, c) in [("fastcore", &cores.fastcore), ("pg-core", &cores.pg_core)] {
        let seen = by_cursor(c, 2).await;
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(seen.len(), 7, "{name}: every conversation once: {seen:?}");
        assert_eq!(sorted, want, "{name}");
        assert_eq!(
            seen.first().map(String::as_str),
            Some("paging-0@outside.test"),
            "{name}: newest first"
        );
        assert_eq!(
            seen.last().map(String::as_str),
            Some("paging-6@outside.test"),
            "{name}: oldest last"
        );

        let old = by_time(c, 2).await;
        assert!(
            old.len() < 7,
            "{name}: paging by time alone was expected to skip the rest of the shared second: {old:?}"
        );

        let both = c
            .list_conversations(ALICE, &request(2, Some(1), Some("1:1".into())))
            .await;
        assert_eq!(
            both.err().map(|e| e.status_code()),
            Some(400),
            "{name}: before_ts and a cursor"
        );
        orders.push(seen);
    }
    assert_eq!(orders[0], orders[1], "the two cores order one second alike");
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
}

async fn seed(c: &Cores) {
    let dates = [NEWER, SAME, SAME, SAME, SAME, SAME, OLDER];
    for (n, date) in dates.iter().enumerate() {
        let raw = mail(n, date);
        for core in [&c.fastcore, &c.pg_core] {
            core.import_message(ALICE, &raw, None)
                .await
                .expect("import");
        }
    }
}
