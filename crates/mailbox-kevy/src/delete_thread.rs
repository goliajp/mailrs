//! Deleting a conversation and everything keyed by its messages.

use std::collections::HashSet;
use std::io;

use super::KevyMailboxStore;
use super::keys;
use crate::thread_ownership::file_base;

impl KevyMailboxStore {
    /// Hard-delete `thread_id` for `user`. Removes the row hash + drops
    /// it from every index zset the row could be in. Idempotent: a
    /// re-call after deletion is a no-op returning false, and so is a
    /// call from somebody not in the thread.
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
        // Only somebody in the thread deletes their copy of it. Without
        // this a stranger who named a thread with no owner recorded was
        // its "last owner" and took the shared records with them, and
        // anybody's answer said whether a thread id existed.
        if !self.is_thread_participant(user, thread_id)? {
            return Ok((false, Vec::new()));
        }
        let thread_key = keys::thread(thread_id);
        let msgs_zset = keys::thread_messages(thread_id);
        let store = self.store();

        let per_msg = self.thread_message_files(user, thread_id)?;
        // The indexed text goes with the blobs — the body and whatever was
        // extracted from attachments — so a search cannot find a
        // conversation through a message that is no longer there. The
        // extraction state goes too, so a backfill can queue the message
        // again for a recipient who still has a copy.
        let mut text_keys: Vec<(String, Vec<String>)> = Vec::new();
        for (mid, _, _) in &per_msg {
            let mut keys_for = vec![keys::message_text(mid)];
            keys_for.extend(self.attachment_text_keys(mid)?);
            text_keys.push((mid.clone(), keys_for));
        }

        // No "hclear" in kevy 3.17; keep the explicit field list. Single
        // source of truth with the write path — a field written but not
        // listed here survives the delete and resurrects the row.
        let fields = crate::thread_row::ThreadRow::field_names();
        let msg_by_uid_key = keys::user_msg_by_uid(user);
        let uid_by_mid_key = keys::user_uid_by_mid(user);

        // Who else holds this thread, and which of its messages. The shared
        // records — the thread hash, its message index, each message's blob
        // and text — are theirs too, so they go only with the last owner.
        // Deleting them for everyone left the other owners a thread
        // self-heal had to rebuild, and handed the deleter's leftover files
        // the same rebuild. Read from the declared `tid` index, before the
        // transaction: the atomic context has no index query.
        let mut others: HashSet<String> = HashSet::new();
        let mut held_by_others: HashSet<String> = HashSet::new();
        for (owner, mid) in self.thread_message_owners(thread_id)? {
            if owner != user {
                others.insert(owner);
                held_by_others.insert(mid);
            }
        }

        let mut files: Vec<String> = Vec::new();
        let existed = store.atomic(|ctx| {
            // The category used to pick which per-category zset to
            // clean; all this asks now is whether the thread is
            // there at all.
            if !ctx.hexists(thread_key.as_bytes(), b"category")? {
                return Ok(false);
            }

            for (mid, uid, blob_ref) in &per_msg {
                // Every file this user's copy has had, not only the one the
                // row names: the same Message-ID can sit in Sent and INBOX,
                // or in INBOX once per alias it arrived through, and any
                // copy left on disk brings the conversation back.
                let files_key = keys::user_message_files(user, mid);
                for name in ctx.smembers(files_key.as_bytes())? {
                    if let Ok(name) = String::from_utf8(name) {
                        files.push(name);
                    }
                }
                if let Some(b) = blob_ref {
                    files.push(file_base(b).to_string());
                }
                ctx.del(&[files_key.as_bytes()]);
                ctx.del(&[keys::message_by_message_id(user, mid).as_bytes()]);
                // This user's own row about the message. It was missing
                // from this list and nothing complained, because nothing
                // reads such a row once the membership row above is gone —
                // they accumulated instead. A declared index that groups by
                // a column on these rows makes the leak load-bearing: the
                // orphan keeps its group, and a deleted conversation goes on
                // being counted.
                ctx.del(&[keys::user_message(user, mid).as_bytes()]);
                if let Some(u) = uid {
                    let uid_s = u.to_string();
                    ctx.hdel(msg_by_uid_key.as_bytes(), &[uid_s.as_bytes()])?;
                }
                ctx.hdel(uid_by_mid_key.as_bytes(), &[mid.as_bytes()])?;
                if held_by_others.contains(mid) {
                    continue;
                }
                ctx.del(&[keys::message_blob(mid).as_bytes()]);
                ctx.zrem(msgs_zset.as_bytes(), &[mid.as_bytes()])?;
            }

            for (mid, keys_for) in &text_keys {
                if held_by_others.contains(mid) {
                    continue;
                }
                for key in keys_for {
                    ctx.del(&[key.as_bytes()]);
                }
            }

            if others.is_empty() {
                // The thread's own message-index zset.
                ctx.del(&[msgs_zset.as_bytes()]);
                // Thread aggregate fields.
                ctx.hdel(thread_key.as_bytes(), fields)?;
            }

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
        files.sort();
        files.dedup();
        // and leaves every label of this user
        self.drop_thread_labels(user, thread_id)?;
        Ok((existed, files))
    }
}
