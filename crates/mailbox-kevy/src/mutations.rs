//! Thread-level mutations — archive / unarchive / pin / unpin / delete.
//!
//! v2 Stage B.1: kevy 3.17 `AtomicCtx` gained `zrem` and `hdel`, so
//! the old two-step "hset in atomic + zrem outside" workaround is gone
//! — every mutator collapses into a single closure holding one shard
//! write lock. `delete_thread` is the heaviest one: 1 hget + 1 hdel +
//! 7 zrem now serialize atomically.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Flip `archived` on or off for `thread_id`. Toggles both the
    /// `archived` field on the row and membership in the per-user
    /// archived zset (added when archived=true, removed otherwise).
    ///
    /// Returns true if the row existed.
    pub fn set_archived(&self, user: &str, thread_id: &str, archived: bool) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "archived",
            archived,
            keys::user_threads_archived,
        )
    }

    /// Flip `pinned` on or off. Same shape as `set_archived`.
    pub fn set_pinned(&self, user: &str, thread_id: &str, pinned: bool) -> io::Result<bool> {
        self.toggle_flag(user, thread_id, "pinned", pinned, keys::user_threads_pinned)
    }

    /// Flip `has_action` on or off. Same shape.
    pub fn set_has_action(
        &self,
        user: &str,
        thread_id: &str,
        has_action: bool,
    ) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "has_action",
            has_action,
            keys::user_threads_has_action,
        )
    }

    /// Flip `starred` on or off. Same shape — toggles `starred` field
    /// + per-user `starred` zset membership.
    pub fn set_starred(&self, user: &str, thread_id: &str, starred: bool) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "starred",
            starred,
            keys::user_threads_starred,
        )
    }

    /// Move a thread between the Inbox and Junk top-level folders
    /// (v2.4.1 roadmap Phase 3, RFC-B §3.4). `is_junk=true` writes
    /// `category="spam"`, adds the thread to
    /// `user_threads_junk`, and removes it from `user_threads_inbox`.
    /// `is_junk=false` flips both memberships and rewrites `category`
    /// to `"inbox"`.
    ///
    /// Returns true if the row existed. The `by_category:*` zsets
    /// are NOT rebuilt here — the row's old category zset entry
    /// stays behind for one arrival cycle. That's harmless because
    /// list handlers filter by folder axis first (§Phase 2 read
    /// path), and the entry gets cleaned up on the next
    /// `upsert_thread`.
    pub fn set_junk(&self, user: &str, thread_id: &str, is_junk: bool) -> io::Result<bool> {
        // Thin back-compat wrapper over set_bucket (v2.9): mark-junk
        // stays a two-value flip between Junk and Inbox.
        self.set_bucket(
            user,
            thread_id,
            if is_junk {
                keys::Bucket::Junk
            } else {
                keys::Bucket::Inbox
            },
        )
    }

    /// Force `thread_id` into a triage bucket ∈ {inbox, notifications,
    /// promotions, junk} — stamps the thread's `category` field to the
    /// bucket's canonical category and moves it between the four folder
    /// zsets (zadd target, zrem the other three) in one atomic closure.
    ///
    /// Returns true if the row existed. The `by_category:*` zsets are
    /// NOT rebuilt here (same rationale as the old set_junk) — cleaned
    /// on the next `upsert_thread`.
    pub fn set_bucket(
        &self,
        user: &str,
        thread_id: &str,
        bucket: keys::Bucket,
    ) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let target = bucket.zset(user);
        let others: Vec<String> = keys::Bucket::all_zsets(user)
            .into_iter()
            .filter(|k| *k != target)
            .collect();
        let new_category = bucket.category().as_bytes();
        self.store().atomic(|ctx| {
            if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                return Ok(false);
            }
            ctx.hset(
                thread_key.as_bytes(),
                &[(b"category" as &[u8], new_category)],
            )?;
            let score = ctx
                .hget(thread_key.as_bytes(), b"latest_date")?
                .and_then(|v| {
                    std::str::from_utf8(&v)
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .unwrap_or(0);
            ctx.zadd(target.as_bytes(), &[(score as f64, thread_id.as_bytes())])?;
            for other in &others {
                ctx.zrem(other.as_bytes(), &[thread_id.as_bytes()])?;
            }
            Ok(true)
        })
    }

    /// Common path: read latest_date (for the zadd score), hset the
    /// boolean field, and add or remove from the matching index zset.
    fn toggle_flag<F>(
        &self,
        user: &str,
        thread_id: &str,
        field: &'static str,
        on: bool,
        index_key_fn: F,
    ) -> io::Result<bool>
    where
        F: FnOnce(&str) -> String,
    {
        let thread_key = keys::thread(thread_id);
        let idx = index_key_fn(user);
        let val: &[u8] = if on { b"1" } else { b"0" };
        self.store().atomic(|ctx| {
            if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                return Ok(false);
            }
            ctx.hset(thread_key.as_bytes(), &[(field.as_bytes(), val)])?;
            if on {
                // Need a score — use the row's latest_date so the index
                // stays sortable by recency.
                let score = ctx
                    .hget(thread_key.as_bytes(), b"latest_date")?
                    .and_then(|v| {
                        std::str::from_utf8(&v)
                            .ok()
                            .and_then(|s| s.parse::<i64>().ok())
                    })
                    .unwrap_or(0);
                ctx.zadd(idx.as_bytes(), &[(score as f64, thread_id.as_bytes())])?;
            } else {
                ctx.zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
            }
            Ok(true)
        })
    }

    /// Flip a thread back to unread. Mirrors `mark_seen` in the
    /// opposite direction: set `unread_count` to at least 1 and add the
    /// row to `has_unread`. Score used is the row's own `latest_date` so
    /// the has_unread index remains sortable.
    ///
    /// Returns `true` when the row existed. Idempotent.
    pub fn mark_unread(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let idx = keys::user_threads_has_unread(user);
        self.store().atomic(|ctx| {
            if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                return Ok(false);
            }
            let latest = ctx
                .hget(thread_key.as_bytes(), b"latest_date")?
                .and_then(|v| {
                    std::str::from_utf8(&v)
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .unwrap_or(0);
            let cur = ctx
                .hget(thread_key.as_bytes(), b"unread_count")?
                .and_then(|v| {
                    std::str::from_utf8(&v)
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .unwrap_or(0);
            if cur < 1 {
                ctx.hset(thread_key.as_bytes(), &[(b"unread_count" as &[u8], b"1")])?;
            }
            ctx.zadd(idx.as_bytes(), &[(latest as f64, thread_id.as_bytes())])?;
            Ok(true)
        })
    }

    /// Set `snoozed_until` (epoch seconds; `0` = unsnooze) on the
    /// thread. No dedicated index zset — snoozed threads still appear
    /// in activity/category zsets; the webapi filters by comparing
    /// `snoozed_until > now` when the user selects the "hide snoozed"
    /// view.
    ///
    /// Returns `true` when the row existed.
    pub fn set_snoozed(
        &self,
        _user: &str,
        thread_id: &str,
        snoozed_until: i64,
    ) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let val = snoozed_until.to_string();
        self.store().atomic(|ctx| {
            if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                return Ok(false);
            }
            ctx.hset(
                thread_key.as_bytes(),
                &[(b"snoozed_until" as &[u8], val.as_bytes())],
            )?;
            Ok(true)
        })
    }

    /// Hard-delete `thread_id` for `user`. Removes the row hash + drops
    /// it from every index zset the row could be in. Idempotent: a
    /// re-call after deletion is a no-op returning false.
    ///
    /// Reads `category` BEFORE the deletion so we know which
    /// per-category zset to clean — that index is keyed by the
    /// category string, not derivable from the tid alone.
    ///
    /// **Returns the `blob_ref`s of every message the thread carried.**
    /// The caller is responsible for `unlink`-ing those maildir files —
    /// they live on disk, kevy can't reach them, and self-heal will
    /// resurrect the whole thread from any surviving file on its next
    /// tick. Confirmed on prod 2026-07-24 with two "ghost FYI" threads
    /// that the pre-fix delete had turned into permanent zombies.
    pub fn delete_thread(&self, user: &str, thread_id: &str) -> io::Result<(bool, Vec<String>)> {
        let thread_key = keys::thread(thread_id);
        let msgs_zset = keys::thread_messages(thread_id);
        let store = self.store();

        // Enumerate messages OUTSIDE the atomic block: AtomicCtx has no
        // `zrange`, and every blob is a plain `get` — one round trip
        // per message on a typical thread (< 20 hops).
        let members = store.zrange(msgs_zset.as_bytes(), 0, -1)?;
        let mut per_msg: Vec<(String, Option<u32>, Option<String>)> =
            Vec::with_capacity(members.len());
        for (mid_bytes, _score) in &members {
            let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                continue;
            };
            let blob = store.get(keys::message_blob(mid).as_bytes())?;
            let (uid, blob_ref) = match blob.as_deref() {
                Some(bytes) => {
                    let v: serde_json::Value =
                        serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null);
                    let uid = v
                        .get("uid")
                        .and_then(|x| x.as_u64())
                        .filter(|u| *u > 0)
                        .map(|u| u as u32);
                    let blob_ref = v
                        .get("blob_ref")
                        .and_then(|x| x.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    (uid, blob_ref)
                }
                None => (None, None),
            };
            per_msg.push((mid.to_string(), uid, blob_ref));
        }
        let blob_refs: Vec<String> = per_msg.iter().filter_map(|(_, _, br)| br.clone()).collect();

        // No "hclear" in kevy 3.17; keep the explicit field list. Single
        // source of truth with the write path — a field written but not
        // listed here survives the delete and resurrects the row.
        let fields = crate::thread_row::ThreadRow::field_names();
        let msg_by_uid_key = keys::user_msg_by_uid(user);
        let uid_by_mid_key = keys::user_uid_by_mid(user);

        let existed = store.atomic(|ctx| {
            let category = ctx
                .hget(thread_key.as_bytes(), b"category")?
                .and_then(|v| String::from_utf8(v).ok());
            let Some(cat) = category else {
                // hash doesn't exist — nothing to clean.
                return Ok(false);
            };

            // Per-message cleanup — msg blob, RFC Message-ID → thread
            // pointer, and both directions of the uid ↔ message-id map.
            // Any of these left behind kept the message reachable via
            // find_by_message_id / by_uid lookups even after the row
            // vanished from the thread aggregate.
            for (mid, uid, _blob_ref) in &per_msg {
                ctx.del(&[keys::message_blob(mid).as_bytes()]);
                ctx.del(&[keys::message_by_message_id(user, mid).as_bytes()]);
                if let Some(u) = uid {
                    let uid_s = u.to_string();
                    ctx.hdel(msg_by_uid_key.as_bytes(), &[uid_s.as_bytes()])?;
                }
                ctx.hdel(uid_by_mid_key.as_bytes(), &[mid.as_bytes()])?;
            }

            // The thread's own message-index zset.
            ctx.del(&[msgs_zset.as_bytes()]);

            // Thread aggregate fields.
            ctx.hdel(thread_key.as_bytes(), fields)?;

            // Per-user thread indexes.
            let indexes = [
                keys::user_threads_by_activity(user),
                keys::user_threads_by_category(user, &cat),
                keys::user_threads_pinned(user),
                keys::user_threads_archived(user),
                keys::user_threads_has_unread(user),
                keys::user_threads_has_action(user),
                keys::user_threads_starred(user),
                // v2.8.2 — the Phase 2 folder zsets were missing from
                // this cleanup list, leaving orphan members behind on
                // every delete (invisible rows, inflated zcard totals).
                // v2.9 — the notifications/promotions buckets join them.
                keys::user_threads_inbox(user),
                keys::user_threads_notifications(user),
                keys::user_threads_promotions(user),
                keys::user_threads_junk(user),
                keys::user_threads_sent(user),
            ];
            for idx in &indexes {
                ctx.zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
            }
            Ok(true)
        })?;
        Ok((existed, blob_refs))
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

    fn arr<'a>(tid: &'a str, user: &'a str) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread: true,
            is_own: false,
        }
    }

    #[test]
    fn set_archived_round_trip() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();

        assert!(s.set_archived(u, "t1", true).unwrap());
        assert!(s.get_thread("t1").unwrap().unwrap().archived);
        let arch = keys::user_threads_archived(u);
        assert_eq!(s.store().zcard(arch.as_bytes()).unwrap(), 1);

        assert!(s.set_archived(u, "t1", false).unwrap());
        assert!(!s.get_thread("t1").unwrap().unwrap().archived);
        assert_eq!(s.store().zcard(arch.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn set_pinned_uses_pinned_zset() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        assert!(s.set_pinned(u, "t1", true).unwrap());
        let pinned = keys::user_threads_pinned(u);
        assert_eq!(s.store().zcard(pinned.as_bytes()).unwrap(), 1);
        // appears in the pinned filter list
        let f = crate::ListThreadsFilter {
            pinned: true,
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].thread_id, "t1");
    }

    #[test]
    fn delete_thread_clears_all_indexes() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        s.set_archived(u, "t1", true).unwrap();
        s.set_pinned(u, "t1", true).unwrap();
        s.set_has_action(u, "t1", true).unwrap();

        // every zset has the row now
        for idx in [
            keys::user_threads_by_activity(u),
            keys::user_threads_by_category(u, "inbox"),
            keys::user_threads_pinned(u),
            keys::user_threads_archived(u),
            keys::user_threads_has_unread(u),
            keys::user_threads_has_action(u),
        ] {
            assert_eq!(s.store().zcard(idx.as_bytes()).unwrap(), 1, "idx {idx}");
        }

        // v2.8.2: arrival also filed the row into the Inbox folder zset.
        let inbox = keys::user_threads_inbox(u);
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 1);

        let (existed, blob_refs) = s.delete_thread(u, "t1").unwrap();
        assert!(existed);
        // record_message_arrival did not write a message blob, so the
        // returned blob_ref list is empty — this test covers the
        // thread-hash + index cleanup half only. A dedicated
        // messages-and-files test would seed upsert_message first.
        assert!(blob_refs.is_empty());
        assert!(s.get_thread("t1").unwrap().is_none());
        for idx in [
            keys::user_threads_by_activity(u),
            keys::user_threads_by_category(u, "inbox"),
            keys::user_threads_pinned(u),
            keys::user_threads_archived(u),
            keys::user_threads_has_unread(u),
            keys::user_threads_has_action(u),
            // v2.8.2 — delete must also clear the folder zsets.
            keys::user_threads_inbox(u),
            keys::user_threads_junk(u),
            keys::user_threads_sent(u),
        ] {
            assert_eq!(s.store().zcard(idx.as_bytes()).unwrap(), 0, "idx {idx}");
        }
    }

    #[test]
    fn set_bucket_migrates_between_all_four_buckets() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap(); // inbound → Inbox
        let inbox = keys::user_threads_inbox(u);
        let notif = keys::user_threads_notifications(u);
        let promo = keys::user_threads_promotions(u);
        let junk = keys::user_threads_junk(u);
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 1);

        // Inbox → Promotions: only Promotions holds it now.
        assert!(s.set_bucket(u, "t1", keys::Bucket::Promotions).unwrap());
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(promo.as_bytes()).unwrap(), 1);
        assert_eq!(s.get_thread("t1").unwrap().unwrap().category, "promotion");

        // Promotions → Notifications.
        assert!(s.set_bucket(u, "t1", keys::Bucket::Notifications).unwrap());
        assert_eq!(s.store().zcard(promo.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(notif.as_bytes()).unwrap(), 1);

        // Notifications → Junk (via the set_junk back-compat wrapper).
        assert!(s.set_junk(u, "t1", true).unwrap());
        assert_eq!(s.store().zcard(notif.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(junk.as_bytes()).unwrap(), 1);

        // Junk → Inbox (move-to-inbox path).
        assert!(s.set_bucket(u, "t1", keys::Bucket::Inbox).unwrap());
        assert_eq!(s.store().zcard(junk.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 1);
        // Exactly one bucket ever holds it.
        for z in [&notif, &promo, &junk] {
            assert_eq!(s.store().zcard(z.as_bytes()).unwrap(), 0);
        }
    }

    #[test]
    fn delete_missing_returns_false() {
        let s = store();
        let (existed, blob_refs) = s.delete_thread("u@x.com", "nope").unwrap();
        assert!(!existed);
        assert!(blob_refs.is_empty());
    }

    #[test]
    fn delete_returns_blob_refs_for_upserted_messages() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        // Two messages under the thread, each with a distinct blob_ref
        // pointing at a maildir filename the caller must unlink.
        let m1 = serde_json::json!({
            "uid": 42, "blob_ref": "1784.M0P1Q0.host:2,S", "thread_id": "t1",
            "message_id": "aaa@x", "internal_date": 100,
        })
        .to_string();
        let m2 = serde_json::json!({
            "uid": 43, "blob_ref": ".Sent/1785.M0P1Q1.host:2,S", "thread_id": "t1",
            "message_id": "bbb@x", "internal_date": 200,
        })
        .to_string();
        s.upsert_message("t1", "aaa@x", 100, m1.as_bytes()).unwrap();
        s.upsert_message("t1", "bbb@x", 200, m2.as_bytes()).unwrap();

        let (existed, blob_refs) = s.delete_thread(u, "t1").unwrap();
        assert!(existed);
        let mut br = blob_refs;
        br.sort();
        assert_eq!(
            br,
            vec![
                "1784.M0P1Q0.host:2,S".to_string(),
                ".Sent/1785.M0P1Q1.host:2,S".to_string(),
            ]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );

        // Neither the message blobs nor the thread-messages zset should
        // survive — the whole point of the refactor.
        assert!(s.get_message("aaa@x").unwrap().is_none());
        assert!(s.get_message("bbb@x").unwrap().is_none());
        assert_eq!(
            s.store()
                .zcard(keys::thread_messages("t1").as_bytes())
                .unwrap(),
            0
        );
    }

    #[test]
    fn flag_toggle_on_missing_thread_returns_false() {
        let s = store();
        assert!(!s.set_archived("u@x.com", "nope", true).unwrap());
        assert!(!s.set_pinned("u@x.com", "nope", true).unwrap());
    }
}
