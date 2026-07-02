//! KV key helpers — every key the kevy backend reads or writes.
//!
//! Single source of truth. Per-method implementations call these instead
//! of writing literal `format!` strings so renames stay local.

/// Thread aggregate hash. Fields: subject, senders_csv, count,
/// unread_count, latest_date, latest_preview, category, importance_level,
/// importance_score, requires_action, pinned, archived, has_action,
/// sent_count.
pub fn thread(tid: &str) -> String {
    format!("mailrs:thread:{tid}")
}

/// Per-user activity index — zset(tid → max internal_date).
/// Used for `/v1/users/{u}/conversations:list` ordered by recency.
pub fn user_threads_by_activity(user: &str) -> String {
    format!("mailrs:user:{user}:threads:by_activity")
}

/// Per-user pinned subset.
pub fn user_threads_pinned(user: &str) -> String {
    format!("mailrs:user:{user}:threads:pinned")
}

/// Per-user archived subset.
pub fn user_threads_archived(user: &str) -> String {
    format!("mailrs:user:{user}:threads:archived")
}

/// Per-user category index.
pub fn user_threads_by_category(user: &str, category: &str) -> String {
    format!("mailrs:user:{user}:threads:by_category:{category}")
}

/// Per-user unread (excluding spam) subset — `count_unseen` reads this.
pub fn user_threads_has_unread(user: &str) -> String {
    format!("mailrs:user:{user}:threads:has_unread:non_spam")
}

/// Per-user "requires action" subset — `count_action_threads` reads this.
pub fn user_threads_has_action(user: &str) -> String {
    format!("mailrs:user:{user}:threads:has_action")
}

/// Per-user starred (flagged) subset. Score = latest_date for recency sort.
pub fn user_threads_starred(user: &str) -> String {
    format!("mailrs:user:{user}:threads:starred")
}

/// Per-user Sent-folder subset — threads with `sent_count > 0`. Same
/// shape/semantics as the other index zsets; score = latest_date.
pub fn user_threads_sent(user: &str) -> String {
    format!("mailrs:user:{user}:threads:sent")
}

/// Per-thread message index — zset member = message_id (RFC string),
/// score = internal_date (epoch seconds). One ZRANGE returns the
/// full message timeline in order.
pub fn thread_messages(thread_id: &str) -> String {
    format!("mailrs:thread:{thread_id}:messages")
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
/// the actual vector lives in meili arroy (Phase 7.7). Kept as a stable
/// indirection so meili can be swapped without changing wire layout.
pub fn analysis_embedding_ref(message_id: i64) -> String {
    format!("mailrs:analysis:{message_id}:embedding_ref")
}

/// Contact hash — per user × email.
pub fn contact(user: &str, email: &str) -> String {
    format!("mailrs:contact:{user}:{email}")
}

/// Outbound queue row (for sender split).
pub fn outbound(id: i64) -> String {
    format!("mailrs:outbound:{id}")
}

/// Outbound pending queue — sender claims with BRPOPLPUSH.
pub const OUTBOUND_PENDING: &str = "mailrs:outbound:pending";

/// Outbound inflight list — for stale recovery.
pub const OUTBOUND_INFLIGHT: &str = "mailrs:outbound:inflight";

/// Suppression set — sender consults before sending.
pub const OUTBOUND_SUPPRESSION: &str = "mailrs:outbound:suppression";

/// Account hash — one per user address. Fields mirror
/// AccountWithHashWire so the same payload round-trips through pg-dump.
pub fn account(address: &str) -> String {
    format!("mailrs:account:{address}")
}

/// Global account address index — set of all account addresses. Used
/// for admin/list_accounts and pg-dump reverse walks.
pub const ACCOUNT_INDEX: &str = "mailrs:accounts:index";

/// Effective permissions blob for a user — cached so login doesn't
/// need to re-compute the graph on every request.
pub fn account_permissions(address: &str) -> String {
    format!("mailrs:account:{address}:perms")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shapes_are_stable() {
        // Lock down a few representative shapes — if these change, any
        // existing kevy data on disk is invalidated.
        assert_eq!(thread("tid-abc"), "mailrs:thread:tid-abc");
        assert_eq!(
            user_threads_by_activity("u@x.com"),
            "mailrs:user:u@x.com:threads:by_activity"
        );
        assert_eq!(mailbox(7), "mailrs:mailbox:7");
        assert_eq!(mailbox_messages(7), "mailrs:mailbox:7:messages");
        assert_eq!(
            message_by_message_id("u@x.com", "abc@def.com"),
            "mailrs:message:by-message-id:u@x.com:abc@def.com"
        );
        assert_eq!(OUTBOUND_PENDING, "mailrs:outbound:pending");
    }
}
