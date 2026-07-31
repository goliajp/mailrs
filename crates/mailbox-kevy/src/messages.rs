//! Per-message storage — write a JSON-serialized payload per message
//! + add to the per-thread index zset (score = internal_date).
//!
//! Phase 7.11. The kevy layout:
//!   mailrs:msg:<message_id>           string  — serde-json of any
//!                                                  type the caller
//!                                                  passes in
//!   mailrs:thread:<tid>:messages      zset    — message_id → internal_date
//!
//! The caller picks the JSON shape. `mailrs-fastcore` writes the same
//! `mailrs_core_api::method::message::MessageWire` rows the monolith
//! returns, so webapi's consumer code is unchanged.

use std::io;

use super::KevyMailboxStore;
use super::keys;

/// What is true of one user's copy of a message, borrowed for the write.
#[derive(Debug, Clone, Copy)]
pub struct UserMessageFacts<'a> {
    /// The maildir filename **in this user's mailbox**.
    pub blob_ref: &'a str,
    /// This user's IMAP UID for it.
    pub uid: u32,
    /// This user's flags.
    pub flags: u32,
    /// This user's mod-sequence.
    pub modseq: u64,
}

/// The same, owned, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedUserMessageFacts {
    /// The maildir filename in this user's mailbox.
    pub blob_ref: String,
    /// This user's IMAP UID.
    pub uid: u32,
    /// This user's flags.
    pub flags: u32,
    /// This user's mod-sequence.
    pub modseq: u64,
}

impl KevyMailboxStore {
    /// Write `payload` bytes to the message-blob key + zadd to the
    /// thread's message index with score = `internal_date`. `payload`
    /// is opaque — callers usually pass a serde-json'd MessageWire.
    ///
    /// **Crate-private on purpose.** It writes the shared half and not the
    /// per-user row, so a caller outside this crate using it leaves the
    /// message invisible to the per-user read path — the
    /// `every-writer-maintains-the-row` failure this projection exists to
    /// prevent, and the one `record_message_arrival` already caused once by
    /// not writing the membership row. [`upsert_user_message`] is the entry
    /// point; omitting the per-user facts is a compile error rather than a
    /// message nobody can see.
    pub(crate) fn upsert_message(
        &self,
        thread_id: &str,
        message_id: &str,
        internal_date: i64,
        payload: &[u8],
    ) -> io::Result<()> {
        let blob_key = keys::message_blob(message_id);
        let zset = keys::thread_messages(thread_id);
        self.store()
            .atomic(|ctx| {
                ctx.set(blob_key.as_bytes(), payload);
                ctx.zadd(
                    zset.as_bytes(),
                    &[(internal_date as f64, message_id.as_bytes())],
                )?;
                Ok(())
            })
            .map_err(std::io::Error::other)
    }

    /// Write a message *as one user's copy of it*.
    ///
    /// The shared blob and index as before, plus the per-user row and the
    /// per-user thread index. Those two are what make a mailbox a mailbox:
    /// a maildir filename, a uid and a set of flags are true of one owner
    /// and not another, and a thread can have several. Six such fields sat
    /// on the shared blob until 2026-07-31, and on a multi-owner thread each
    /// was whoever wrote last — 74 messages on production were served to a
    /// user the row did not name, their `blob_ref` pointing into somebody
    /// else's maildir.
    ///
    /// Stage 1 of `.claude/rfcs/20260731-per-user-message-projection.md`:
    /// written and not yet read, so a backfill and a shadow comparison can
    /// run before anything depends on it.
    pub fn upsert_user_message(
        &self,
        user: &str,
        thread_id: &str,
        message_id: &str,
        internal_date: i64,
        payload: &[u8],
        per_user: &UserMessageFacts<'_>,
    ) -> io::Result<()> {
        let blob_key = keys::message_blob(message_id);
        let zset = keys::thread_messages(thread_id);
        let user_key = keys::user_message(user, message_id);
        let user_zset = keys::thread_user_messages(user, thread_id);
        let uid = per_user.uid.to_string();
        let flags = per_user.flags.to_string();
        let modseq = per_user.modseq.to_string();
        self.store()
            .atomic(|ctx| {
                ctx.set(blob_key.as_bytes(), payload);
                ctx.zadd(
                    zset.as_bytes(),
                    &[(internal_date as f64, message_id.as_bytes())],
                )?;
                ctx.hset(
                    user_key.as_bytes(),
                    &[
                        (b"blob_ref".as_slice(), per_user.blob_ref.as_bytes()),
                        (b"uid".as_slice(), uid.as_bytes()),
                        (b"flags".as_slice(), flags.as_bytes()),
                        (b"modseq".as_slice(), modseq.as_bytes()),
                    ],
                )?;
                ctx.zadd(
                    user_zset.as_bytes(),
                    &[(internal_date as f64, message_id.as_bytes())],
                )?;
                Ok(())
            })
            .map_err(std::io::Error::other)
    }

    /// One user's facts about one message, or `None` if they have no copy.
    pub fn user_message_facts(
        &self,
        user: &str,
        message_id: &str,
    ) -> io::Result<Option<OwnedUserMessageFacts>> {
        let key = keys::user_message(user, message_id);
        let flat = self
            .store()
            .hgetall(key.as_bytes())
            .map_err(std::io::Error::other)?;
        if flat.is_empty() {
            return Ok(None);
        }
        let mut blob_ref = None;
        let mut uid = 0u32;
        let mut flags = 0u32;
        let mut modseq = 0u64;
        // The embedded store returns pairs, not the network client's flat
        // alternating list.
        for (field, value) in &flat {
            let v = String::from_utf8_lossy(value).to_string();
            match std::str::from_utf8(field).unwrap_or("") {
                "blob_ref" => blob_ref = Some(v),
                "uid" => uid = v.parse().unwrap_or(0),
                "flags" => flags = v.parse().unwrap_or(0),
                "modseq" => modseq = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        // A row without a blob_ref is not a copy — returning one would put
        // the caller back to building a path from the requesting user,
        // which is the defect this replaces.
        Ok(blob_ref.map(|blob_ref| OwnedUserMessageFacts {
            blob_ref,
            uid,
            flags,
            modseq,
        }))
    }

    /// The message ids one user has in one thread, oldest first.
    pub fn user_thread_message_ids(&self, user: &str, thread_id: &str) -> io::Result<Vec<String>> {
        let key = keys::thread_user_messages(user, thread_id);
        let members = self
            .store()
            .zrange(key.as_bytes(), 0, -1)
            .map_err(std::io::Error::other)?;
        Ok(members
            .into_iter()
            .filter_map(|(m, _)| String::from_utf8(m).ok())
            .collect())
    }

    /// Read message bytes for `message_id`. Returns `None` if the key
    /// is missing (deleted or never written).
    pub fn get_message(&self, message_id: &str) -> io::Result<Option<Vec<u8>>> {
        let key = keys::message_blob(message_id);
        self.store()
            .get(key.as_bytes())
            .map_err(std::io::Error::other)
    }

    /// Look up a message by (user, uid) via the per-user uid → message_id
    /// hash. Returns the raw payload bytes (JSON MessageWire) or None
    /// when the uid isn't indexed (or the message was deleted).
    pub fn get_message_by_uid(&self, user: &str, uid: u32) -> io::Result<Option<Vec<u8>>> {
        let idx_key = keys::user_msg_by_uid(user);
        let mid_bytes = self
            .store()
            .hget(idx_key.as_bytes(), uid.to_string().as_bytes())
            .map_err(std::io::Error::other)?;
        let Some(mid_bytes) = mid_bytes else {
            return Ok(None);
        };
        let mid = String::from_utf8_lossy(&mid_bytes).to_string();
        self.get_message(&mid)
    }

    /// Populate the per-user uid → message_id index for a single message.
    /// Called from deliver / migrate paths so per-uid lookups are O(1).
    /// Register a KNOWN (user, uid, message_id) triple — both direction
    /// maps AND raise the allocation counter so future `allocate_uid`
    /// calls never re-issue this uid. This is what migration/backfill
    /// tooling must use: writing only the forward map (the old backfill
    /// behaviour) left `next_uid` at 0, so the first post-migration
    /// delivery allocated uid=1 and overwrote the migrated message's
    /// forward mapping.
    pub fn register_uid(&self, user: &str, uid: u32, message_id: &str) -> io::Result<()> {
        if uid == 0 {
            return Ok(());
        }
        // v2 Stage B.2: rev + forward + counter-max collapsed into one
        // atomic closure. Prior implementation could race the counter
        // read with a concurrent allocate_uid's incr — the pre-fix
        // counter cur could be stale and the conditional set could
        // shrink the counter back below the value allocate_uid already
        // moved past, letting future allocations collide with a uid
        // this backfill just installed.
        let rev_key = keys::user_uid_by_mid(user);
        let idx_key = keys::user_msg_by_uid(user);
        let counter_key = keys::user_next_uid(user);
        self.store()
            .atomic(|ctx| {
                ctx.hset(
                    rev_key.as_bytes(),
                    &[(message_id.as_bytes(), uid.to_string().as_bytes())],
                )?;
                ctx.hset(
                    idx_key.as_bytes(),
                    &[(uid.to_string().as_bytes(), message_id.as_bytes())],
                )?;
                let cur = ctx
                    .get(counter_key.as_bytes())?
                    .and_then(|b| String::from_utf8(b).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                if cur < uid as i64 {
                    ctx.set(counter_key.as_bytes(), uid.to_string().as_bytes());
                }
                Ok(())
            })
            .map_err(std::io::Error::other)
    }

    pub fn index_uid(&self, user: &str, uid: u32, message_id: &str) -> io::Result<()> {
        let idx_key = keys::user_msg_by_uid(user);
        self.store()
            .hset(
                idx_key.as_bytes(),
                &[(uid.to_string().as_bytes(), message_id.as_bytes())],
            )
            .map_err(std::io::Error::other)?;
        Ok(())
    }

    /// Assign a per-user uid to `message_id` and persist both directions
    /// of the mapping. Idempotent: if the message already has a uid,
    /// the existing value is returned without touching the counter.
    ///
    /// Used by the self-heal path so `/api/mail/messages/{uid}/…`
    /// endpoints (raw source, attachments) can resolve messages that
    /// weren't handed a uid by the monolith migration.
    pub fn allocate_uid(&self, user: &str, message_id: &str) -> io::Result<u32> {
        // v2 Stage B.2 · Phase 2: entire idempotent-check + counter-incr
        // + reverse+forward index write runs inside one shard-write
        // lock. Prior implementation could race between the initial
        // hget miss and the incr — two concurrent allocate_uid calls
        // for the same message_id issued two different uids and left
        // one orphaned in the forward index.
        let rev_key = keys::user_uid_by_mid(user);
        let counter_key = keys::user_next_uid(user);
        let idx_key = keys::user_msg_by_uid(user);
        self.store()
            .atomic(|ctx| {
                if let Some(existing) = ctx.hget(rev_key.as_bytes(), message_id.as_bytes())?
                    && let Ok(s) = std::str::from_utf8(&existing)
                    && let Ok(uid) = s.parse::<u32>()
                {
                    return Ok(uid);
                }
                let uid_i = ctx.incr(counter_key.as_bytes())?;
                let uid = uid_i.clamp(1, u32::MAX as i64) as u32;
                ctx.hset(
                    rev_key.as_bytes(),
                    &[(message_id.as_bytes(), uid.to_string().as_bytes())],
                )?;
                ctx.hset(
                    idx_key.as_bytes(),
                    &[(uid.to_string().as_bytes(), message_id.as_bytes())],
                )?;
                Ok(uid)
            })
            .map_err(std::io::Error::other)
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
    pub fn list_thread_messages(&self, user: &str, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        if !self.is_thread_participant(user, thread_id)? {
            return Ok(Vec::new());
        }
        self.thread_messages_unscoped(thread_id)
    }

    /// Whether `user` holds a copy of `thread_id`.
    pub(crate) fn is_thread_participant(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        self.store()
            .hexists(keys::thread_user(user, thread_id).as_bytes(), b"tid")
            .map_err(std::io::Error::other)
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

    pub(crate) fn thread_messages_unscoped(&self, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        let zset = keys::thread_messages(thread_id);
        let entries = self
            .store()
            .zrange(zset.as_bytes(), 0, -1)
            .map_err(std::io::Error::other)?;
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
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn a_stranger_reads_nothing_from_someone_elses_thread() {
        // The message index is keyed by thread alone, so before the
        // membership gate this returned the full timeline to anybody
        // who could name the thread — and the name is the root
        // Message-ID, which every correspondent already has.
        let s = store();
        deliver(&s, "owner@x.com", "t1");
        s.upsert_message("t1", "msg-1", 100, b"private").unwrap();

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

    #[test]
    fn both_participants_of_a_shared_thread_can_read_it() {
        // Two local accounts on one thread — the case that produced the
        // phantom counter — must both keep access.
        let s = store();
        deliver(&s, "a@x.com", "t1");
        deliver(&s, "b@x.com", "t1");
        s.upsert_message("t1", "msg-1", 100, b"shared").unwrap();

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

#[cfg(test)]
mod user_message_tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use super::*;

    fn store() -> KevyMailboxStore {
        KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("in-memory kevy"),
        ))
    }

    fn facts<'a>(blob_ref: &'a str, uid: u32) -> UserMessageFacts<'a> {
        UserMessageFacts {
            blob_ref,
            uid,
            flags: 0,
            modseq: 1,
        }
    }

    /// The case that produced this: one message delivered to two accounts.
    ///
    /// Each has its own file on disk, its own uid and its own flags. Sharing
    /// one `blob_ref` between them meant one of the two read a filename in
    /// the other's maildir — 74 messages on production, and the reason
    /// `devops@golia.jp` saw an empty body on a message it had received.
    ///
    /// No test covered two owners before this one, which is how six per-user
    /// fields sat on a shared row without anything noticing.
    #[test]
    fn two_recipients_of_one_message_each_read_their_own_copy() {
        let s = store();
        let mid = "<m1@example.com>";
        let payload = br#"{"subject":"Meeting"}"#;

        s.upsert_user_message(
            "lihao@golia.jp",
            "t1",
            mid,
            100,
            payload,
            &facts("1785371752.M825462P1Q12.host", 7),
        )
        .expect("lihao copy");
        s.upsert_user_message(
            "devops@golia.jp",
            "t1",
            mid,
            100,
            payload,
            &facts("1785371752.M746980P1Q2.other", 3),
        )
        .expect("devops copy");

        let lihao = s
            .user_message_facts("lihao@golia.jp", mid)
            .expect("read")
            .expect("lihao has a copy");
        let devops = s
            .user_message_facts("devops@golia.jp", mid)
            .expect("read")
            .expect("devops has a copy");

        assert_ne!(
            lihao.blob_ref, devops.blob_ref,
            "each owner's blob_ref names a file in their own maildir"
        );
        assert_eq!(lihao.blob_ref, "1785371752.M825462P1Q12.host");
        assert_eq!(devops.blob_ref, "1785371752.M746980P1Q2.other");
        // uids are allocated per user and must not be shared either.
        assert_eq!(lihao.uid, 7);
        assert_eq!(devops.uid, 3);

        // One shared blob underneath, holding what is genuinely shared.
        assert_eq!(
            s.get_message(mid).expect("get").as_deref(),
            Some(&payload[..])
        );
    }

    /// A mailbox contains the mail its owner received.
    ///
    /// The shared `thread:{tid}:messages` index gives every owner every
    /// message in the thread whoever it was delivered to. A message sent to
    /// one owner only must not appear in the other's thread.
    #[test]
    fn a_message_one_owner_never_received_is_not_in_their_thread() {
        let s = store();
        s.upsert_user_message(
            "lihao@golia.jp",
            "t1",
            "<shared@example.com>",
            100,
            b"{}",
            &facts("a.host", 1),
        )
        .expect("shared");
        s.upsert_user_message(
            "devops@golia.jp",
            "t1",
            "<shared@example.com>",
            100,
            b"{}",
            &facts("b.host", 1),
        )
        .expect("shared");
        // A later message in the same thread, delivered to lihao only.
        s.upsert_user_message(
            "lihao@golia.jp",
            "t1",
            "<lihao-only@example.com>",
            200,
            b"{}",
            &facts("c.host", 2),
        )
        .expect("lihao only");

        let lihao = s
            .user_thread_message_ids("lihao@golia.jp", "t1")
            .expect("read");
        let devops = s
            .user_thread_message_ids("devops@golia.jp", "t1")
            .expect("read");

        assert_eq!(lihao.len(), 2);
        assert_eq!(
            devops,
            vec!["<shared@example.com>".to_string()],
            "devops received one of the two and must see one"
        );
        assert!(
            s.user_message_facts("devops@golia.jp", "<lihao-only@example.com>")
                .expect("read")
                .is_none()
        );
    }

    /// Ordering is by internal_date, oldest first — the timeline order.
    #[test]
    fn a_users_thread_is_in_arrival_order() {
        let s = store();
        for (mid, date) in [("<c@x>", 300), ("<a@x>", 100), ("<b@x>", 200)] {
            s.upsert_user_message("u@x.com", "t1", mid, date, b"{}", &facts("f.host", 1))
                .expect("write");
        }
        assert_eq!(
            s.user_thread_message_ids("u@x.com", "t1").expect("read"),
            vec!["<a@x>", "<b@x>", "<c@x>"]
        );
    }

    /// No row means no copy, and the caller must be able to tell that from
    /// a row that exists — otherwise it falls back to building a path from
    /// the requesting user, which is the defect being replaced.
    #[test]
    fn a_user_without_a_copy_reads_none() {
        let s = store();
        assert!(
            s.user_message_facts("nobody@x.com", "<m@x>")
                .expect("read")
                .is_none()
        );
    }
}
