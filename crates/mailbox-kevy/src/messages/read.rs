//! Reads that are not per-user: the shared blob and the thread listing.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Read message bytes for `message_id`. Returns `None` if the key
    /// is missing (deleted or never written).
    pub fn get_message(&self, message_id: &str) -> io::Result<Option<Vec<u8>>> {
        let key = keys::message_blob(message_id);
        self.store()
            .get(key.as_bytes())
            .map_err(std::io::Error::from)
    }

    /// List all messages in `thread_id` in chronological order
    /// (lowest internal_date first).
    ///
    /// v2 Stage B.3: N × get is amortized in one atomic closure —
    /// the initial zrange runs outside (AtomicCtx has no zset reads
    /// in kevy 3.17), then the get fanout serializes under a single
    /// shard lock so callers observe a consistent snapshot for the
    /// per-message payloads.
    /// Returns an empty list for a thread `user` has no copy of.
    ///
    /// The message index is keyed by thread alone, so reading it
    /// without asking who wants it hands any account the contents of
    /// any thread it can name — and a thread id is the root message's
    /// Message-ID, which every correspondent in the thread already
    /// knows and which is enumerable for machine senders
    /// (`post-208490793@substack.com`,
    /// `goliajp/devops/check-suites/CS_.../178533` are both real
    /// examples from prod). The pg lane has taken `user` since it was
    /// written (`mailrs-mailbox/src/pg/thread_ops/mod.rs:92`); this
    /// lane dropped it during the fastcore rewrite and nothing failed,
    /// because no test asked whether a stranger could read the thread.
    ///
    /// Membership is the per-user row: present for every thread a user
    /// has a copy of, and 30,510 of 30,510 on prod at the time of
    /// writing (`threaduser-census`).
    /// **Stage 4**: the messages come from this user's own index, and the
    /// per-user facts are overlaid on the shared blob.
    ///
    /// Before this the messages came from `thread:{tid}:messages`, which is
    /// shared, so every owner of a thread was served every message in it
    /// whoever it was delivered to — and each was served one owner's
    /// `blob_ref`, `uid` and `flags`. On production that meant 74 messages
    /// whose maildir file sat in somebody else's mailbox: measured, none of
    /// those 74 resolved for the user reading them and all 74 resolve from
    /// the per-user row.
    ///
    /// A message the user has no copy of is not in their index and so not in
    /// their thread, which is what a mailbox means.
    pub fn list_thread_messages(&self, user: &str, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        if !self.is_thread_participant(user, thread_id)? {
            return Ok(Vec::new());
        }
        let mids = self.user_thread_message_ids(user, thread_id)?;
        let mut out = Vec::with_capacity(mids.len());
        for mid in &mids {
            // Through the same decision as the uid fetch: serving the shared
            // blob would put back the defect, since its blob_ref may be
            // another mailbox's file.
            match self.user_message_view(user, mid)? {
                Some(view) => out.push(view),
                None => continue,
            }
        }
        Ok(out)
    }

    /// Whether `user` holds a copy of `thread_id`.
    pub(crate) fn is_thread_participant(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        self.store()
            .hexists(keys::thread_user(user, thread_id).as_bytes(), b"tid")
            .map_err(std::io::Error::from)
    }

    /// Unfiltered read for in-process maintenance sweeps — self-heal,
    /// backfills, rethread.
    ///
    /// Deliberately not the default. These callers walk a single user's
    /// own index already, but heal exists precisely to repair threads
    /// whose membership row is missing, so gating it on that row would
    /// make the repair a no-op against the damage it is meant to fix.
    /// Never call this from a request path: it answers for any thread
    /// whoever asks.
    pub fn thread_messages_for_maintenance(&self, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        self.thread_messages_unscoped(thread_id)
    }

    // `pub` rather than `pub(crate)`: the thread-date audit in
    // fastcore reads a thread's messages to work out what the row
    // should say. Widening this beats a second way to read them.
    pub fn thread_messages_unscoped(&self, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        let zset = keys::thread_messages(thread_id);
        let entries = self.store().zrange(zset.as_bytes(), 0, -1)?;
        self.store()
            .atomic(|ctx| {
                let mut out = Vec::with_capacity(entries.len());
                for (mid_bytes, _score) in &entries {
                    let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                        continue;
                    };
                    let blob_key = keys::message_blob(mid);
                    if let Some(bytes) = ctx.get(blob_key.as_bytes())? {
                        out.push(bytes);
                    }
                }
                Ok(out)
            })
            .map_err(std::io::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::UserMessageFacts;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        // The aggregate index that derives the counters, too — without
        // it every count reads zero, which looks exactly like a broken
        // count rather than a store that was never fully booted.
        s.ensure_admin_indexes();
        s
    }

    /// Give `user` a copy of `tid` the way a delivery does.
    fn deliver(s: &KevyMailboxStore, user: &str, tid: &str) {
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "alice@x.com",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }

    /// Stage 6: a pre-stage-5 row loses the owner it names, once.
    #[test]
    fn stripping_a_shared_row_is_idempotent() {
        let s = store();
        deliver(&s, "owner@x.com", "t1");
        // A row of the shape stage 5 stopped writing: per-user facts on
        // the blob every owner reads.
        let legacy = br#"{"message_id":"msg-1","blob_ref":"owner-copy.host:2,S","uid":7,"flags":1,"modseq":3,"mailbox_id":2,"user_address":"owner@x.com","subject":"Subj"}"#;
        s.upsert_message("t1", "msg-1", 100, legacy).unwrap();

        let (seen, rewritten) = s.strip_shared_per_user_fields("t1").unwrap();
        assert_eq!((seen, rewritten), (1, 1));
        let blob = s.thread_messages_for_maintenance("t1").unwrap().remove(0);
        let v: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        assert_eq!(v["blob_ref"], "");
        assert_eq!(v["uid"], 0);
        assert_eq!(v["user_address"], "");
        assert_eq!(v["subject"], "Subj", "shared facts are left alone");

        // Second pass: nothing to do, and it says so rather than
        // rewriting the same bytes every cycle.
        assert_eq!(s.strip_shared_per_user_fields("t1").unwrap(), (1, 0));
    }

    #[test]
    fn a_stranger_reads_nothing_from_someone_elses_thread() {
        // The message index is keyed by thread alone, so before the
        // membership gate this returned the full timeline to anybody
        // who could name the thread — and the name is the root
        // Message-ID, which every correspondent already has.
        let s = store();
        deliver(&s, "owner@x.com", "t1");
        s.upsert_user_message(
            "owner@x.com",
            "t1",
            "msg-1",
            100,
            b"private",
            &UserMessageFacts {
                blob_ref: "owner-copy.host",
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();

        assert!(
            s.list_thread_messages("stranger@x.com", "t1")
                .unwrap()
                .is_empty(),
        );
        assert_eq!(
            s.list_thread_messages("owner@x.com", "t1").unwrap().len(),
            1
        );
    }

    /// The uid fetch and the thread listing must answer the missing-copy
    /// question the same way.
    ///
    /// They did not: the listing dropped the message, the uid fetch served
    /// the shared row. Both are reachable from the same click — open a
    /// conversation, then download the attachment — so the two answers were
    /// visible side by side, one of them naming another owner's file. This
    /// pins them to one answer rather than to today's answer.
    #[test]
    fn the_two_readers_agree_when_a_user_has_no_copy() {
        let s = store();
        deliver(&s, "owner@x.com", "t1");
        s.upsert_user_message(
            "owner@x.com",
            "t1",
            "msg-1",
            100,
            b"{\"blob_ref\":\"owner-copy.host\"}",
            &UserMessageFacts {
                blob_ref: "owner-copy.host",
                uid: 7,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
        s.register_uid("owner@x.com", 7, "msg-1").unwrap();
        // A second account on the thread that never received a copy: the
        // thread membership exists and the uid is indexed, the per-user
        // message row does not exist. The uid index is what makes the fetch
        // reachable at all, so without it this would pass for the wrong
        // reason.
        deliver(&s, "other@x.com", "t1");
        s.register_uid("other@x.com", 7, "msg-1").unwrap();

        assert_eq!(
            s.list_thread_messages("other@x.com", "t1").unwrap().len(),
            0,
            "the listing must not serve a copy this user does not have"
        );
        assert!(
            s.get_message_by_uid("other@x.com", 7).unwrap().is_none(),
            "the uid fetch must reach the same conclusion, not fall back \
             to the shared row's blob_ref"
        );
        // And the owner is unaffected by either rule.
        assert_eq!(
            s.list_thread_messages("owner@x.com", "t1").unwrap().len(),
            1
        );
        assert!(s.get_message_by_uid("owner@x.com", 7).unwrap().is_some());
    }

    #[test]
    fn both_participants_of_a_shared_thread_can_read_it() {
        // Two local accounts on one thread — the case that produced the
        // phantom counter — must both keep access.
        let s = store();
        deliver(&s, "a@x.com", "t1");
        deliver(&s, "b@x.com", "t1");
        // Both received it, which is what two local accounts on one thread
        // means: two deliveries, two files, two per-user rows over one
        // shared blob.
        for (u, blob_ref) in [("a@x.com", "a-copy.host"), ("b@x.com", "b-copy.host")] {
            s.upsert_user_message(
                u,
                "t1",
                "msg-1",
                100,
                b"shared",
                &UserMessageFacts {
                    blob_ref,
                    uid: 1,
                    flags: 0,
                    modseq: 1,
                },
            )
            .unwrap();
        }

        assert_eq!(s.list_thread_messages("a@x.com", "t1").unwrap().len(), 1);
        assert_eq!(s.list_thread_messages("b@x.com", "t1").unwrap().len(), 1);
    }

    #[test]
    fn maintenance_reads_bypass_the_gate_on_purpose() {
        // Self-heal repairs threads whose membership row is missing;
        // gating it would make the repair a no-op against the exact
        // damage it exists to fix.
        let s = store();
        s.upsert_message("orphan", "msg-1", 100, b"payload")
            .unwrap();

        assert!(
            s.list_thread_messages("anyone@x.com", "orphan")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            s.thread_messages_for_maintenance("orphan").unwrap().len(),
            1
        );
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let s = store();
        s.upsert_message("t1", "msg-1", 100, b"payload-1").unwrap();
        let got = s.get_message("msg-1").unwrap().unwrap();
        assert_eq!(got, b"payload-1");
    }

    #[test]
    fn list_returns_chronological_order() {
        let s = store();
        // out-of-order insertion
        s.upsert_message("t1", "msg-2", 200, b"second").unwrap();
        s.upsert_message("t1", "msg-1", 100, b"first").unwrap();
        s.upsert_message("t1", "msg-3", 300, b"third").unwrap();
        let got = s.thread_messages_for_maintenance("t1").unwrap();
        assert_eq!(
            got,
            vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
        );
    }

    #[test]
    fn list_empty_thread_returns_empty_vec() {
        let s = store();
        let got = s.thread_messages_for_maintenance("never-existed").unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn register_uid_raises_allocation_counter() {
        let s = store();
        // simulate migration: register a known high uid
        s.register_uid("u@x.y", 27757, "migrated-mid").unwrap();
        // next allocation must NOT collide with the registered range
        let fresh = s.allocate_uid("u@x.y", "new-mid").unwrap();
        assert_eq!(fresh, 27758);
        // registered mapping intact in both directions
        let fwd = s
            .store()
            .hget(keys::user_msg_by_uid("u@x.y").as_bytes(), b"27757")
            .unwrap()
            .unwrap();
        assert_eq!(fwd, b"migrated-mid");
        // idempotent re-register never lowers the counter
        s.register_uid("u@x.y", 5, "old-mid").unwrap();
        assert_eq!(s.allocate_uid("u@x.y", "new-mid-2").unwrap(), 27759);
    }

    #[test]
    fn allocate_uid_concurrent_same_mid_is_idempotent() {
        // 100 threads calling allocate_uid(user, same-mid) must all
        // return the SAME uid and leave the counter at exactly 1.
        // Prior to Stage B.2 the race between the initial hget-miss
        // and incr let two concurrent callers issue two different
        // uids for the same mid.
        use std::sync::Arc;
        use std::thread;
        let s = Arc::new(store());
        let mut handles = Vec::new();
        for _ in 0..100 {
            let sc = Arc::clone(&s);
            handles.push(thread::spawn(move || {
                sc.allocate_uid("u@x.y", "shared-mid").unwrap()
            }));
        }
        let uids: Vec<u32> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(
            uids.iter().all(|u| *u == uids[0]),
            "uids diverged: {uids:?}"
        );
        // Counter must have been bumped exactly once — next fresh
        // allocation reads uids[0] + 1.
        assert_eq!(s.allocate_uid("u@x.y", "next-mid").unwrap(), uids[0] + 1,);
        // Reverse + forward mapping consistent.
        let fwd = s
            .store()
            .hget(
                keys::user_msg_by_uid("u@x.y").as_bytes(),
                uids[0].to_string().as_bytes(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(fwd, b"shared-mid");
    }

    #[test]
    fn upsert_overwrites_existing_payload() {
        let s = store();
        s.upsert_message("t1", "msg-1", 100, b"v1").unwrap();
        s.upsert_message("t1", "msg-1", 100, b"v2").unwrap();
        let got = s.get_message("msg-1").unwrap().unwrap();
        assert_eq!(got, b"v2");
        // zset member dedup'd by member name — still 1 entry
        let zset = keys::thread_messages("t1");
        assert_eq!(s.store().zcard(zset.as_bytes()).unwrap(), 1);
    }
}
