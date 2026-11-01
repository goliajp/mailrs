//! A sender's conversations: keyed on the address the newest message
//! from someone else came from, filtered to the reader.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// The ids of `user`'s conversations whose newest message from someone
    /// else came from `sender` (compared lowercased), newest first.
    pub fn sender_thread_ids(&self, user: &str, sender: &str) -> io::Result<Vec<String>> {
        use kevy_embedded::{IndexValue, ScalarQueryOpts, ValueFilter};
        let filters = [ValueFilter::Eq {
            field: b"user",
            value: user.as_bytes(),
        }];
        let key = IndexValue::Str(sender.trim().to_ascii_lowercase().into_bytes());
        let prefix_len = keys::thread_user(user, "").len();
        let mut out = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.store.idx_query_claused(
                b"threaduser.sec_in_from",
                &key,
                &key,
                None,
                1000,
                ScalarQueryOpts {
                    filters: &filters,
                    sort: Some((b"activity", true)),
                    distinct: None,
                    facets: &[],
                    offset,
                },
            )?;
            let n = page.rows.len();
            out.extend(page.rows.into_iter().filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            }));
            if n < 1000 {
                return Ok(out);
            }
            offset += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use crate::thread_row::ThreadRow;
    use crate::{KevyMailboxStore, SectionArrival};

    fn row(tid: &str, at: i64) -> ThreadRow {
        ThreadRow {
            account_id: String::new(),
            thread_id: tid.into(),
            subject: "s".into(),
            senders_csv: "news@shop.example".into(),
            count: 1,
            unread_count: 0,
            latest_date: at,
            latest_preview: String::new(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            quarantined: false,
            has_action: false,
            sent_count: 0,
            starred: false,
            snoozed_until: 0,
            trashed_at: 0,
        }
    }

    #[test]
    fn a_senders_conversations_are_found_for_this_reader_only() {
        let st = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        st.ensure_thread_table();
        for (user, tid, at, from) in [
            ("me@x", "t1", 100, "news@shop.example"),
            ("me@x", "t2", 200, "news@shop.example"),
            ("me@x", "t3", 300, "friend@x.example"),
            ("you@x", "t4", 400, "news@shop.example"),
        ] {
            st.upsert_thread(user, &row(tid, at)).unwrap();
            st.record_section_arrival(&SectionArrival {
                user,
                thread_id: tid,
                at,
                is_own: false,
                facts: Default::default(),
                from_addr: from,
            })
            .unwrap();
        }
        assert_eq!(
            st.sender_thread_ids("me@x", "News@Shop.example").unwrap(),
            vec!["t2", "t1"]
        );
        assert!(st.sender_thread_ids("me@x", "nobody@x").unwrap().is_empty());
    }
}
