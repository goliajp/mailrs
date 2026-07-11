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

/// Alias key: `mailrs:alias:<address>` — string, value is the resolved
/// target address. `contact@golia.jp -> lihao@golia.jp` lives here.
/// Reads follow one hop; cycles are broken by a depth cap in the caller.
pub fn alias(address: &str) -> String {
    format!("mailrs:alias:{address}")
}

/// Set-of-aliases index: `mailrs:aliases:index` — SADD every alias key
/// on write so admin tooling can list them without SCAN.
pub const ALIAS_INDEX: &str = "mailrs:aliases:index";

/// Per-domain row: `mailrs:domain:<name>` string, value = created_at epoch.
pub fn domain(name: &str) -> String {
    format!("mailrs:domain:{name}")
}

/// Set-of-domains index: `mailrs:domains:index` — SADD every domain name.
pub const DOMAIN_INDEX: &str = "mailrs:domains:index";

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
