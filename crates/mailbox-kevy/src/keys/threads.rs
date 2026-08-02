//! Keys for threads, their per-user rows, and the text index.

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

    /// Stable name of the bucket itself, for the `bucket` column on the
    /// membership row. Distinct from [`Self::category`], which is the
    /// canonical *category* value a forced bucket writes — several
    /// categories map onto one bucket, so the column has to carry the
    /// bucket, not a representative category.
    pub fn name(self) -> &'static str {
        match self {
            Bucket::Inbox => "inbox",
            Bucket::Notifications => "notifications",
            Bucket::Promotions => "promotions",
            Bucket::Junk => "junk",
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

/// Membership row for one (user, thread) pair — the row the declared
/// `threaduser` table is built over.
///
/// Threads themselves are multi-owner: `mailrs:thread:<tid>` is global
/// and `tid` is derived from a Message-ID, so two recipients of the
/// same message share one thread hash. Every per-user fact therefore
/// belongs on a row of its own, not as a column on the thread — writing
/// `user` onto the shared hash would make one owner wrong on every
/// write, silently.
///
/// This holds exactly what the twelve per-user zsets hold today, in one
/// place the engine can index instead of twelve places we sync by hand.
pub fn thread_user(user: &str, tid: &str) -> String {
    format!("mailrs:threaduser:{user}:{tid}")
}

/// Key-prefix domain the `threaduser` table is declared over.
pub const THREAD_USER_PREFIX: &[u8] = b"mailrs:threaduser:";

/// Every per-user thread index the legacy zset layer maintains.
///
/// Used by the membership-row backfill: the individual zsets disagree
/// with each other (one prod account's `sent` held 58 threads where
/// `by_activity` held 9), so only their union is the user's real
/// thread set.
pub fn all_user_thread_zsets(user: &str) -> Vec<String> {
    let mut out = vec![
        user_threads_by_activity(user),
        user_threads_sent(user),
        user_threads_starred(user),
        user_threads_archived(user),
        user_threads_pinned(user),
        user_threads_has_unread(user),
        user_threads_has_action(user),
    ];
    out.extend(Bucket::all_zsets(user));
    for cat in ["inbox", "notification", "promotion", "spam"] {
        out.push(user_threads_by_category(user, cat));
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::keys::*;

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
