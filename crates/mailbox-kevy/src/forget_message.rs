//! Taking one message out of one user's mailbox, when its file is gone.
//!
//! An IMAP EXPUNGE or a POP3 DELE removes a file; the index has to follow,
//! or the conversation goes on listing a message nobody can open. The SQL
//! core does this by deleting the row, and so does this.

use std::collections::HashSet;
use std::io;

use super::KevyMailboxStore;
use super::keys;
use crate::thread_ownership::file_base;

/// What removing a file did to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forgotten {
    /// The user has no row for the message: nothing to do.
    NoCopy,
    /// Another file of the same message is still on disk; the row now
    /// names it.
    OtherCopy,
    /// The message left the conversation, which still has others.
    Message { thread_id: String },
    /// It was the conversation's last message for the user, so the
    /// conversation went with it.
    Thread { thread_id: String },
}

impl KevyMailboxStore {
    /// `file` (a `blob_ref`, with or without flags) of `user`'s copy of
    /// `message_id` is gone from disk. When no other file of the copy is
    /// (`on_disk` answers for each), the copy leaves the index; otherwise
    /// the row is pointed at one that is.
    pub fn forget_message_file(
        &self,
        user: &str,
        message_id: &str,
        file: &str,
        on_disk: impl Fn(&str) -> bool,
    ) -> io::Result<Forgotten> {
        let Some(facts) = self.user_message_facts(user, message_id)? else {
            return Ok(Forgotten::NoCopy);
        };
        let gone = file_base(file);
        let files_key = keys::user_message_files(user, message_id);
        let mut names: Vec<String> = self
            .store()
            .smembers(files_key.as_bytes())?
            .into_iter()
            .filter_map(|n| String::from_utf8(n).ok())
            .collect();
        if names.is_empty() {
            names.push(file_base(&facts.blob_ref).to_string());
        }
        let left: Vec<String> = names
            .into_iter()
            .filter(|n| n != gone && on_disk(n))
            .collect();
        if let Some(keep) = left.first() {
            let row = keys::user_message(user, message_id);
            let repoint = file_base(&facts.blob_ref) == gone;
            self.store()
                .atomic(|ctx| {
                    ctx.srem(files_key.as_bytes(), &[gone.as_bytes()])?;
                    if repoint {
                        ctx.hset(row.as_bytes(), &[(b"blob_ref" as &[u8], keep.as_bytes())])?;
                    }
                    Ok(())
                })
                .map_err(io::Error::from)?;
            return Ok(Forgotten::OtherCopy);
        }
        let mut thread_id = facts.tid.clone();
        if thread_id.is_empty() {
            thread_id = self
                .get_message(message_id)?
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|w| w["thread_id"].as_str().map(String::from))
                .unwrap_or_default();
        }
        self.forget_user_message(user, message_id, &thread_id, facts.uid)
    }

    /// Remove `user`'s copy of `message_id` from `thread_id`: their row and
    /// every per-user key of it, and the shared records when no other
    /// owner holds the message. The conversation's row is recounted; when
    /// nothing of the user's is left in it, the conversation is deleted
    /// for them as a whole.
    fn forget_user_message(
        &self,
        user: &str,
        message_id: &str,
        thread_id: &str,
        uid: u32,
    ) -> io::Result<Forgotten> {
        let mine = self.user_thread_message_ids(user, thread_id)?;
        if !mine.iter().any(|m| m != message_id) {
            self.delete_thread(user, thread_id)?;
            return Ok(Forgotten::Thread {
                thread_id: thread_id.to_string(),
            });
        }
        let held_by_others: HashSet<String> = self
            .thread_message_owners(thread_id)?
            .into_iter()
            .filter(|(owner, _)| owner != user)
            .map(|(_, mid)| mid)
            .collect();
        let shared_too = !held_by_others.contains(message_id);
        let mut text_keys = vec![keys::message_text(message_id)];
        text_keys.extend(self.attachment_text_keys(message_id)?);
        let row = keys::user_message(user, message_id);
        let uid_s = uid.to_string();
        self.store()
            .atomic(|ctx| {
                ctx.del(&[row.as_bytes()]);
                ctx.del(&[keys::user_message_files(user, message_id).as_bytes()]);
                ctx.del(&[keys::message_by_message_id(user, message_id).as_bytes()]);
                if uid != 0 {
                    ctx.hdel(keys::user_msg_by_uid(user).as_bytes(), &[uid_s.as_bytes()])?;
                }
                ctx.hdel(
                    keys::user_uid_by_mid(user).as_bytes(),
                    &[message_id.as_bytes()],
                )?;
                ctx.zrem(
                    keys::thread_user_messages(user, thread_id).as_bytes(),
                    &[message_id.as_bytes()],
                )?;
                if shared_too {
                    ctx.del(&[keys::message_blob(message_id).as_bytes()]);
                    ctx.zrem(
                        keys::thread_messages(thread_id).as_bytes(),
                        &[message_id.as_bytes()],
                    )?;
                    for k in &text_keys {
                        ctx.del(&[k.as_bytes()]);
                    }
                }
                Ok(())
            })
            .map_err(io::Error::from)?;
        self.delete_ledger(user, message_id)?;
        self.refresh_thread_after_removal(user, thread_id)?;
        Ok(Forgotten::Message {
            thread_id: thread_id.to_string(),
        })
    }

    /// The conversation's row, as what is left in it says: the counts, and
    /// the date and subject of the newest message the user did not send.
    fn refresh_thread_after_removal(&self, user: &str, thread_id: &str) -> io::Result<()> {
        let Some(mut row) = self.get_thread(thread_id)? else {
            return Ok(());
        };
        if let Some((count, unread, sent)) = self.recount_from_messages(user, thread_id)? {
            row.count = count;
            row.unread_count = unread;
            row.sent_count = sent;
        }
        let msgs = self.thread_messages_unscoped(thread_id)?;
        if let Some(w) = crate::thread_row::display_message(&msgs, user) {
            row.latest_date = w["internal_date"].as_i64().unwrap_or(row.latest_date);
            row.subject = w["subject"].as_str().unwrap_or(&row.subject).to_string();
        }
        self.upsert_thread(user, &row)?;
        self.set_unread_axis(user, thread_id, row.unread_count)
    }
}
