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
        self.toggle_flag(user, thread_id, "archived", archived)
    }

    /// Flip `pinned` on or off. Same shape as `set_archived`.
    pub fn set_pinned(&self, user: &str, thread_id: &str, pinned: bool) -> io::Result<bool> {
        self.toggle_flag(user, thread_id, "pinned", pinned)
    }

    /// Flip `has_action` on or off. Same shape.
    pub fn set_has_action(
        &self,
        user: &str,
        thread_id: &str,
        has_action: bool,
    ) -> io::Result<bool> {
        self.toggle_flag(user, thread_id, "has_action", has_action)
    }

    /// Flip `starred` on or off. Same shape — toggles `starred` field
    /// + per-user `starred` zset membership.
    pub fn set_starred(&self, user: &str, thread_id: &str, starred: bool) -> io::Result<bool> {
        self.toggle_flag(user, thread_id, "starred", starred)
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
        let new_category = bucket.category().as_bytes();
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                    return Ok(false);
                }
                // Moving a thread into a folder un-archives it.
                // Archiving means "out of the incoming stream"; filing
                // it somewhere is putting it back. Without this the
                // thread lands in its new bucket and stays in the
                // Archived tab, which is where it was moved *from* —
                // the move appears to do nothing.
                //
                // Safe because this is only ever a user action:
                // classification on the ingest path goes through
                // record_message_arrival and never calls this.
                ctx.hset(
                    thread_key.as_bytes(),
                    &[(b"category" as &[u8], new_category), (b"archived", b"0")],
                )?;
                ctx.hset(
                    keys::thread_user(user, thread_id).as_bytes(),
                    &[
                        (b"bucket".as_slice(), bucket.name().as_bytes()),
                        (b"category".as_slice(), new_category),
                        (b"archived".as_slice(), b"0".as_slice()),
                    ],
                )?;
                Ok(true)
            })
            .map_err(std::io::Error::from)
    }

    /// Common path: hset the boolean field on **this user's** membership
    /// row, which is where the declared indexes read it.
    ///
    /// The shared hash no longer receives it. It has no user segment, so
    /// a flag on it is one owner's answer offered to everybody — and it
    /// was: `thread_user_pairs` read it back out on every arrival and
    /// wrote it onto each owner's row.
    ///
    /// The existence check stays on the shared hash: it is what says the
    /// conversation exists at all, and starring something that is not
    /// there should still answer `false`.
    fn toggle_flag(
        &self,
        user: &str,
        thread_id: &str,
        field: &'static str,
        on: bool,
    ) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let val: &[u8] = if on { b"1" } else { b"0" };
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                    return Ok(false);
                }
                ctx.hset(
                    keys::thread_user(user, thread_id).as_bytes(),
                    &[(field.as_bytes(), val)],
                )?;
                Ok(true)
            })
            .map_err(std::io::Error::from)
    }

    /// Flip a thread back to unread. Mirrors `mark_seen` in the
    /// opposite direction: set `unread_count` to at least 1 and add the
    /// row to `has_unread`. Score used is the row's own `latest_date` so
    /// the has_unread index remains sortable.
    ///
    /// Returns `true` when the row existed. Idempotent.
    pub fn mark_unread(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(thread_key.as_bytes(), b"count")? {
                    return Ok(false);
                }
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
                // The unread axis reads the row, not the counter — and
                // the per-user counter has to follow the same flip, or
                // marking unread would light the axis while this user's
                // count still said zero (RFC 20260730 S1).
                ctx.hset(
                    keys::thread_user(user, thread_id).as_bytes(),
                    &[(b"unread" as &[u8], b"1" as &[u8]), (b"unread_count", b"1")],
                )?;
                Ok(true)
            })
            .map_err(std::io::Error::from)
    }

    /// Put a thread away until an epoch second, or `0` to bring it
    /// back.
    ///
    /// On **this user's** membership row, not the shared thread hash.
    /// It used to be the hash, which had two consequences: putting a
    /// thread away did it for everyone who could see it, and — because
    /// nothing anywhere parsed the field — it did it for nobody. The
    /// list read now filters `snoozed_until <= now` off the declared
    /// column, so a thread comes back when its time passes with
    /// nothing sweeping it awake.
    ///
    /// Returns `true` when the row existed.
    pub fn set_snoozed(&self, user: &str, thread_id: &str, snoozed_until: i64) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let val = snoozed_until.to_string();
        // Away is `archived`, and the time is beside it.
        //
        // Not a predicate of its own on the list read: the page comes
        // from an ORDERPATH and the total from `idx_count`, and only
        // the page can carry a FILTER — a snooze predicate on one and
        // not the other is a list and a badge that disagree, which is
        // the failure `scoped_clause` exists to prevent. `archived` is
        // an equality column both already share, so every axis, count
        // and cursor keeps working with nothing new to keep in step.
        let away = snoozed_until > 0;
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(key.as_bytes(), b"tid")? {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"snoozed_until" as &[u8], val.as_bytes()),
                        (b"archived", crate::thread_row::flag(away)),
                    ],
                )?;
                Ok(true)
            })
            .map_err(std::io::Error::from)
    }

    /// Bring back every thread whose time has come.
    ///
    /// Returns how many woke — a number that is 0 on the overwhelming
    /// majority of calls, and costs an empty range scan to produce:
    /// the index is keyed on `snoozed_until`, every ordinary row
    /// stores `0`, and this asks for `[1, now]`. Nothing is written
    /// when nothing is due, which is what
    /// `periodic-work-must-converge` asks of anything on a timer.
    pub fn wake_snoozed(&self, now: i64) -> io::Result<usize> {
        let (rows, _) = self.store().idx_query(
            b"threaduser.snoozed_until",
            &kevy_embedded::IndexValue::I64(1),
            &kevy_embedded::IndexValue::I64(now),
            None,
            500,
        )?;
        let mut woken = 0usize;
        for (key, _) in rows {
            // not a KevyError, so it does not go through the engine's
            // ErrorKind mapping — this one stays an explicit `other`.
            let key = String::from_utf8(key).map_err(std::io::Error::other)?;
            let changed = self.store().atomic(|ctx| {
                // Re-read inside the transaction: the row may have
                // been unsnoozed, or snoozed further out, between
                // the query and here.
                let due = ctx
                    .hget(key.as_bytes(), b"snoozed_until")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                if due == 0 || due > now {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"snoozed_until" as &[u8], b"0" as &[u8]),
                        (b"archived", b"0"),
                    ],
                )?;
                Ok(true)
            })?;
            if changed {
                woken += 1;
            }
        }
        Ok(woken)
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
            // The category used to pick which per-category zset to
            // clean; all this asks now is whether the thread is
            // there at all.
            if !ctx.hexists(thread_key.as_bytes(), b"category")? {
                return Ok(false);
            }

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

            // The membership row is the thread's presence on every
            // axis the table serves, so deleting the thread has to
            // delete it too — otherwise the row outlives the data
            // and every axis keeps listing a thread that is gone.
            //
            // This replaced a twelve-key zrem sweep: one row now
            // carries what twelve indexes used to, and the list of
            // keys to remember to clean out is gone with them. That
            // list had been wrong twice — the folder zsets were
            // missing from it until v2.8.2, then the two new
            // buckets until v2.9, each time leaving orphans behind
            // on every delete.
            ctx.del(&[keys::thread_user(user, thread_id).as_bytes()]);
            // And this user's index of the thread's messages. It is a
            // separate key from the shared one above and was not in this
            // list, so a deleted thread left one behind — the same
            // omission, in the same key, that made a merge lose half a
            // conversation.
            ctx.del(&[keys::thread_user_messages(user, thread_id).as_bytes()]);
            Ok(true)
        })?;
        Ok((existed, blob_refs))
    }
}

impl KevyMailboxStore {
    /// Set a thread's display date.
    ///
    /// Goes through `upsert_thread` rather than writing the two fields
    /// directly, and that is the whole point. The shared hash calls
    /// this `latest_date` and the membership row calls it `activity`,
    /// but the row belongs to a **declared table**: its ordering is
    /// derived from the row when the row is written the way the table
    /// expects. Poking `activity` with a bare `hset` updated the field
    /// — `get_thread` and a by-id lookup both reported the new value —
    /// and left the index sorting on the old one, so the conversation
    /// list went on showing the date and the position that had just
    /// been repaired.
    ///
    /// Returns whether anything changed, so a sweep with nothing to do
    /// does nothing.
    pub fn set_thread_display_date(
        &self,
        user: &str,
        thread_id: &str,
        date: i64,
    ) -> io::Result<bool> {
        let Some(mut row) = self.get_thread(thread_id)? else {
            return Ok(false);
        };
        if row.latest_date == date {
            return Ok(false);
        }
        row.latest_date = date;
        self.upsert_thread(user, &row)?;
        Ok(true)
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

    /// Count on the axis the UI queries, not on a raw index.
    fn axis_count(s: &KevyMailboxStore, user: &str, f: crate::ListThreadsFilter<'_>) -> usize {
        s.list_threads_by_activity(user, &f, 0, 1000).unwrap().1
    }

    #[test]
    fn set_archived_round_trip() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();

        assert!(s.set_archived(u, "t1", true).unwrap());
        // Read back from the row that owns the fact, not the shared
        // aggregate — which has no user segment and no longer carries it.
        assert!(s.get_thread_for_user(u, "t1").unwrap().unwrap().archived);
        let archived = |s: &KevyMailboxStore| {
            axis_count(
                s,
                u,
                crate::ListThreadsFilter {
                    archived: true,
                    ..Default::default()
                },
            )
        };
        assert_eq!(archived(&s), 1);

        assert!(s.set_archived(u, "t1", false).unwrap());
        assert!(!s.get_thread("t1").unwrap().unwrap().archived);
        assert_eq!(archived(&s), 0);
    }

    #[test]
    fn set_pinned_shows_up_on_the_pinned_axis() {
        let s = store();
        // the flag axes are served from the declared table
        s.ensure_thread_table();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        assert!(s.set_pinned(u, "t1", true).unwrap());
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
    fn delete_thread_clears_every_axis() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        s.set_pinned(u, "t1", true).unwrap();
        s.set_has_action(u, "t1", true).unwrap();
        // A second thread for the archived axis. It cannot be `t1`: since
        // 2026-08-05 every list but Archived excludes archived threads, so
        // one thread cannot be on the archived axis and the other four at
        // once — and a test that wants "on every axis" has to use two.
        s.record_message_arrival(&arr("t2", u)).unwrap();
        s.set_archived(u, "t2", true).unwrap();

        // the threads are on every axis now
        let on_axes = |s: &KevyMailboxStore| {
            [
                crate::ListThreadsFilter {
                    pinned: true,
                    ..Default::default()
                },
                crate::ListThreadsFilter {
                    archived: true,
                    ..Default::default()
                },
                crate::ListThreadsFilter {
                    has_action: true,
                    ..Default::default()
                },
                crate::ListThreadsFilter {
                    folder: Some("Inbox"),
                    ..Default::default()
                },
                crate::ListThreadsFilter::default(),
            ]
            .into_iter()
            .map(|f| axis_count(s, u, f))
            .collect::<Vec<_>>()
        };
        assert_eq!(on_axes(&s), vec![1, 1, 1, 1, 1]);

        let (existed, blob_refs) = s.delete_thread(u, "t1").unwrap();
        assert!(existed);
        // record_message_arrival did not write a message blob, so the
        // returned blob_ref list is empty — this test covers the
        // thread-hash + index cleanup half only. A dedicated
        // messages-and-files test would seed upsert_message first.
        assert!(blob_refs.is_empty());
        assert!(s.get_thread("t1").unwrap().is_none());
        assert!(s.delete_thread(u, "t2").unwrap().0);

        // Every axis again, this time through the same queries as
        // above. This used to assert `zcard == 0` over the nine legacy
        // zsets, which passes for a thread that was never on them —
        // and since nothing writes those keys any more, it passed for
        // every thread, deleted or not.
        assert_eq!(on_axes(&s), vec![0, 0, 0, 0, 0]);
        // The membership row is what carries all five, so its absence
        // is the fact the axes are derived from.
        assert!(
            s.store()
                .hgetall(keys::thread_user(u, "t1").as_bytes())
                .unwrap()
                .is_empty(),
            "the membership row outlived the thread"
        );
    }

    /// The thread's messages, from the index the mailbox reads.
    ///
    /// The membership row above was the *shared* half of the delete. This
    /// user's own index of the thread's messages is a separate key, and it
    /// was not on the list — so a deleted thread left one behind. The same
    /// key, and the same omission, that made a merge lose half a
    /// conversation.
    #[test]
    fn deleting_a_thread_takes_this_users_index_of_its_messages() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        s.upsert_user_message(
            u,
            "t1",
            "m-1",
            100,
            br#"{"message_id":"m-1","thread_id":"t1"}"#,
            &crate::UserMessageFacts {
                blob_ref: "m-1.host",
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
        assert_eq!(s.user_thread_message_ids(u, "t1").unwrap().len(), 1);

        assert!(s.delete_thread(u, "t1").unwrap().0);
        assert!(
            s.user_thread_message_ids(u, "t1").unwrap().is_empty(),
            "the per-user message index outlived the thread"
        );
    }

    /// Filing an archived thread has to take it out of Archived —
    /// otherwise the move lands it in the new bucket and leaves it in
    /// the tab it was moved from, so nothing appears to happen.
    #[test]
    fn filing_an_archived_thread_unarchives_it() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        s.set_archived(u, "t1", true).unwrap();
        assert!(s.get_thread_for_user(u, "t1").unwrap().unwrap().archived);

        let archived_count = |s: &KevyMailboxStore| {
            axis_count(
                s,
                u,
                crate::ListThreadsFilter {
                    archived: true,
                    ..Default::default()
                },
            )
        };
        assert_eq!(archived_count(&s), 1);

        assert!(s.set_bucket(u, "t1", keys::Bucket::Notifications).unwrap());
        assert!(
            !s.get_thread("t1").unwrap().unwrap().archived,
            "the thread must no longer be archived"
        );
        assert_eq!(archived_count(&s), 0, "and must leave the Archived axis");

        let in_notifications = axis_count(
            &s,
            u,
            crate::ListThreadsFilter {
                folder: Some("Notifications"),
                ..Default::default()
            },
        );
        assert_eq!(in_notifications, 1, "while arriving in its new bucket");
    }

    #[test]
    fn set_bucket_migrates_between_all_four_buckets() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap(); // inbound → Inbox

        // Where the four bucket axes say the thread is. Exactly one
        // must claim it at every step — that is the invariant the four
        // separate zsets used to encode by hand.
        let where_is_it = |s: &KevyMailboxStore| -> Vec<&'static str> {
            ["Inbox", "Notifications", "Promotions", "Junk"]
                .into_iter()
                .filter(|folder| {
                    let f = crate::ListThreadsFilter {
                        folder: Some(folder),
                        ..Default::default()
                    };
                    axis_count(s, u, f) == 1
                })
                .collect()
        };

        assert_eq!(where_is_it(&s), vec!["Inbox"]);

        assert!(s.set_bucket(u, "t1", keys::Bucket::Promotions).unwrap());
        assert_eq!(where_is_it(&s), vec!["Promotions"]);
        assert_eq!(s.get_thread("t1").unwrap().unwrap().category, "promotion");

        assert!(s.set_bucket(u, "t1", keys::Bucket::Notifications).unwrap());
        assert_eq!(where_is_it(&s), vec!["Notifications"]);

        // via the set_junk back-compat wrapper
        assert!(s.set_junk(u, "t1", true).unwrap());
        assert_eq!(where_is_it(&s), vec!["Junk"]);

        assert!(s.set_bucket(u, "t1", keys::Bucket::Inbox).unwrap());
        assert_eq!(where_is_it(&s), vec!["Inbox"]);
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

    /// Snoozing takes a thread out of the inbox, and time puts it back.
    ///
    /// Before this, `set_snoozed` wrote `snoozed_until` to the shared
    /// thread hash and nothing anywhere parsed it: the web's snooze
    /// dropped the row optimistically and the next refetch brought it
    /// straight back. The feature had never done anything, on either
    /// client, and every layer looked right on its own.
    #[test]
    fn a_snoozed_thread_leaves_the_inbox_and_comes_back_by_itself() {
        let s = store();
        let u = "alice@x.com";
        s.record_message_arrival(&MessageArrival {
            thread_id: "t-nap",
            user: u,
            subject: "Later",
            senders_csv: "bob@y.com",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
        let inbox = || {
            s.list_thread_ids_by_bucket_via_table(
                u,
                "inbox",
                crate::table_query::ArchiveScope::Live,
                50,
            )
            .unwrap()
        };
        assert_eq!(
            inbox(),
            vec!["t-nap".to_string()],
            "not in the inbox to begin with"
        );

        assert!(s.set_snoozed(u, "t-nap", 5_000).unwrap());
        assert!(inbox().is_empty(), "snoozed and still in the inbox");

        // Nothing is due yet, and asking costs no writes.
        assert_eq!(s.wake_snoozed(4_999).unwrap(), 0);
        assert!(inbox().is_empty(), "woke early");

        assert_eq!(
            s.wake_snoozed(5_000).unwrap(),
            1,
            "the due row did not wake"
        );
        assert_eq!(
            inbox(),
            vec!["t-nap".to_string()],
            "woke, but not back in the inbox"
        );
        // And waking is idempotent: a second sweep has nothing to do.
        assert_eq!(s.wake_snoozed(9_999).unwrap(), 0);
    }

    /// The count and the list answer the same question.
    ///
    /// They are built from one equality prefix precisely so they
    /// cannot disagree; a snooze predicate that applied to the page
    /// and not to `idx_count` would be a list with nothing in it above
    /// a badge that says three.
    #[test]
    fn the_bucket_count_follows_the_list_when_a_thread_naps() {
        let s = store();
        let u = "alice@x.com";
        for (i, tid) in ["t1", "t2", "t3"].iter().enumerate() {
            s.record_message_arrival(&MessageArrival {
                thread_id: tid,
                user: u,
                subject: "s",
                senders_csv: "bob@y.com",
                latest_date: 100 + i as i64,
                latest_preview: "p",
                category: "inbox",
                unread: true,
                is_own: false,
            })
            .unwrap();
        }
        let count = || {
            s.count_thread_ids_by_bucket_via_table(
                u,
                "inbox",
                crate::table_query::ArchiveScope::Live,
            )
            .unwrap()
        };
        assert_eq!(count(), 3);
        s.set_snoozed(u, "t2", 5_000).unwrap();
        assert_eq!(
            count(),
            2,
            "the badge still counts a thread the list will not show"
        );
        s.wake_snoozed(5_000).unwrap();
        assert_eq!(count(), 3);
    }

    /// Unsnoozing by hand is setting the time to zero, and it must put
    /// the thread back rather than leaving it filed away for good.
    #[test]
    fn unsnoozing_returns_the_thread() {
        let s = store();
        let u = "alice@x.com";
        s.record_message_arrival(&MessageArrival {
            thread_id: "t-nap",
            user: u,
            subject: "s",
            senders_csv: "bob@y.com",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: false,
            is_own: false,
        })
        .unwrap();
        s.set_snoozed(u, "t-nap", 5_000).unwrap();
        assert!(s.set_snoozed(u, "t-nap", 0).unwrap());
        assert_eq!(
            s.list_thread_ids_by_bucket_via_table(
                u,
                "inbox",
                crate::table_query::ArchiveScope::Live,
                50
            )
            .unwrap(),
            vec!["t-nap".to_string()]
        );
    }

    /// A thread nobody has a row for cannot be snoozed, and says so
    /// rather than creating one.
    #[test]
    fn snoozing_a_thread_with_no_row_reports_it() {
        let s = store();
        assert!(!s.set_snoozed("alice@x.com", "nope", 5_000).unwrap());
    }

    /// A snooze survives the next message.
    ///
    /// `upsert_thread` derives the membership row from the shared
    /// aggregate, which has no user segment — when it emitted the
    /// per-user flags, an arrival for one owner rewrote every owner's
    /// state with whatever the last one had. `starred` was fixed that
    /// way and `snoozed_until` joins it: mail arriving in a thread
    /// somebody put away must not drag it back out.
    #[test]
    fn an_arrival_does_not_cancel_a_snooze() {
        let s = store();
        let u = "alice@x.com";
        let arrive = |date: i64| {
            s.record_message_arrival(&MessageArrival {
                thread_id: "t-nap",
                user: u,
                subject: "s",
                senders_csv: "bob@y.com",
                latest_date: date,
                latest_preview: "p",
                category: "inbox",
                unread: true,
                is_own: false,
            })
            .unwrap();
        };
        arrive(100);
        s.set_snoozed(u, "t-nap", 5_000).unwrap();
        arrive(200);

        let row = s.get_thread_for_user(u, "t-nap").unwrap().expect("row");
        assert_eq!(row.snoozed_until, 5_000, "the arrival cancelled the snooze");
        assert!(
            s.list_thread_ids_by_bucket_via_table(
                u,
                "inbox",
                crate::table_query::ArchiveScope::Live,
                50
            )
            .unwrap()
            .is_empty(),
            "the arrival dragged a put-away thread back into the inbox"
        );
    }
}
