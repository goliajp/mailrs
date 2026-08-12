//! Merging two threads' rows into one.

//! Thread-merge + `Message-ID → thread_id` index (v2.9.5 threading fix).
//!
//! Threading fragmented because the write paths derived thread ids from
//! three inconsistent rules and nothing recorded which thread a given
//! RFC 5322 `Message-ID:` landed in — a reply carrying `In-Reply-To:
//! <our-msgid>` could not find its conversation and opened a new one.
//! This module adds the reconciliation index and the merge primitive the
//! rethread backfill uses to heal existing fragments.

pub(crate) fn combine_rows(
    into_tid: &str,
    a: crate::thread_row::ThreadRow,
    b: crate::thread_row::ThreadRow,
) -> crate::thread_row::ThreadRow {
    // The display fields follow the fresher side — but a side made up
    // entirely of the user's own sends is not allowed to be that side
    // while the other has inbound mail in it.
    //
    // Without that second clause, replying with a changed subject moved
    // the conversation to the top of Inbox stamped with the reply's
    // time. Gmail's subject-change rule opens the reply as its own
    // thread first; that thread is sent-only, so its `latest_date` is
    // legitimately the send time; the rethread pass then merges it back
    // and "fresher" handed the whole conversation that timestamp. The
    // row is supposed to follow the last INBOUND message (2026-07-18) —
    // `record_message_arrival` and the maildir self-heal both enforce
    // that, and this path never learned it, so the repair those two do
    // was undone by the merge.
    //
    // `sent_count == count` is the row-level spelling of "nothing here
    // but my own": a merged row's `senders_csv` holds both sides'
    // senders, so it cannot answer this and the counters can.
    let (latest, older) = if display_side_is_a(&a, &b) {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    };
    let mut senders: Vec<String> = Vec::new();
    for part in a.senders_csv.split(',').chain(b.senders_csv.split(',')) {
        let p = part.trim();
        if !p.is_empty() && !senders.iter().any(|s| s.eq_ignore_ascii_case(p)) {
            senders.push(p.to_string());
        }
    }
    let mut importance_score = a.importance_score;
    if b.importance_score > importance_score {
        importance_score = b.importance_score;
    }
    crate::thread_row::ThreadRow {
        thread_id: into_tid.to_string(),
        subject: latest.subject,
        senders_csv: senders.join(","),
        count: a.count + b.count,
        unread_count: a.unread_count + b.unread_count,
        latest_date: latest.latest_date,
        latest_preview: latest.latest_preview,
        category: latest.category,
        importance_level: older.importance_level,
        importance_score,
        requires_action: a.requires_action || b.requires_action,
        pinned: a.pinned || b.pinned,
        archived: a.archived && b.archived,
        has_action: a.has_action || b.has_action,
        sent_count: a.sent_count + b.sent_count,
        starred: a.starred || b.starred,
        // The sooner of the two, and never a sleep the merge invents:
        // `archived` above is `a && b`, so a merge of one snoozed
        // thread with a live one comes back to the inbox, and the
        // wake time has to come back with it.
        snoozed_until: match (a.snoozed_until, b.snoozed_until) {
            (0, _) | (_, 0) => 0,
            (x, y) => x.min(y),
        },
    }
}

/// Whether `a` supplies the display fields when merged with `b`.
///
/// Fresher wins, except that a sent-only side yields to one with
/// inbound mail regardless of dates. Two sent-only sides — or two with
/// inbound — fall back to fresher, which is the old behaviour.
fn display_side_is_a(
    a: &crate::thread_row::ThreadRow,
    b: &crate::thread_row::ThreadRow,
) -> bool {
    let a_sent_only = is_sent_only(a);
    let b_sent_only = is_sent_only(b);
    if a_sent_only != b_sent_only {
        return b_sent_only;
    }
    a.latest_date >= b.latest_date
}

/// Every message in this thread is one the user sent.
///
/// `count` can be 0 on rows whose message entities were never indexed
/// (182 of them on prod when the per-user projection shipped); those
/// are not evidence of a sent-only thread, so they are not treated as
/// one.
fn is_sent_only(row: &crate::thread_row::ThreadRow) -> bool {
    row.count > 0 && row.sent_count >= row.count
}

#[cfg(test)]
mod search_tests {
    use super::combine_rows;
    use crate::{KevyMailboxStore, ThreadRow};
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("in-memory kevy"));
        let mb = KevyMailboxStore::new(s);
        mb.ensure_admin_indexes();
        mb
    }

    fn row(tid: &str, subject: &str, senders: &str, preview: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: subject.into(),
            senders_csv: senders.into(),
            count: 1,
            unread_count: 0,
            latest_date: 100,
            latest_preview: preview.into(),
            category: "inbox".into(),
            importance_level: String::new(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
            snoozed_until: 0,
        }
    }

    /// The bug this rule exists for, in the shape it was reported.
    ///
    /// A reply whose subject changed opens as its own thread (Gmail's
    /// rule), that thread is sent-only so it carries the send time, and
    /// the rethread pass merges it back into the conversation. Before
    /// the inbound clause, "fresher wins" handed the whole conversation
    /// the reply's timestamp and it jumped to the top of Inbox dated
    /// two days after the last thing that actually arrived.
    #[test]
    fn my_own_reply_does_not_re_date_the_conversation() {
        let mut inbound = row("t-inbound", "Payment rejection", "them@qti.example", "hello");
        inbound.count = 3;
        inbound.sent_count = 1;
        inbound.latest_date = 1_786_361_580; // the last thing they sent

        let mut mine = row("t-mine", "PO 4300078149 / Updated Banking", "me@golia.jp", "");
        mine.count = 1;
        mine.sent_count = 1; // sent-only: nothing arrived here
        mine.latest_date = 1_786_541_659; // two days newer

        let merged = combine_rows("t-inbound", inbound.clone(), mine.clone());
        assert_eq!(
            merged.latest_date, inbound.latest_date,
            "the row took the reply's time and jumped to the top of Inbox"
        );
        assert_eq!(
            merged.subject, inbound.subject,
            "the row took the reply's subject"
        );
        // argument order must not decide it
        let other_way = combine_rows("t-inbound", mine, inbound.clone());
        assert_eq!(other_way.latest_date, inbound.latest_date);
    }

    #[test]
    fn two_threads_with_inbound_still_take_the_fresher() {
        let mut older = row("t1", "older", "a@x.com", "");
        older.count = 2;
        older.sent_count = 0;
        older.latest_date = 100;
        let mut newer = row("t2", "newer", "b@x.com", "");
        newer.count = 2;
        newer.sent_count = 0;
        newer.latest_date = 900;
        assert_eq!(combine_rows("t1", older, newer).latest_date, 900);
    }

    #[test]
    fn two_sent_only_threads_still_take_the_fresher() {
        let mut older = row("t1", "older", "me@golia.jp", "");
        older.count = 1;
        older.sent_count = 1;
        older.latest_date = 100;
        let mut newer = row("t2", "newer", "me@golia.jp", "");
        newer.count = 1;
        newer.sent_count = 1;
        newer.latest_date = 900;
        let merged = combine_rows("t1", older, newer);
        assert_eq!(merged.latest_date, 900, "a sent-only pair has no inbound to prefer");
        assert_eq!(merged.subject, "newer");
    }

    /// A row whose message entities were never indexed reads count=0.
    /// That is missing information, not proof of a sent-only thread.
    #[test]
    fn a_countless_row_is_not_treated_as_sent_only() {
        let mut unindexed = row("t1", "unindexed", "a@x.com", "");
        unindexed.count = 0;
        unindexed.sent_count = 0;
        unindexed.latest_date = 900;
        let mut inbound = row("t2", "inbound", "b@x.com", "");
        inbound.count = 2;
        inbound.sent_count = 0;
        inbound.latest_date = 100;
        assert_eq!(combine_rows("t1", unindexed, inbound).latest_date, 900);
    }

    #[test]
    fn finds_by_subject_sender_and_preview() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(
            u,
            &row("t1", "Release notes", "bot@github.com", "v9 is out"),
        )
        .unwrap();
        s.upsert_thread(u, &row("t2", "Lunch", "alice@x.com", "see you at noon"))
            .unwrap();

        // subject
        assert_eq!(s.search_threads(u, "release", 10).unwrap()[0].0, "t1");
        // sender — an explicit requirement, users search by who sent it
        assert_eq!(s.search_threads(u, "github", 10).unwrap()[0].0, "t1");
        // preview
        assert_eq!(s.search_threads(u, "noon", 10).unwrap()[0].0, "t2");
    }

    #[test]
    fn finds_japanese_without_a_tokenizer() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(
            u,
            &row("t1", "小柳ルミ子 誕生日", "アメマガ <sp@ameba.jp>", ""),
        )
        .unwrap();
        // CJK bigrams — the mailbox this was reported against is mostly
        // Japanese commercial mail
        assert_eq!(s.search_threads(u, "アメマガ", 10).unwrap().len(), 1);
        assert_eq!(s.search_threads(u, "誕生日", 10).unwrap().len(), 1);
    }

    #[test]
    fn never_returns_another_users_threads() {
        let s = store();
        s.upsert_thread("a@x.com", &row("ta", "shared word", "s@x.com", ""))
            .unwrap();
        s.upsert_thread("b@x.com", &row("tb", "shared word", "s@x.com", ""))
            .unwrap();

        let a = s.search_threads("a@x.com", "shared", 10).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "ta");
    }

    #[test]
    fn reflects_edits_without_a_reindex_step() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("t1", "before", "s@x.com", ""))
            .unwrap();
        assert_eq!(s.search_threads(u, "before", 10).unwrap().len(), 1);

        s.upsert_thread(u, &row("t1", "after", "s@x.com", ""))
            .unwrap();
        // the commit hook maintains the index — no pipeline to lag
        assert!(s.search_threads(u, "before", 10).unwrap().is_empty());
        assert_eq!(s.search_threads(u, "after", 10).unwrap().len(), 1);
    }

    #[test]
    fn finds_a_thread_by_words_only_in_the_body() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("t1", "Q3 planning", "alice@x.com", ""))
            .unwrap();
        s.index_message_text("m1@x", "t1", "the budget spreadsheet is attached")
            .unwrap();

        // subject/sender index knows nothing about "spreadsheet"
        assert!(s.search_threads(u, "spreadsheet", 10).unwrap().is_empty());
        // the body index does
        assert_eq!(
            s.search_message_bodies(u, "spreadsheet", 10).unwrap(),
            vec!["t1".to_string()]
        );
    }

    #[test]
    fn body_search_is_per_user_and_deduplicated() {
        let s = store();
        s.upsert_thread("a@x.com", &row("ta", "s", "p@x.com", ""))
            .unwrap();
        s.upsert_thread("b@x.com", &row("tb", "s", "p@x.com", ""))
            .unwrap();
        // two messages in the same thread both mention the term
        s.index_message_text("m1@x", "ta", "quarterly invoice")
            .unwrap();
        s.index_message_text("m2@x", "ta", "quarterly invoice again")
            .unwrap();
        s.index_message_text("m3@x", "tb", "quarterly invoice")
            .unwrap();

        let hits = s.search_message_bodies("a@x.com", "quarterly", 10).unwrap();
        assert_eq!(hits, vec!["ta".to_string()], "one row per thread, own only");
    }

    #[test]
    fn forgetting_a_message_removes_it_from_body_search() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("t1", "s", "p@x.com", "")).unwrap();
        s.index_message_text("m1@x", "t1", "confidential terms")
            .unwrap();
        assert_eq!(
            s.search_message_bodies(u, "confidential", 10)
                .unwrap()
                .len(),
            1
        );

        s.forget_message_text("m1@x").unwrap();
        assert!(
            s.search_message_bodies(u, "confidential", 10)
                .unwrap()
                .is_empty(),
            "a deleted message must not stay searchable"
        );
    }

    #[test]
    fn body_text_is_capped_on_a_char_boundary() {
        // Multi-byte input right at the cap must not panic or split a
        // char — the cap is a byte count, the content is UTF-8.
        let long: String = "日".repeat(crate::keys::MESSAGE_TEXT_CAP);
        let capped = crate::keys::cap_message_text(&long);
        assert!(capped.len() <= crate::keys::MESSAGE_TEXT_CAP);
        assert!(long.starts_with(capped));
    }

    #[test]
    fn empty_query_returns_nothing_rather_than_everything() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("t1", "x", "s@x.com", "")).unwrap();
        assert!(s.search_threads(u, "   ", 10).unwrap().is_empty());
    }
}
