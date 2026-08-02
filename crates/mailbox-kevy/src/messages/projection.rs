//! The per-user projection: one shared blob plus one row per owner,
//! and the reads that overlay the two.

use std::io;

use super::KevyMailboxStore;
use super::keys;

use super::{OwnedUserMessageFacts, UserMessageFacts, overlay_user_facts, strip_per_user_fields};

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
        // Stage 5: the shared blob stops carrying the per-user fields.
        //
        // They are cleared rather than left at the caller's values, because a
        // shared row naming one owner is what served 74 messages on
        // production a `blob_ref` in somebody else's mailbox. Old rows keep
        // theirs, but nothing new records a per-user fact in a place several
        // users read — and since 2026-08-01 nothing reads those leftovers
        // either: `user_message_view` is the one decision, and it answers
        // "no copy" rather than handing back another owner's.
        let shared_payload = strip_per_user_fields(payload);
        self.store()
            .atomic(|ctx| {
                ctx.set(blob_key.as_bytes(), &shared_payload);
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

    /// Clear the per-user fields off the shared blobs of one thread —
    /// stage 6 of the per-user message projection.
    ///
    /// Stage 5 stopped *writing* them; rows written before it keep one
    /// owner's `blob_ref`, `uid`, `flags` and `modseq` on a row several
    /// users read. Nothing serves those any more — `user_message_view` is
    /// the single decision and it answers "no copy" — so this is not a
    /// repair. It is removing the thing a future fallback could reach for:
    /// on production 326 shared rows still name a file, and none of those
    /// files resolve for the user asking (`maintenance:usermsg-shadow`,
    /// `shared_resolves: 0`).
    ///
    /// Returns `(messages_seen, rewritten)`. The write is conditional on the
    /// blob actually changing, so a second pass over a stripped thread does
    /// no writes and reports none — a sweep that redoes its work every cycle
    /// is a busy-wait, not a sweep.
    pub fn strip_shared_per_user_fields(&self, thread_id: &str) -> io::Result<(u64, u64)> {
        let mut seen = 0u64;
        let mut rewritten = 0u64;
        for blob in self.thread_messages_for_maintenance(thread_id)? {
            seen += 1;
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&blob) else {
                continue;
            };
            let Some(mid) = v.get("message_id").and_then(|m| m.as_str()) else {
                continue;
            };
            let stripped = strip_per_user_fields(&blob);
            if stripped == blob {
                continue;
            }
            self.store()
                .set(keys::message_blob(mid).as_bytes(), &stripped)
                .map_err(io::Error::other)?;
            rewritten += 1;
        }
        Ok((seen, rewritten))
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

    /// One user's view of one message, or `None` if they have no copy.
    ///
    /// **The** place that decides what a missing per-user row means, because
    /// the two readers that decided it separately disagreed: the thread
    /// listing dropped the message and the uid fetch served the shared row.
    /// One of them had to be wrong, and a reader that answers a question
    /// about ownership differently from its neighbour is the shape that put
    /// somebody else's `blob_ref` in front of 74 messages to begin with.
    ///
    /// The answer is `None`. Since stage 5 the shared row does not carry
    /// per-user fields at all, so falling back to it yields either a blank
    /// `blob_ref` (new rows, stripped) or whichever owner wrote last (old
    /// rows) — neither is this user's copy, and both are worse than saying
    /// there isn't one.
    ///
    /// Every message on production resolves (`only_in_shared: 0`,
    /// `maintenance:usermsg-shadow`, 2026-08-01), so this returning `None`
    /// means an invariant broke rather than a backfill lagging — hence the
    /// warning, which is the only way anyone would find out.
    pub fn user_message_view(&self, user: &str, message_id: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(shared) = self.get_message(message_id)? else {
            return Ok(None);
        };
        match self.user_message_facts(user, message_id)? {
            Some(facts) => Ok(Some(overlay_user_facts(&shared, &facts))),
            None => {
                tracing::warn!(
                    %user,
                    %message_id,
                    "message has no per-user row; not served (run maintenance:backfill-user-messages)"
                );
                Ok(None)
            }
        }
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
}

#[cfg(test)]
mod tests {
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

        // One shared blob underneath, holding what is genuinely shared and
        // nothing that depends on who is asking. Stage 5 blanks the
        // per-user fields on the way in, so the row cannot name one owner
        // while several read it.
        let shared: serde_json::Value =
            serde_json::from_slice(&s.get_message(mid).expect("get").expect("present"))
                .expect("json");
        assert_eq!(shared["subject"], "Meeting");
        assert_eq!(shared["blob_ref"], "");
        assert_eq!(shared["uid"], 0);
        assert_eq!(shared["user_address"], "");
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

    /// The behaviour the whole projection exists for.
    ///
    /// Two owners of one thread read the same mail and each gets their own
    /// `blob_ref`, `uid` and `flags` — the fields that decide which file is
    /// opened, what IMAP calls it, and whether it shows as read. Before the
    /// cutover both were served one owner's, and on production that left 74
    /// messages naming a file in somebody else's mailbox, none of which
    /// resolved for the user reading them.
    #[test]
    fn each_owner_reads_the_thread_with_their_own_facts() {
        let s = store();
        let mid = "<m1@example.com>";
        // Both own the thread — membership is what `list_thread_messages`
        // gates on.
        for u in ["lihao@golia.jp", "devops@golia.jp"] {
            s.record_message_arrival(&crate::MessageArrival {
                thread_id: "t1",
                user: u,
                subject: "Meeting",
                senders_csv: "someone@x.com",
                latest_date: 100,
                latest_preview: "",
                category: "inbox",
                unread: true,
                is_own: false,
            })
            .expect("membership");
        }
        s.upsert_user_message(
            "lihao@golia.jp",
            "t1",
            mid,
            100,
            br#"{"subject":"Meeting","blob_ref":"shared.host","uid":1,"flags":0,"modseq":0}"#,
            &UserMessageFacts {
                blob_ref: "lihao-copy.host",
                uid: 7,
                flags: 1,
                modseq: 9,
            },
        )
        .expect("lihao");
        s.upsert_user_message(
            "devops@golia.jp",
            "t1",
            mid,
            100,
            br#"{"subject":"Meeting","blob_ref":"shared.host","uid":1,"flags":0,"modseq":0}"#,
            &UserMessageFacts {
                blob_ref: "devops-copy.host",
                uid: 3,
                flags: 0,
                modseq: 4,
            },
        )
        .expect("devops");

        let read = |u: &str| -> serde_json::Value {
            let msgs = s.list_thread_messages(u, "t1").expect("read");
            assert_eq!(msgs.len(), 1, "{u} sees one message");
            serde_json::from_slice(&msgs[0]).expect("json")
        };

        let lihao = read("lihao@golia.jp");
        let devops = read("devops@golia.jp");

        // The shared content is the same for both.
        assert_eq!(lihao["subject"], "Meeting");
        assert_eq!(devops["subject"], "Meeting");
        // The per-user facts are not, and neither is the shared blob's.
        assert_eq!(lihao["blob_ref"], "lihao-copy.host");
        assert_eq!(devops["blob_ref"], "devops-copy.host");
        assert_eq!(lihao["uid"], 7);
        assert_eq!(devops["uid"], 3);
        // Read state is per user: maildir encodes \Seen in the filename, so
        // sharing this would share one owner's read state with the other.
        assert_eq!(lihao["flags"], 1);
        assert_eq!(devops["flags"], 0);
    }

    /// A message one owner never received is not in the other's thread,
    /// through the real read path.
    #[test]
    fn the_read_path_does_not_serve_a_message_the_user_has_no_copy_of() {
        let s = store();
        for u in ["lihao@golia.jp", "devops@golia.jp"] {
            s.record_message_arrival(&crate::MessageArrival {
                thread_id: "t1",
                user: u,
                subject: "Meeting",
                senders_csv: "someone@x.com",
                latest_date: 100,
                latest_preview: "",
                category: "inbox",
                unread: true,
                is_own: false,
            })
            .expect("membership");
        }
        s.upsert_user_message(
            "lihao@golia.jp",
            "t1",
            "<only-lihao@x>",
            200,
            b"{}",
            &UserMessageFacts {
                blob_ref: "a.host",
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("write");

        assert_eq!(
            s.list_thread_messages("lihao@golia.jp", "t1")
                .expect("read")
                .len(),
            1
        );
        assert!(
            s.list_thread_messages("devops@golia.jp", "t1")
                .expect("read")
                .is_empty(),
            "devops never received it and must not be served it"
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
