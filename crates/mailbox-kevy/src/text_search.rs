//! Conversation search over the text indexes and the caller's own rows.

use std::io;

use crate::KevyMailboxStore;

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
    ///
    /// No thread id on the row: the message is shared by its recipients and
    /// each copy can sit in a different thread, which a merge or split then
    /// moves. A hit resolves through the caller's own message row instead.
    pub fn index_message_text(&self, message_id: &str, body_text: &str) -> io::Result<()> {
        let key = crate::keys::message_text(message_id);
        let text = crate::keys::cap_message_text(body_text.trim());
        if text.is_empty() {
            self.store().del(&[key.as_bytes()])?;
            return Ok(());
        }
        self.store().hset(
            key.as_bytes(),
            &[(crate::keys::MESSAGE_TEXT_FIELD, text.as_bytes())],
        )?;
        Ok(())
    }

    /// Thread ids whose message bodies match `query`, best first and
    /// de-duplicated, resolved like `Self::thread_of_message_hit`.
    pub fn search_message_bodies(
        &self,
        user: &str,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.search_message_rows(
            crate::keys::IDX_MESSAGE_TEXT,
            crate::keys::MSGTEXT_PREFIX,
            false,
            user,
            query,
            limit,
        )
    }

    /// A text index over per-message rows, answered as the caller's thread
    /// ids. `prefix` is the rows' key prefix; what follows it is the message
    /// id, then `:{n}` when `numbered`.
    pub(crate) fn search_message_rows(
        &self,
        index: &[u8],
        prefix: &[u8],
        numbered: bool,
        user: &str,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        const OVERFETCH: usize = 8;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let hits =
            self.store()
                .idx_match(index, query.as_bytes(), limit.saturating_mul(OVERFETCH))?;
        let mut out: Vec<String> = Vec::with_capacity(limit);
        for (key, _score) in hits {
            let Some(rest) = key.strip_prefix(prefix) else {
                continue;
            };
            let Ok(rest) = std::str::from_utf8(rest) else {
                continue;
            };
            let mid = match numbered {
                true => rest.rsplit_once(':').map_or(rest, |(mid, _)| mid),
                false => rest,
            };
            let Some(tid) = self.thread_of_message_hit(user, mid)? else {
                continue;
            };
            if out.contains(&tid) {
                continue;
            }
            out.push(tid);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// The caller's thread for a message a text index matched, or `None`
    /// when the caller has no copy of it or no longer holds that thread.
    ///
    /// Through the caller's own message row, which exists only if they
    /// received the message and follows it through a merge or split — not
    /// through a thread id stored on the indexed row, which is one
    /// recipient's and goes stale when the message moves.
    pub(crate) fn thread_of_message_hit(
        &self,
        user: &str,
        mid: &str,
    ) -> io::Result<Option<String>> {
        let Some(own) = self.user_message_facts(user, mid)? else {
            return Ok(None);
        };
        if own.tid.is_empty() {
            return Ok(None);
        }
        let member = crate::keys::thread_user(user, &own.tid);
        if self.store().exists(&[member.as_bytes()])? == 0 {
            return Ok(None);
        }
        Ok(Some(own.tid))
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
    use crate::KevyMailboxStore;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    const U: &str = "u@x.com";

    fn store() -> KevyMailboxStore {
        KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()))
    }

    /// A message `mid` in `user`'s thread `tid`, the way delivery leaves it.
    fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, date: i64) {
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user,
            subject: "s",
            senders_csv: "a@x.com",
            latest_date: date,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
        let wire = serde_json::json!({
            "message_id": mid, "thread_id": tid, "subject": "s",
            "sender": "a@x.com", "internal_date": date, "flags": 0,
        });
        s.upsert_user_message(
            user,
            tid,
            mid,
            date,
            wire.to_string().as_bytes(),
            &crate::UserMessageFacts {
                blob_ref: mid,
                uid: date as u32,
                flags: 0,
                modseq: 0,
            },
        )
        .unwrap();
    }

    #[test]
    fn finds_a_thread_by_words_only_in_the_body() {
        let s = store();
        deliver(&s, U, "t1", "m1@x", 1);
        s.index_message_text("m1@x", "the budget spreadsheet is attached")
            .unwrap();
        assert!(s.search_threads(U, "spreadsheet", 10).unwrap().is_empty());
        assert_eq!(
            s.search_message_bodies(U, "spreadsheet", 10).unwrap(),
            vec!["t1".to_string()]
        );
    }

    #[test]
    fn body_search_is_per_user_and_deduplicated() {
        let s = store();
        deliver(&s, "a@x.com", "ta", "m1@x", 1);
        deliver(&s, "a@x.com", "ta", "m2@x", 2);
        deliver(&s, "b@x.com", "tb", "m3@x", 3);
        s.index_message_text("m1@x", "quarterly invoice").unwrap();
        s.index_message_text("m2@x", "quarterly invoice again")
            .unwrap();
        s.index_message_text("m3@x", "quarterly invoice").unwrap();
        let hits = s.search_message_bodies("a@x.com", "quarterly", 10).unwrap();
        assert_eq!(hits, vec!["ta".to_string()], "one row per thread, own only");
    }

    /// The indexed row carries no thread, so a merge or a split cannot
    /// leave it pointing at the conversation the message left.
    #[test]
    fn body_search_follows_the_message_through_a_merge_and_a_split() {
        let s = store();
        deliver(&s, U, "t1", "m1@x", 1);
        deliver(&s, U, "t2", "m2@x", 2);
        s.index_message_text("m2@x", "walrus migration notes")
            .unwrap();
        assert_eq!(
            s.search_message_bodies(U, "walrus", 10).unwrap(),
            vec!["t2".to_string()]
        );

        s.merge_thread_into(U, "t2", "t1").unwrap();
        assert_eq!(
            s.search_message_bodies(U, "walrus", 10).unwrap(),
            vec!["t1".to_string()],
            "a merged message is found in the thread it moved to"
        );

        let new_tid = s.split_message_to_new_thread(U, "m2@x").unwrap().unwrap();
        assert_eq!(
            s.search_message_bodies(U, "walrus", 10).unwrap(),
            vec![new_tid],
            "a split message is found in its own thread"
        );
    }

    #[test]
    fn deleting_the_thread_removes_its_body_rows_and_its_hits() {
        let s = store();
        deliver(&s, U, "t1", "m1@x", 1);
        s.index_message_text("m1@x", "confidential terms").unwrap();
        assert_eq!(
            s.search_message_bodies(U, "confidential", 10)
                .unwrap()
                .len(),
            1
        );

        assert!(s.delete_thread(U, "t1").unwrap().0);
        let key = crate::keys::message_text("m1@x");
        assert_eq!(s.store().exists(&[key.as_bytes()]).unwrap(), 0);
        assert!(
            s.search_message_bodies(U, "confidential", 10)
                .unwrap()
                .is_empty(),
            "a deleted message must not stay searchable"
        );
    }
}
