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

/// Key prefix the thread-search text index is declared over.
pub const THREAD_PREFIX: &[u8] = b"mailrs:thread:";

/// Name of the full-text index over [`THREAD_SEARCH_FIELD`].
pub const IDX_THREAD_SEARCH: &[u8] = b"mailrs_thread_search";

/// Synthesised hash field the text index reads. kevy indexes exactly
/// one field per text index, so subject / senders / preview are
/// concatenated into this one. Written by every path that writes the
/// row, and the index is maintained by kevy's commit hook — there is no
/// separate pipeline that can silently fall behind.
pub const THREAD_SEARCH_FIELD: &[u8] = b"search_blob";

/// Per-message body text, indexed for full-text search. Separate from
/// the thread row because a thread accumulates messages: folding every
/// body into the row's `search_blob` would grow one value without
/// bound, and rewrite all of it on each arrival.
pub fn message_text(message_id: &str) -> String {
    format!("mailrs:msgtext:{message_id}")
}

/// Key prefix the message-body text index is declared over.
pub const MSGTEXT_PREFIX: &[u8] = b"mailrs:msgtext:";

/// Name of the full-text index over message bodies.
pub const IDX_MESSAGE_TEXT: &[u8] = b"mailrs_message_text";

/// Indexed field on a `mailrs:msgtext:*` row.
pub const MESSAGE_TEXT_FIELD: &[u8] = b"body";

/// Companion field: which thread the message belongs to, so a body hit
/// resolves back to a conversation without a second lookup.
pub const MESSAGE_TEXT_TID_FIELD: &[u8] = b"tid";

/// Upper bound on indexed body text, in bytes. Mail runs to megabytes
/// once HTML and quoted history are counted, and indexing all of it
/// would multiply the AOF for diminishing recall — the terms that
/// identify a message are near its top. Truncation is on a char
/// boundary.
pub const MESSAGE_TEXT_CAP: usize = 8 * 1024;

/// Truncate `text` to [`MESSAGE_TEXT_CAP`] without splitting a char.
pub fn cap_message_text(text: &str) -> &str {
    if text.len() <= MESSAGE_TEXT_CAP {
        return text;
    }
    let mut end = MESSAGE_TEXT_CAP;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Build the value for [`THREAD_SEARCH_FIELD`].
pub fn search_blob(subject: &str, senders_csv: &str, preview: &str) -> String {
    let mut out = String::with_capacity(subject.len() + senders_csv.len() + preview.len() + 2);
    out.push_str(subject);
    out.push(' ');
    out.push_str(senders_csv);
    out.push(' ');
    out.push_str(preview);
    out
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

/// Per-user Junk-folder subset (v2.4.0 roadmap Phase 2, RFC-A). Threads
/// classified as junk mail (currently: `category ∈ {"spam", "scam"}`;
/// Phase 3 adds per-user blacklist + DMARC-quarantine + score-threshold
/// sources).
/// Same shape as the other index zsets: score = latest_date.
///
/// **Topology semantics:** Junk is a top-level folder (§D2), not an
/// inbox sub-category. On arrival a message enters exactly ONE of
/// {`user_threads_inbox`, `user_threads_junk`, `user_threads_sent`};
/// filtering by folder is an axis switch in `ListThreadsFilter`.
pub fn user_threads_junk(user: &str) -> String {
    format!("mailrs:user:{user}:threads:junk")
}

/// Per-user Inbox-folder subset (v2.4.0 roadmap Phase 2). Threads that
/// are not junk and not exclusively self-sent. Score = latest_date.
///
/// **Why a dedicated zset:** the existing `user_threads_by_activity`
/// key was written to for every arrival regardless of classification,
/// so junk threads leaked into "All"/"Inbox" list views. This zset
/// tracks the true Inbox membership so `folder=Inbox` is an axis
/// switch instead of a client-side subtraction.
pub fn user_threads_inbox(user: &str) -> String {
    format!("mailrs:user:{user}:threads:inbox")
}

/// Per-user Notifications-folder subset (v2.9 triage). Automated /
/// transactional mail (`category == "notification"`). Top-level
/// bucket, mutually exclusive with inbox/promotions/junk. Score =
/// latest_date.
pub fn user_threads_notifications(user: &str) -> String {
    format!("mailrs:user:{user}:threads:notifications")
}

/// Per-user Promotions-folder subset (v2.9 triage). Marketing / bulk
/// commercial mail (`category == "promotion"`). Top-level bucket,
/// mutually exclusive with inbox/notifications/junk. Score =
/// latest_date.
pub fn user_threads_promotions(user: &str) -> String {
    format!("mailrs:user:{user}:threads:promotions")
}

/// The triage bucket a thread belongs to. Exactly one of these holds a
/// non-sent-only thread at any time (Sent is an orthogonal axis; the
/// archived/starred/pinned flags are orthogonal too). The bucket is a
/// pure function of the thread's `category` field — see [`bucket_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Inbox,
    Notifications,
    Promotions,
    Junk,
}

impl Bucket {
    /// The folder zset key for this bucket.
    pub fn zset(self, user: &str) -> String {
        match self {
            Bucket::Inbox => user_threads_inbox(user),
            Bucket::Notifications => user_threads_notifications(user),
            Bucket::Promotions => user_threads_promotions(user),
            Bucket::Junk => user_threads_junk(user),
        }
    }

    /// The canonical `category` field value for this bucket, used when a
    /// mutation forces a bucket (`set_bucket`).
    pub fn category(self) -> &'static str {
        match self {
            Bucket::Inbox => "inbox",
            Bucket::Notifications => "notification",
            Bucket::Promotions => "promotion",
            Bucket::Junk => "spam",
        }
    }

    /// All four bucket zset keys — used to zrem a thread from the three
    /// it's leaving when moving into one, and by `delete_thread` cleanup.
    pub fn all_zsets(user: &str) -> [String; 4] {
        [
            user_threads_inbox(user),
            user_threads_notifications(user),
            user_threads_promotions(user),
            user_threads_junk(user),
        ]
    }
}

/// Map a thread's `category` string to its triage bucket. This is the
/// single source of truth for the bucket axis — every arrival/upsert
/// path derives folder membership through here so the "exactly one of
/// {inbox, notifications, promotions, junk}" invariant stays consistent.
pub fn bucket_of(category: &str) -> Bucket {
    if category.eq_ignore_ascii_case("spam") || category.eq_ignore_ascii_case("scam") {
        Bucket::Junk
    } else if category.eq_ignore_ascii_case("notification")
        || category.eq_ignore_ascii_case("notifications")
    {
        Bucket::Notifications
    } else if category.eq_ignore_ascii_case("promotion")
        || category.eq_ignore_ascii_case("promotions")
    {
        Bucket::Promotions
    } else {
        Bucket::Inbox
    }
}

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
/// `AccountWithHashWire` (blob) + the range-indexed
/// `{domain, active, created_at}` triple. Enumerated via
/// `accounts_by_active` (see below) — no separate set index.
pub fn account(address: &str) -> String {
    format!("mailrs:account:{address}")
}

/// Effective permissions blob for a user — cached so login doesn't
/// need to re-compute the graph on every request.
pub fn account_permissions(address: &str) -> String {
    format!("mailrs:account:{address}:perms")
}

// ── v2.6.0 §P6 dual-write keyspace ────────────────────────────────────
//
// The legacy admin-CRUD store pattern is `mailrs:{alias,domain}:<x>`
// string + `mailrs:{aliases,domains}:index` set; listing walks the set
// and issues per-key GETs (N+1 RTT). See RFC
// `20260709-v2.3-p6-admin-crud-idx-query.md` §1.
//
// Phase 9 (this commit) introduces a parallel `v2:` hash keyspace that
// the roadmap Phase 10 will switch reads to via `idx_query_range`, and
// Phase 11 will drop the legacy prefix. Write paths dual-populate both
// keyspaces; read paths still hit the legacy layout.
//
// `mailrs:account:<addr>` is already a hash (blob field), so account
// dual-write extends the SAME key with `{domain, active, created_at}`
// derived fields — no `v2:` sibling needed.

/// v2 alias hash key: `mailrs:alias:v2:<address>` — hash
/// `{target, domain, created_at, active}`.
pub fn alias_v2(address: &str) -> String {
    format!("mailrs:alias:v2:{address}")
}

/// v2 alias index prefix used by the RANGE indexes below.
pub const ALIAS_V2_PREFIX: &[u8] = b"mailrs:alias:v2:";

/// Range index over `mailrs:alias:v2:*`.domain — one RTT list-by-domain.
pub const IDX_ALIASES_BY_DOMAIN: &[u8] = b"aliases_by_domain";

/// Range index over `mailrs:alias:v2:*`.target — reverse lookup
/// (RFC §3.1: "who forwards TO this address?").
pub const IDX_ALIASES_BY_TARGET: &[u8] = b"aliases_by_target";

/// v2 domain hash key: `mailrs:domain:v2:<name>` — hash `{created_at}`.
pub fn domain_v2(name: &str) -> String {
    format!("mailrs:domain:v2:{name}")
}

/// v2 domain index prefix.
pub const DOMAIN_V2_PREFIX: &[u8] = b"mailrs:domain:v2:";

/// Range index over `mailrs:domain:v2:*`.created_at — one RTT list
/// sorted by insertion timestamp (server-side sort).
pub const IDX_DOMAINS_BY_CREATED: &[u8] = b"domains_by_created";

/// Account index prefix — SAME key as the legacy hash, additional fields.
pub const ACCOUNT_PREFIX: &[u8] = b"mailrs:account:";

/// Range index over `mailrs:account:*`.domain.
pub const IDX_ACCOUNTS_BY_DOMAIN: &[u8] = b"accounts_by_domain";

/// Range index over `mailrs:account:*`.active — active accounts one RTT.
pub const IDX_ACCOUNTS_BY_ACTIVE: &[u8] = b"accounts_by_active";

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
