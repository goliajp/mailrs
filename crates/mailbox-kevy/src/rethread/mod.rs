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

mod combine;

pub(crate) use combine::*;

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

        // The same move on this user's own index. It reads from a
        // different key, and moving only the shared one left the merged
        // messages out of the mailbox that owns them: the reading pane
        // lists a thread from `thread_user_messages`, so a merged
        // conversation showed only the half that was already in the target
        // — and the recount, which counts what the user holds, agreed with
        // the pane and disagreed with the thread. `from`'s key is removed
        // by `delete_thread` below.
        let from_user_key = keys::thread_user_messages(user, from);
        let into_user_key = keys::thread_user_messages(user, into);
        for (mid_bytes, score) in &store.zrange(from_user_key.as_bytes(), 0, -1)? {
            store.zadd(into_user_key.as_bytes(), &[(*score, mid_bytes.as_slice())])?;
        }

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
                // Display fields follow the (new) latest INBOUND
                // message. Taking the last message outright let the
                // user's own send re-date and re-title the row, which
                // is the same defect `record_message_arrival` and the
                // maildir self-heal each guard against — a rule worth
                // stating once, in `thread_row::display_message`.
                let msgs = self.thread_messages_unscoped(&old_tid)?;
                if let Some(w) = crate::thread_row::display_message(&msgs, user) {
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
            snoozed_until: 0,
        };
        self.upsert_thread(user, &row)?;
        Ok(Some(new_tid))
    }

    /// Recompute (count, unread_count, sent_count) for a thread from its
    /// per-message wires: unread = messages without the \Seen flag that
    /// the user didn't send; sent = messages the user sent. `None` when
    /// the thread has no messages to count (keep the hash values).
    pub(crate) fn recount_from_messages(
        &self,
        user: &str,
        tid: &str,
    ) -> io::Result<Option<(i64, i64, i64)>> {
        // This user's messages, not the thread's. A thread is shared and a
        // mailbox is not: `list_thread_messages` states the rule — "a
        // message the user has no copy of is not in their index and so not
        // in their thread, which is what a mailbox means" — and counting
        // the shared index broke it, most visibly on a 28-message thread
        // where the 28th belonged to another mailbox and was counted as
        // this user's unread mail.
        let mids = self.user_thread_message_ids(user, tid)?;
        if mids.is_empty() {
            return Ok(None);
        }
        let mut count = 0i64;
        let mut unread = 0i64;
        let mut sent = 0i64;
        for mid in &mids {
            // Whether it has been read is a per-user fact, and the shared
            // blob's `flags` is stripped to zero on every write precisely
            // because it is shared. Reading it there counted every message
            // unread and repaired read threads back to unread — the
            // resurrection this sweep exists to prevent, performed by the
            // sweep.
            let Some(facts) = self.user_message_facts(user, mid)? else {
                // An id in this user's index with no row behind it: not a
                // copy, so not counted. Skipped rather than guessed at.
                continue;
            };
            let Some(bytes) = self.get_message(mid)? else {
                continue;
            };
            let Ok(w) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            count += 1;
            let seen = facts.flags & 1 != 0;
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

impl KevyMailboxStore {
    /// Full-text search the caller's threads.
    ///
    /// The text index spans every thread key regardless of owner (kevy
    /// indexes are declared over a key prefix and thread rows carry no
    /// owner field), so hits are filtered against the caller's membership
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
        let mut out = Vec::with_capacity(limit);
        for (key, score) in hits {
            let Ok(key) = String::from_utf8(key) else {
                continue;
            };
            let Some(tid) = key.strip_prefix("mailrs:thread:") else {
                continue;
            };
            // ownership check — the index is global, the answer is not
            let owned = crate::keys::thread_user(user, tid);
            if self.store().exists(&[owned.as_bytes()])? == 0 {
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
    /// membership rows, same as [`Self::search_threads`].
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
            let owned = crate::keys::thread_user(user, &tid);
            if self.store().exists(&[owned.as_bytes()])? == 0 {
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

impl KevyMailboxStore {
    /// Substring search over the caller's own rows — what a user means
    /// by "just find the ones containing this".
    ///
    /// The token index cannot answer it. `kevy_text::tokenize` splits
    /// latin text on word boundaries and CJK into bigrams, so `qualcomm`
    /// is one token and `ualcom` is none of it; the query grammar offers
    /// `word*` but has no infix form. This walks the membership rows
    /// instead and asks each for `contains`.
    ///
    /// The haystack is the row's own `subject`, `senders_csv` and
    /// `latest_preview` — **not** message bodies. Bodies are capped at
    /// 8 KiB each and there are tens of thousands of them; scanning that
    /// per keystroke is not a search, it is a table scan wearing one.
    /// Bodies stay on the token index, which is where BM25 earns its
    /// keep anyway.
    ///
    /// Walks newest-first and stops at `limit` hits, so a common
    /// substring costs a short walk and a rare one costs the whole
    /// index — bounded either way by the user's own thread count.
    /// `skip` holds the thread ids an earlier stage already returned:
    /// every token match is also a substring match, so without it the
    /// entire first stage would repeat here.
    pub fn like_threads(
        &self,
        user: &str,
        query: &str,
        skip: &std::collections::HashSet<String>,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let needle = query.trim().to_lowercase();
        // Two characters is the floor the tokenizer itself uses
        // (`flush_word` drops shorter words), so anything below it would
        // be a substring scan for something the index would never have
        // indexed — and, on a mailbox this size, a match on nearly
        // everything.
        if needle.chars().count() < 2 || limit == 0 {
            return Ok(Vec::new());
        }
        // Candidates, not results: `in_search_scope` decides which tab a
        // hit belongs to, and it compares `row.archived` against the tab
        // the search came from. Excluding archived here would make a
        // search inside Archived able to match nothing.
        let tids = self.list_thread_ids_by_activity_via_table(
            user,
            crate::ArchiveScope::All,
            usize::MAX,
            None,
        )?;
        let mut out = Vec::with_capacity(limit);
        for tid in tids {
            if skip.contains(&tid) {
                continue;
            }
            let Some(row) = self.get_thread_for_user(user, &tid)? else {
                continue;
            };
            let hit = row.subject.to_lowercase().contains(&needle)
                || row.senders_csv.to_lowercase().contains(&needle)
                || row.latest_preview.to_lowercase().contains(&needle);
            if hit {
                out.push(tid);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        s
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
        let msgs = s.thread_messages_for_maintenance("t-new").unwrap();
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
        // Seeded as delivery does it — the shared blob *and* this user's
        // row. The counters are recounted from the rows, so a
        // shared-half-only seed would be testing a state no delivery can
        // produce.
        for (tid, mid, date) in [("t-old", "m1", 100i64), ("t-new", "m2", 200)] {
            let w = serde_json::json!({
                "message_id": mid, "thread_id": tid, "internal_date": date,
                "flags": 1, "sender": "other@x.com",
            });
            s.upsert_user_message(
                u,
                tid,
                mid,
                date,
                &serde_json::to_vec(&w).unwrap(),
                &crate::UserMessageFacts {
                    blob_ref: mid,
                    uid: 1,
                    flags: 1,
                    modseq: 1,
                },
            )
            .unwrap();
        }

        s.merge_thread_into(u, "t-old", "t-new").unwrap();

        let row = s.get_thread("t-new").unwrap().unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 0);
        // And the merged message is in the mailbox that owns it, not only
        // in the shared index — the reading pane lists a thread from the
        // per-user index, so a merge that moves one and not the other
        // shows half a conversation.
        assert_eq!(
            s.list_thread_messages(u, "t-new").unwrap().len(),
            2,
            "the merged-in message is missing from this user's thread"
        );
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

    /// A token match is a substring match too. If the LIKE stage did not
    /// skip what the token stage already returned, every exact hit would
    /// appear twice — once ranked, once again at the bottom.
    #[test]
    fn like_skips_what_the_token_stage_already_returned() {
        let s = store();
        let u = "alice@x.com";
        arrive(&s, "t1", u, "Qualcomm invoice", 300);
        arrive(&s, "t2", u, "about qualcomm systems", 200);

        let matched: Vec<String> = s
            .search_threads(u, "qualcomm", 10)
            .unwrap()
            .into_iter()
            .map(|(tid, _)| tid)
            .collect();
        assert!(!matched.is_empty(), "the token index must find these");

        let seen: std::collections::HashSet<String> = matched.iter().cloned().collect();
        let liked = s.like_threads(u, "qualcomm", &seen, 10).unwrap();
        assert!(
            liked.is_empty(),
            "every token hit is also a substring hit; the skip set is what stops the repeat"
        );
    }

    /// The reason LIKE exists: a fragment that is not a token.
    #[test]
    fn like_finds_a_fragment_the_token_index_cannot() {
        let s = store();
        let u = "alice@x.com";
        arrive(&s, "t1", u, "Qualcomm invoice", 300);

        let empty = std::collections::HashSet::new();
        assert!(
            s.search_threads(u, "ualcom", 10).unwrap().is_empty(),
            "a mid-word fragment is not a token, so BM25 cannot see it"
        );
        assert_eq!(
            s.like_threads(u, "ualcom", &empty, 10).unwrap(),
            vec!["t1".to_string()],
            "which is exactly what the substring stage is for"
        );
    }

    /// Newest first, so a common fragment returns the recent threads
    /// rather than whichever ones the walk reached first.
    #[test]
    fn like_returns_newest_first() {
        let s = store();
        let u = "alice@x.com";
        arrive(&s, "old", u, "invoice alpha", 100);
        arrive(&s, "mid", u, "invoice beta", 200);
        arrive(&s, "new", u, "invoice gamma", 300);

        let empty = std::collections::HashSet::new();
        assert_eq!(
            s.like_threads(u, "nvoic", &empty, 10).unwrap(),
            vec!["new".to_string(), "mid".to_string(), "old".to_string()]
        );
    }

    /// One character is below the tokenizer's own floor, and on a real
    /// mailbox it matches nearly everything. Refusing it is the answer,
    /// not returning half the index.
    #[test]
    fn like_needs_two_characters() {
        let s = store();
        let u = "alice@x.com";
        arrive(&s, "t1", u, "Qualcomm invoice", 300);

        let empty = std::collections::HashSet::new();
        assert!(s.like_threads(u, "q", &empty, 10).unwrap().is_empty());
        assert!(s.like_threads(u, " q ", &empty, 10).unwrap().is_empty());
        assert_eq!(s.like_threads(u, "qu", &empty, 10).unwrap().len(), 1);
    }

    /// The haystack is the row's own subject, senders and preview — and
    /// the row is per-user, so one owner's search cannot reach another's
    /// threads.
    #[test]
    fn like_searches_senders_and_preview_and_only_the_callers_rows() {
        let s = store();
        s.record_message_arrival(&MessageArrival {
            thread_id: "t1",
            user: "alice@x.com",
            subject: "nothing useful",
            senders_csv: "billing@qualcomm.com",
            latest_date: 100,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();

        let empty = std::collections::HashSet::new();
        assert_eq!(
            s.like_threads("alice@x.com", "ualcomm.co", &empty, 10)
                .unwrap(),
            vec!["t1".to_string()],
            "senders are part of the haystack"
        );
        assert!(
            s.like_threads("bob@x.com", "ualcomm.co", &empty, 10)
                .unwrap()
                .is_empty(),
            "bob has no row for this thread"
        );
    }
}
