//! Wire types shared across RPC methods.
//!
//! Domain types in `mailrs-mailbox` lack `Serialize`/`Deserialize` derives
//! and the ironrule forbids touching that crate to add them. So this module
//! defines wire mirrors with the same field shape + Serde + `From` /
//! `to_summary` conversions in both directions.
//!
//! Conversions live here so neither `mailrs-mailbox` nor the server cement
//! has to be modified (ironrule).

use serde::{Deserialize, Serialize};

// ──────────────────────────────────────────────────────────────────────
// Health
// ──────────────────────────────────────────────────────────────────────

/// Health probe response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    /// Wire version this server speaks (matches `API_VERSION`).
    pub version: String,
    /// Which backend is running: `"pg"` (core) or `"kevy"` (fastcore).
    pub backend: BackendKind,
    /// `true` if all dependencies are reachable.
    pub ready: bool,
    /// The core's container memory, from its cgroup; `None` where the
    /// cgroup cannot be read (not in a container, cgroup v1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryWire>,
    /// The spool this core drains. `None` from a core that predates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spool: Option<SpoolHealthWire>,
    /// Work items this core gave up on because the process died on them
    /// each time it tried. `None` from a core without those loops (the SQL
    /// core) or one that predates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_aside: Option<SetAsideWire>,
    /// This core's parse workers. `None` from a core that predates them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsing: Option<ParsingHealthWire>,
}

/// What one process's parse workers went through since it started.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsingHealthWire {
    /// Whether parsing runs in workers; `false` means in process.
    pub worker: bool,
    /// Messages a worker failed on in the last 24 hours.
    pub failures_24h: u64,
    /// Unix seconds of the last such failure; `None` before the first.
    pub last_failure_at: Option<i64>,
    /// Workers started again after one died or was killed.
    pub worker_restarts: u64,
    /// `parser_failure_total` by parser and kind, since start.
    pub failures: Vec<ParserFailureWire>,
    /// Unix seconds since which a worker has been held back after dying
    /// repeatedly; `None` while none is, or from a process that predates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_since: Option<i64>,
}

/// One `parser_failure_total{parser, kind}` count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParserFailureWire {
    pub parser: String,
    /// `panic`, `timeout`, `killed` or `error`.
    pub kind: String,
    pub count: u64,
}

/// The spool, as a health probe sees it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpoolHealthWire {
    /// Files set aside in `<spool>/quarantine/` after the drain died on
    /// each of them three times.
    pub quarantined: u64,
    /// Age of the oldest file waiting in `<spool>/incoming/new/`, in
    /// seconds; 0 when none is waiting.
    pub oldest_incoming_age_seconds: u64,
}

/// Items set aside by the core's background loops.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SetAsideWire {
    /// Attachment-text queue items given up on; `None` when the store
    /// could not be read.
    pub attachment_text: Option<u64>,
    /// Conversations the fraud backfill skipped for its current rules;
    /// `None` when no backfill runs here.
    pub fraud_backfill: Option<u64>,
}

/// A container's memory, as its cgroup counts it: the process and every
/// child it starts (the OCR worker), page cache included.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryWire {
    pub used_bytes: u64,
    /// `None` when the cgroup sets no limit.
    pub limit_bytes: Option<u64>,
    /// The core's own key-value store, by its own count: bytes it holds
    /// and keys. `None` for a core without one (the SQL core).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kevy_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kevy_keys: Option<u64>,
}

/// Which storage backend a `mailrs-core-api` server is wrapping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// `mailrs-core` running on PG (today's `spg` cement).
    Pg,
    /// `mailrs-fastcore` running on kevy (experimental).
    Kevy,
}

// ──────────────────────────────────────────────────────────────────────
// Identifier aliases
// ──────────────────────────────────────────────────────────────────────

/// User scope identifier (email address). Used as the primary "tenant" key.
pub type UserAddress = String;

/// Internal numeric ID for a mailbox row.
pub type MailboxId = i64;

/// Internal numeric ID for a message row.
pub type MessageId = i64;

/// Maildir-style on-disk identifier for a message.
pub type MaildirId = String;

/// Thread identifier (string hash from References/Message-ID chain).
pub type ThreadId = String;

// ──────────────────────────────────────────────────────────────────────
// Conversation wire types (mirror `mailrs_mailbox::ConversationSummary`)
// ──────────────────────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::ConversationSummary`.
///
/// Same field shape, with Serde derives. Conversion via `From` impls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationSummaryWire {
    pub thread_id: ThreadId,
    pub subject: String,
    pub participants: String,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_date: i64,
    pub category: String,
    pub flagged: bool,
    pub snippet: String,
    pub pinned: bool,
    pub archived: bool,
    /// Held as suspected fraud, for this reader.
    ///
    /// On the wire because a list asked with `include_quarantined`
    /// shows held conversations **marked** — and a client that cannot
    /// tell which ones they are cannot mark anything. Default false so
    /// an older client reads an ordinary row rather than failing.
    #[serde(default)]
    pub quarantined: bool,
    pub importance_level: String,
    pub importance_score: f32,
    pub requires_action: bool,
    pub sent_count: u32,
    /// Which connected mailbox this arrived at, empty for our own.
    ///
    /// The client needs it for two things a unified list cannot do
    /// without: the colour dot beside the row, and choosing which
    /// address a reply leaves by.
    #[serde(default)]
    pub account_id: String,
    /// Epoch seconds this reader put the thread away until, or `0`.
    ///
    /// On the row so a client can say *Snoozed until Tuesday* rather
    /// than showing a thread that has silently left the inbox. It is
    /// per-reader: the field used to live on the shared thread hash,
    /// where putting a conversation away would have done it for
    /// everyone who could see it — had anything read it at all.
    #[serde(default)]
    pub snoozed_until: i64,
    /// Epoch seconds this reader moved the thread to the Trash, or `0`.
    /// A client says when the Trash will delete it for good from this.
    #[serde(default)]
    pub trashed_at: i64,
    /// Ids of this reader's labels on the thread.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The sectioned inbox's section for this reader (`awaiting`, `feed`,
    /// `new`, `reply`, `other`), or empty when it has not been decided yet.
    #[serde(default)]
    pub section: String,
    /// Epoch seconds the reader's last message went out while the
    /// conversation waits on a person to answer, or `0`.
    #[serde(default)]
    pub awaiting_since: i64,
    /// The team conversation this is the reader's copy of,
    /// `"{group}:{conversation}"`; empty when it is not a team copy.
    #[serde(default)]
    pub shared_conv: String,
    /// The team member who answers it; empty for nobody.
    #[serde(default)]
    pub assignee: String,
    /// Where a list stood after this row: hand it back as
    /// [`ConversationFilter::after_cursor`] for the rows after it. Opaque to
    /// clients; empty on rows that did not come from a list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cursor: String,
}

impl From<&mailrs_mailbox::types::ConversationSummary> for ConversationSummaryWire {
    fn from(s: &mailrs_mailbox::types::ConversationSummary) -> Self {
        Self {
            labels: Vec::new(),
            // The mailbox stone has no account concept — connected
            // mailboxes are a fastcore-side feature — so everything
            // arriving through it is this deployment's own, which is
            // what the empty string means.
            account_id: String::new(),
            thread_id: s.thread_id.clone(),
            subject: s.subject.clone(),
            participants: s.participants.clone(),
            message_count: s.message_count,
            unread_count: s.unread_count,
            last_date: s.last_date,
            category: s.category.clone(),
            flagged: s.flagged,
            snippet: s.snippet.clone(),
            pinned: s.pinned,
            archived: s.archived,
            quarantined: s.quarantined,
            importance_level: s.importance_level.clone(),
            importance_score: s.importance_score,
            requires_action: s.requires_action,
            sent_count: s.sent_count,
            snoozed_until: s.snoozed_until,
            trashed_at: s.trashed_at,
            section: s.section.clone(),
            awaiting_since: s.awaiting_since,
            shared_conv: s.shared_conv.clone(),
            assignee: s.assignee.clone(),
            cursor: String::new(),
        }
    }
}

impl From<mailrs_mailbox::types::ConversationSummary> for ConversationSummaryWire {
    fn from(s: mailrs_mailbox::types::ConversationSummary) -> Self {
        (&s).into()
    }
}

// ──────────────────────────────────────────────────────────────────────
// Conversation filter (matches the 10 args of `list_conversations`)
// ──────────────────────────────────────────────────────────────────────

/// Filter axes for `list_conversations`.
///
/// Mirrors the 10-arg signature in
/// `crates/mailbox/src/pg/thread_ops/query.rs:18`. All filters are
/// optional; `user` is implicit in the path.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationFilter {
    /// Max items to return; 0, or absent, is each endpoint's own default.
    #[serde(default)]
    pub limit: u32,
    /// Page cursor: epoch-seconds, returns threads with `last_date < before_ts`.
    ///
    /// Kept for clients that page by time alone; it skips the rest of a
    /// second that a page boundary falls inside. New clients page with
    /// [`Self::after_cursor`]. Giving both is an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_ts: Option<i64>,
    /// The rows after the one whose [`ConversationSummaryWire::cursor`] this
    /// is: the next page, with no conversation of a shared second lost or
    /// listed twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<String>,
    /// Limit to a single category (`personal` / `bulk` / `spam` / `scam` / ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// When set, query across these domains instead of single `user`.
    /// Empty Vec = single-user mode (alias for None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    /// `true` shows only archived threads, `false` (default) hides them.
    #[serde(default)]
    pub archived: bool,
    /// `true` is the Trash list; `false` (default) leaves the Trash out
    /// of every other list, Archived included.
    #[serde(default)]
    pub trashed: bool,
    /// Only threads carrying this label id, archived ones included and
    /// those in the Trash left out; `folder` and `archived` are ignored.
    #[serde(default)]
    pub label: Option<String>,
    /// `true` also returns conversations held as suspected fraud.
    ///
    /// Default `false`, and the default is the point: a held
    /// conversation is out of the rows **and** out of the total,
    /// because the exclusion lives in the declared query shape rather
    /// than in a filter applied to a page that was already counted.
    ///
    /// This is a query parameter and not a policy. Whether a
    /// conversation is held is decided at receive time; whether a
    /// caller wants to see it is decided here. Two ideas, two
    /// switches — the reader's preference is a third, and lives in
    /// prefs.
    #[serde(default)]
    pub include_quarantined: bool,
    /// Restrict to a single mailbox name (e.g. `INBOX`, `Sent`). `all` is
    /// All Mail: every folder but Junk, archived conversations included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// `Some(true)` = unread only, `Some(false)` = read only, `None` = both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
    /// `Some(true)` = starred only, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    /// `Some(true)` = pinned only, across folders as Starred is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    /// `Some(true)` = only conversations put away until a time still to
    /// come, across folders. They are archived while away, so this list
    /// ignores `archived`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed: Option<bool>,
    /// One section of the sectioned inbox: `awaiting`, `feed`, `new`,
    /// `reply` or `other`. `awaiting` spans the Inbox and Sent and pages by
    /// `before_ts` against `awaiting_since`; the others are the live list's
    /// conversations in that section, newest first. Any other value is no
    /// section at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// With `section = awaiting`: only conversations waiting since this
    /// epoch second or earlier — the ones due a follow-up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_due_by: Option<i64>,
    /// Only the reader's team conversations assigned to the reader,
    /// newest first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub assigned_to_me: bool,
    /// Only conversations that arrived at these connected mailboxes.
    ///
    /// Absent is every account, which is what the unified lists mean.
    /// An empty list is a filter nothing satisfies — somebody who has
    /// unchecked every box is not asking for all of them. This
    /// deployment's own mail is the empty string, so it can be
    /// switched off like any other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts: Option<Vec<String>>,
}

// ──────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_serde() {
        let pg = BackendKind::Pg;
        let s = serde_json::to_string(&pg).unwrap();
        assert_eq!(s, "\"pg\"");
        let back: BackendKind = serde_json::from_str(&s).unwrap();
        assert_eq!(back, BackendKind::Pg);

        let s2 = serde_json::to_string(&BackendKind::Kevy).unwrap();
        assert_eq!(s2, "\"kevy\"");
    }

    #[test]
    fn conversation_filter_omits_empty_options() {
        let f = ConversationFilter {
            limit: 50,
            ..Default::default()
        };
        let s = serde_json::to_string(&f).unwrap();
        assert!(!s.contains("category"));
        assert!(!s.contains("domains"));
        assert!(!s.contains("folder"));
        assert!(s.contains("\"limit\":50"));
        assert!(s.contains("\"archived\":false"));
    }

    #[test]
    fn conversation_summary_wire_roundtrip() {
        let w = ConversationSummaryWire {
            labels: Vec::new(),
            account_id: String::new(),
            thread_id: "tid-1".into(),
            subject: "hello".into(),
            participants: "a@x.com,b@y.com".into(),
            message_count: 3,
            unread_count: 1,
            last_date: 1_700_000_000,
            category: "personal".into(),
            flagged: true,
            snippet: "preview".into(),
            pinned: false,
            archived: false,
            quarantined: false,
            importance_level: "important".into(),
            importance_score: 0.8,
            requires_action: true,
            sent_count: 0,
            snoozed_until: 0,
            trashed_at: 0,
            section: "awaiting".into(),
            awaiting_since: 1_700_000_000,
            shared_conv: "3:7".into(),
            assignee: "a@x.test".into(),
            cursor: "1700000000:42".into(),
        };
        let s = serde_json::to_string(&w).unwrap();
        let back: ConversationSummaryWire = serde_json::from_str(&s).unwrap();
        assert_eq!(w, back);
    }

    #[test]
    fn conversation_filter_full_roundtrip() {
        let f = ConversationFilter {
            label: None,
            accounts: None,
            limit: 50,
            before_ts: Some(1_700_000_000),
            after_cursor: None,
            category: Some("personal".into()),
            domains: Some(vec!["example.com".into(), "test.com".into()]),
            archived: true,
            trashed: false,
            include_quarantined: false,
            folder: Some("INBOX".into()),
            unread: Some(true),
            starred: Some(false),
            pinned: Some(true),
            snoozed: Some(true),
            section: Some("reply".into()),
            followup_due_by: Some(1_700_000_000),
            assigned_to_me: true,
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: ConversationFilter = serde_json::from_str(&s).unwrap();
        assert!(back.assigned_to_me);
        assert_eq!(back.limit, f.limit);
        assert_eq!(back.pinned, Some(true));
        assert_eq!(back.snoozed, Some(true));
        assert_eq!(back.before_ts, f.before_ts);
        assert_eq!(back.domains.as_ref().map(|v| v.len()), Some(2));
    }
}

/// Parse a comma-separated `senders_csv` into bare email addresses,
/// lowercased.
///
/// Handles `Name <email>`, bare `email`, and mixed. Shared because the
/// same CSV is parsed on two paths that must agree: the contacts sync
/// and the not-junk whitelist. They did not — the whitelist stored the
/// whole `Name <email>` string while delivery compares the envelope's
/// bare address, so a whitelisted sender never matched and kept
/// landing in Junk no matter how many times it was marked.
pub fn sender_addresses(csv: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in csv.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let addr = match (token.rfind('<'), token.rfind('>')) {
            (Some(lt), Some(gt)) if gt > lt => token[lt + 1..gt].trim(),
            _ => token,
        };
        if addr.contains('@') {
            out.push(addr.to_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod sender_address_tests {
    use super::sender_addresses;

    #[test]
    fn strips_display_names_and_lowercases() {
        assert_eq!(
            sender_addresses("GMOあおぞらネット銀行 <AC-Biz-Navi@gmo-aozora.com>"),
            vec!["ac-biz-navi@gmo-aozora.com"]
        );
    }

    #[test]
    fn handles_bare_and_mixed() {
        assert_eq!(
            sender_addresses("a@x.com, Bob <b@y.com> , , not-an-address"),
            vec!["a@x.com".to_string(), "b@y.com".to_string()]
        );
    }

    #[test]
    fn a_display_name_containing_an_at_does_not_leak() {
        // The angle brackets win — otherwise "@handle" in a display
        // name would be whitelisted as if it were the sender.
        assert_eq!(
            sender_addresses("@someone <real@x.com>"),
            vec!["real@x.com"]
        );
    }
}
