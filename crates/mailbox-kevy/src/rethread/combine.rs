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
    // the fresher side supplies the display fields
    let (latest, older) = if a.latest_date >= b.latest_date {
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

#[cfg(test)]
mod search_tests {
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
