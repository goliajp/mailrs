//! Thread-merge + `Message-ID → thread_id` index (v2.9.5 threading fix).
//!
//! Threading fragmented because the write paths derived thread ids from
//! three inconsistent rules and nothing recorded which thread a given
//! RFC 5322 `Message-ID:` landed in — a reply carrying `In-Reply-To:
//! <our-msgid>` could not find its conversation and opened a new one.
//! This module adds the reconciliation index and the merge primitive the
//! rethread backfill uses to heal existing fragments.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// Record which thread a `Message-ID:` belongs to. Overwrites — the
    /// latest write wins, which is correct after a merge re-points ids.
    pub fn set_thread_for_message_id(&self, user: &str, mid: &str, tid: &str) -> io::Result<()> {
        let key = keys::message_by_message_id(user, mid);
        self.store().set(key.as_bytes(), tid.as_bytes())?;
        Ok(())
    }

    /// Resolve a `Message-ID:` to the thread it was indexed under.
    pub fn thread_for_message_id(&self, user: &str, mid: &str) -> io::Result<Option<String>> {
        let key = keys::message_by_message_id(user, mid);
        let out = self.store().get(key.as_bytes())?;
        Ok(out.and_then(|v| String::from_utf8(v).ok()))
    }

    /// Merge thread `from` into thread `into` for `user`: move every
    /// message (zset member + blob thread_id field), combine the two
    /// aggregates, rebuild `into`'s index memberships, and drop `from`
    /// from every index. Returns how many messages moved. Idempotent —
    /// re-running with an already-merged `from` is a no-op.
    pub fn merge_thread_into(&self, user: &str, from: &str, into: &str) -> io::Result<usize> {
        if from == into {
            return Ok(0);
        }
        let store = self.store();
        let from_msgs_key = keys::thread_messages(from);
        let into_msgs_key = keys::thread_messages(into);

        // 1. move messages: re-point each blob's thread_id and move the
        //    zset membership (keeping the internal_date score).
        let members = store.zrange(from_msgs_key.as_bytes(), 0, -1)?;
        let mut moved = 0usize;
        for (mid_bytes, score) in &members {
            let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                continue;
            };
            let blob_key = keys::message_blob(mid);
            if let Some(bytes) = store.get(blob_key.as_bytes())?
                && let Ok(mut wire) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                wire["thread_id"] = serde_json::Value::String(into.to_string());
                if let Ok(payload) = serde_json::to_vec(&wire) {
                    store.set(blob_key.as_bytes(), &payload)?;
                }
            }
            store.zadd(into_msgs_key.as_bytes(), &[(*score, mid_bytes.as_slice())])?;
            self.set_thread_for_message_id(user, mid, into)?;
            moved += 1;
        }
        store.del(&[from_msgs_key.as_bytes()])?;

        // 2. combine the aggregates. `from` may have no hash (already
        //    merged) — then only the zset move above mattered.
        let from_row = self.get_thread(from)?;
        let into_row = self.get_thread(into)?;
        let merged = match (into_row, from_row) {
            (Some(a), Some(b)) => Some(combine_rows(into, a, b)),
            (Some(a), None) => Some(a),
            (None, Some(mut b)) => {
                b.thread_id = into.to_string();
                Some(b)
            }
            (None, None) => None,
        };

        // 3. drop `from` from every per-user index + its hash, then
        //    rebuild `into`'s memberships from the merged aggregate —
        //    with the counters recounted from the actual message flags.
        //    Naively summing the two hashes' unread_count resurrected
        //    stale pre-migration values that were never in the unread
        //    index (2026-07-17: years-old mail flooded the Unread tab
        //    after the first prod rethread).
        // Merge — messages have already been re-pointed to `into`;
        // dropping `from` here only removes empty scaffolding, no
        // maildir files should exist for it anymore. Discard the
        // returned blob_refs deliberately.
        let (_existed, _blob_refs) = self.delete_thread(user, from)?;
        if let Some(mut row) = merged {
            if let Some((count, unread, sent)) = self.recount_from_messages(user, into)? {
                row.count = count;
                row.unread_count = unread;
                row.sent_count = sent;
            }
            self.upsert_thread(user, &row)?;
        }
        Ok(moved)
    }

    /// Move a single message out of its current thread into a fresh
    /// thread keyed by its own Message-ID (Gmail's subject-change rule:
    /// a reply that changes topic is a new conversation). Both threads'
    /// aggregates are recounted. Returns the new thread_id, or None when
    /// the message isn't found.
    pub fn split_message_to_new_thread(&self, user: &str, mid: &str) -> io::Result<Option<String>> {
        let store = self.store();
        let blob_key = keys::message_blob(mid);
        let Some(bytes) = store.get(blob_key.as_bytes())? else {
            return Ok(None);
        };
        let Ok(mut wire) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Ok(None);
        };
        let old_tid = wire["thread_id"].as_str().unwrap_or("").to_string();
        let new_tid = mid.to_string();
        if old_tid == new_tid {
            return Ok(Some(new_tid));
        }
        let date = wire["internal_date"].as_i64().unwrap_or(0);
        let subject = wire["subject"].as_str().unwrap_or("").to_string();
        let sender = wire["sender"].as_str().unwrap_or("").to_string();
        let seen = wire["flags"].as_u64().unwrap_or(0) & 1 != 0;
        wire["thread_id"] = serde_json::Value::String(new_tid.clone());
        if let Ok(payload) = serde_json::to_vec(&wire) {
            store.set(blob_key.as_bytes(), &payload)?;
        }
        // move the zset membership
        let old_msgs = keys::thread_messages(&old_tid);
        let new_msgs = keys::thread_messages(&new_tid);
        store.zrem(old_msgs.as_bytes(), &[mid.as_bytes()])?;
        store.zadd(new_msgs.as_bytes(), &[(date as f64, mid.as_bytes())])?;
        self.set_thread_for_message_id(user, mid, &new_tid)?;
        // rebuild the old thread's aggregate (may now be smaller)
        if !old_tid.is_empty()
            && let Some(mut row) = self.get_thread(&old_tid)?
        {
            if let Some((count, unread, sent)) = self.recount_from_messages(user, &old_tid)? {
                row.count = count;
                row.unread_count = unread;
                row.sent_count = sent;
                // display fields follow the (new) latest message
                if let Some(last) = self.list_thread_messages(&old_tid)?.last()
                    && let Ok(w) = serde_json::from_slice::<serde_json::Value>(last)
                {
                    row.latest_date = w["internal_date"].as_i64().unwrap_or(row.latest_date);
                    row.subject = w["subject"].as_str().unwrap_or(&row.subject).to_string();
                }
                self.upsert_thread(user, &row)?;
            } else {
                // no messages left — drop the empty thread entirely.
                // Split moved every message away already, so no maildir
                // files remain on this side.
                let (_existed, _blob_refs) = self.delete_thread(user, &old_tid)?;
            }
        }
        // create the new thread's aggregate
        let is_own = crate::thread_row::senders_csv_contains_user(&sender, user);
        let row = crate::thread_row::ThreadRow {
            thread_id: new_tid.clone(),
            subject,
            senders_csv: sender,
            count: 1,
            unread_count: if !is_own && !seen { 1 } else { 0 },
            latest_date: date,
            latest_preview: String::new(),
            category: "inbox".to_string(),
            importance_level: String::new(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: if is_own { 1 } else { 0 },
            starred: false,
        };
        self.upsert_thread(user, &row)?;
        Ok(Some(new_tid))
    }

    /// Recompute (count, unread_count, sent_count) for a thread from its
    /// per-message wires: unread = messages without the \Seen flag that
    /// the user didn't send; sent = messages the user sent. `None` when
    /// the thread has no messages to count (keep the hash values).
    fn recount_from_messages(&self, user: &str, tid: &str) -> io::Result<Option<(i64, i64, i64)>> {
        let blobs = self.list_thread_messages(tid)?;
        if blobs.is_empty() {
            return Ok(None);
        }
        let mut count = 0i64;
        let mut unread = 0i64;
        let mut sent = 0i64;
        for b in &blobs {
            let Ok(w) = serde_json::from_slice::<serde_json::Value>(b) else {
                continue;
            };
            count += 1;
            let seen = w["flags"].as_u64().unwrap_or(0) & 1 != 0;
            let sender = w["sender"].as_str().unwrap_or("");
            let is_own = crate::thread_row::senders_csv_contains_user(sender, user);
            if is_own {
                sent += 1;
            } else if !seen {
                unread += 1;
            }
        }
        Ok(Some((count, unread, sent)))
    }
}

fn combine_rows(
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    fn arrive(s: &KevyMailboxStore, tid: &str, user: &str, subject: &str, date: i64) {
        s.record_message_arrival(&MessageArrival {
            thread_id: tid,
            user,
            subject,
            senders_csv: "other@x.com",
            latest_date: date,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }

    fn put_msg(s: &KevyMailboxStore, tid: &str, mid: &str, date: i64) {
        let wire = serde_json::json!({"message_id": mid, "thread_id": tid, "internal_date": date});
        s.upsert_message(tid, mid, date, &serde_json::to_vec(&wire).unwrap())
            .unwrap();
    }

    #[test]
    fn msgid_index_roundtrip() {
        let s = store();
        assert_eq!(s.thread_for_message_id("u@x", "m1").unwrap(), None);
        s.set_thread_for_message_id("u@x", "m1", "t1").unwrap();
        assert_eq!(
            s.thread_for_message_id("u@x", "m1").unwrap(),
            Some("t1".into())
        );
    }

    #[test]
    fn merge_moves_messages_and_combines_counts() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t-old", u, "Re: hello", 100);
        arrive(&s, "t-new", u, "Re: hello", 200);
        put_msg(&s, "t-old", "m1", 100);
        put_msg(&s, "t-new", "m2", 200);

        let moved = s.merge_thread_into(u, "t-old", "t-new").unwrap();
        assert_eq!(moved, 1);

        // t-old gone, t-new has both messages and the summed counts
        assert!(s.get_thread("t-old").unwrap().is_none());
        let row = s.get_thread("t-new").unwrap().expect("merged row");
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 2);
        let msgs = s.list_thread_messages("t-new").unwrap();
        assert_eq!(msgs.len(), 2);
        // moved blob got its thread_id re-pointed
        let m1: serde_json::Value = serde_json::from_slice(&msgs[0]).unwrap();
        assert_eq!(m1["thread_id"], "t-new");
        // msgid index points at the canonical thread
        assert_eq!(
            s.thread_for_message_id(u, "m1").unwrap(),
            Some("t-new".into())
        );
    }

    #[test]
    fn merge_recounts_unread_from_flags_not_stale_hash() {
        let s = store();
        let u = "u@x.com";
        // both fragments carry stale unread_count=1 in the hash, but the
        // actual messages are all \Seen — the merged thread must NOT
        // resurrect the stale unread.
        arrive(&s, "t-old", u, "a", 100);
        arrive(&s, "t-new", u, "a", 200);
        let seen1 = serde_json::json!({"message_id":"m1","thread_id":"t-old","internal_date":100,"flags":1,"sender":"other@x.com"});
        let seen2 = serde_json::json!({"message_id":"m2","thread_id":"t-new","internal_date":200,"flags":1,"sender":"other@x.com"});
        s.upsert_message("t-old", "m1", 100, &serde_json::to_vec(&seen1).unwrap())
            .unwrap();
        s.upsert_message("t-new", "m2", 200, &serde_json::to_vec(&seen2).unwrap())
            .unwrap();

        s.merge_thread_into(u, "t-old", "t-new").unwrap();

        let row = s.get_thread("t-new").unwrap().unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 0);
    }

    #[test]
    fn merge_is_idempotent() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t-old", u, "a", 100);
        arrive(&s, "t-new", u, "a", 200);
        put_msg(&s, "t-old", "m1", 100);
        put_msg(&s, "t-new", "m2", 200);
        s.merge_thread_into(u, "t-old", "t-new").unwrap();
        let again = s.merge_thread_into(u, "t-old", "t-new").unwrap();
        assert_eq!(again, 0);
        let row = s.get_thread("t-new").unwrap().unwrap();
        assert_eq!(row.count, 2);
    }
}

impl KevyMailboxStore {
    /// Full-text search the caller's threads.
    ///
    /// The text index spans every thread key regardless of owner (kevy
    /// indexes are declared over a key prefix and thread rows carry no
    /// owner field), so hits are filtered against the caller's activity
    /// zset afterwards. Over-fetches by `OVERFETCH` to leave room for
    /// hits belonging to other accounts.
    pub fn search_threads(
        &self,
        user: &str,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<(String, f64)>> {
        const OVERFETCH: usize = 8;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let hits = self.store().idx_match(
            crate::keys::IDX_THREAD_SEARCH,
            query.as_bytes(),
            limit.saturating_mul(OVERFETCH),
        )?;
        let activity = crate::keys::user_threads_by_activity(user);
        let mut out = Vec::with_capacity(limit);
        for (key, score) in hits {
            let Ok(key) = String::from_utf8(key) else {
                continue;
            };
            let Some(tid) = key.strip_prefix("mailrs:thread:") else {
                continue;
            };
            // ownership check — the index is global, the answer is not
            if self
                .store()
                .zscore(activity.as_bytes(), tid.as_bytes())?
                .is_none()
            {
                continue;
            }
            out.push((tid.to_string(), score));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
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

impl KevyMailboxStore {
    /// Store a message's body text for full-text search, capped by
    /// [`crate::keys::MESSAGE_TEXT_CAP`]. Empty text removes the row so
    /// the index doesn't retain a stale body.
    pub fn index_message_text(
        &self,
        message_id: &str,
        thread_id: &str,
        body_text: &str,
    ) -> io::Result<()> {
        let key = crate::keys::message_text(message_id);
        let text = crate::keys::cap_message_text(body_text.trim());
        if text.is_empty() {
            self.store().del(&[key.as_bytes()])?;
            return Ok(());
        }
        self.store().hset(
            key.as_bytes(),
            &[
                (crate::keys::MESSAGE_TEXT_FIELD, text.as_bytes()),
                (crate::keys::MESSAGE_TEXT_TID_FIELD, thread_id.as_bytes()),
            ],
        )?;
        Ok(())
    }

    /// Drop a message's indexed body — call alongside message deletion
    /// so search can't surface mail that no longer exists.
    pub fn forget_message_text(&self, message_id: &str) -> io::Result<()> {
        self.store()
            .del(&[crate::keys::message_text(message_id).as_bytes()])?;
        Ok(())
    }

    /// Thread ids whose message bodies match `query`, best first and
    /// de-duplicated. Ownership is enforced against the caller's
    /// activity zset, same as [`Self::search_threads`].
    pub fn search_message_bodies(
        &self,
        user: &str,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        const OVERFETCH: usize = 8;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let hits = self.store().idx_match(
            crate::keys::IDX_MESSAGE_TEXT,
            query.as_bytes(),
            limit.saturating_mul(OVERFETCH),
        )?;
        let activity = crate::keys::user_threads_by_activity(user);
        let mut out: Vec<String> = Vec::with_capacity(limit);
        for (key, _score) in hits {
            let Some(tid) = self
                .store()
                .hget(&key, crate::keys::MESSAGE_TEXT_TID_FIELD)?
                .and_then(|v| String::from_utf8(v).ok())
            else {
                continue;
            };
            if out.contains(&tid) {
                continue;
            }
            if self
                .store()
                .zscore(activity.as_bytes(), tid.as_bytes())?
                .is_none()
            {
                continue;
            }
            out.push(tid);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
