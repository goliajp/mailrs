//! KV key helpers — every key the kevy backend reads or writes.
//!
//! Single source of truth. Per-method implementations call these instead
//! of writing literal `format!` strings so renames stay local.

mod admin;
mod threads;

pub use admin::*;
pub use threads::*;

/// Per-user uid → message_id index — hash where field=uid, value=message_id.
/// Populated by the deliver path + `mailrs-fastcore-backfill-uid-index`.
pub fn user_msg_by_uid(user: &str) -> String {
    format!("mailrs:user:{user}:msg_by_uid")
}

/// Reverse index: `mailrs:user:<u>:uid_by_mid` — hash message_id → uid,
/// so a rerun of self-heal can reuse the previously-allocated uid
/// instead of burning a fresh one every tick. Preserves IMAP UIDVALIDITY
/// even when the fastcore process restarts.
pub fn user_uid_by_mid(user: &str) -> String {
    format!("mailrs:user:{user}:uid_by_mid")
}

/// String counter: `mailrs:user:<u>:next_uid` — monotonic uid allocator.
/// `INCR` returns the next uid to assign; caller must persist the
/// mapping via [`user_uid_by_mid`] + [`user_msg_by_uid`] afterwards.
pub fn user_next_uid(user: &str) -> String {
    format!("mailrs:user:{user}:next_uid")
}

// v2.6.2 §P6 legacy drop: the legacy `mailrs:alias:<addr>` /
// `mailrs:domain:<name>` string keys and their companion
// `mailrs:{aliases,domains}:index` sets are gone. The v2 hash
// keyspace + range indexes below are canonical. See RFC
// 20260709-v2.3-p6-admin-crud-idx-query.md for the migration path.

/// Per-thread message index — zset member = message_id (RFC string),
/// score = internal_date (epoch seconds). One ZRANGE returns the
/// full message timeline in order.
pub fn thread_messages(thread_id: &str) -> String {
    format!("mailrs:thread:{thread_id}:messages")
}

/// Per-user facts about one message — `blob_ref`, `uid`, `flags`, `modseq`.
///
/// The message equivalent of [`thread_user`], and for the same reason it
/// gives: a thread is multi-owner, so anything true of one owner and not
/// another belongs on a row of its own. Six such fields sat on the shared
/// `mailrs:msg:{mid}` blob until 2026-07-31, and on a multi-owner thread
/// each was whoever wrote last — measured, 74 messages served to a user the
/// row did not name. `blob_ref` was the only one that announced itself, by
/// naming a maildir file in somebody else's mailbox, so the body came back
/// empty. The other five were simply wrong.
///
/// See `.claude/rfcs/20260731-per-user-message-projection.md`.
pub fn user_message(user: &str, message_id: &str) -> String {
    format!("mailrs:usermsg:{user}:{message_id}")
}

/// The messages one user has in one thread — zset, score = internal_date,
/// member = message_id (RFC string).
///
/// [`thread_messages`] is shared, so every owner of a thread is served every
/// message in it whoever it was delivered to. A mailbox contains the mail
/// its owner received; this is that set.
///
/// **Not** under `mailrs:threaduser:{user}:` — [`all_thread_ids_for_user`]
/// enumerates a user's threads with a `mailrs:threaduser:{user}:*` wildcard
/// and strips the prefix, so a key of the form
/// `mailrs:threaduser:{user}:{tid}:messages` is returned as a thread whose
/// id is `{tid}:messages`. That first spelling doubled the multi-owner
/// count from 74 to 148 the moment the backfill wrote its rows, which is
/// how it was caught.
///
/// [`all_thread_ids_for_user`]: crate::KevyMailboxStore::all_thread_ids_for_user
pub fn thread_user_messages(user: &str, thread_id: &str) -> String {
    format!("mailrs:usermsgs:{user}:{thread_id}")
}

/// Per-message JSON blob — value is a serialized MessageWire.
/// HGET on the `wire` field returns the message; HSET on it overwrites.
pub fn message_blob(message_id: &str) -> String {
    format!("mailrs:msg:{message_id}")
}

/// Mailbox hash. Fields: name, user, uidvalidity, uidnext, highest_modseq.
pub fn mailbox(mailbox_id: i64) -> String {
    format!("mailrs:mailbox:{mailbox_id}")
}

/// Per-user mailbox name index — zset(name → mailbox_id).
pub fn user_mailboxes(user: &str) -> String {
    format!("mailrs:user:{user}:mailboxes")
}

/// Message hash — every column from the `messages` table.
pub fn message(message_id: i64) -> String {
    format!("mailrs:message:{message_id}")
}

/// Per-mailbox uid index — zset(uid → message_id). Used by IMAP FETCH /
/// COPY / EXPUNGE.
pub fn mailbox_messages(mailbox_id: i64) -> String {
    format!("mailrs:mailbox:{mailbox_id}:messages")
}

/// `Message-ID` header → message_id mapping (per user, to keep tenancy).
pub fn message_by_message_id(user: &str, message_id_header: &str) -> String {
    format!("mailrs:message:by-message-id:{user}:{message_id_header}")
}

/// Maildir id → message_id mapping (per user + mailbox).
pub fn message_by_maildir(user: &str, mailbox_name: &str, maildir_id: &str) -> String {
    format!("mailrs:message:by-maildir:{user}:{mailbox_name}:{maildir_id}")
}

/// `email_analysis` row sidecar — `mailrs:analysis:<message_id>` hash.
pub fn analysis(message_id: i64) -> String {
    format!("mailrs:analysis:{message_id}")
}

/// Embedding bytes for semantic search — referenced by analysis row, but
/// the actual vector lives outside the row. Kept as a stable
/// indirection so the vector store can change without touching layout.
pub fn analysis_embedding_ref(message_id: i64) -> String {
    format!("mailrs:analysis:{message_id}:embedding_ref")
}

/// Contact hash — per user × email.
pub fn contact(user: &str, email: &str) -> String {
    format!("mailrs:contact:{user}:{email}")
}
