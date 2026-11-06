//! Thread read / mutate endpoints.
//!
//! Sources:
//! - `crates/mailbox/src/pg/thread_ops/mod.rs`    — 6 fn (read)
//! - `crates/mailbox/src/pg/thread_ops/mutate.rs` — 12 fn (mutate)

use serde::{Deserialize, Serialize};

// ── read paths ──────────────────────────────────────────────────────

pub const PATH_LIST_THREAD_MESSAGES: &str = "/v1/users/{user}/threads/{thread_id}/messages";
pub const PATH_BACKFILL_THREADING: &str = "/v1/admin/backfill-threading";

/// `GET /v1/users/{user}/sent-messages` — one row per message the user
/// actually sent (not per thread), newest first, so the Sent view can
/// show the recipient and every outbound message in a multi-reply thread
/// separately.
pub const PATH_LIST_SENT_MESSAGES: &str = "/v1/users/{user}/sent-messages";

// ── mutate paths (every action becomes its own POST) ───────────────

pub const PATH_MARK_READ: &str = "/v1/users/{user}/threads/{thread_id}/read";
pub const PATH_MARK_UNREAD: &str = "/v1/users/{user}/threads/{thread_id}/unread";
pub const PATH_MARK_ALL_READ: &str = "/v1/users/{user}/conversations:mark-all-read";
/// Mark read everything one list is showing — same filter the list
/// read takes, so the set marked is the set on screen.
pub const PATH_MARK_LIST_READ: &str = "/v1/users/{user}/conversations:mark-list-read";
pub const PATH_STAR: &str = "/v1/users/{user}/threads/{thread_id}/star";
pub const PATH_UNSTAR: &str = "/v1/users/{user}/threads/{thread_id}/unstar";
pub const PATH_PIN: &str = "/v1/users/{user}/threads/{thread_id}/pin";
pub const PATH_UNPIN: &str = "/v1/users/{user}/threads/{thread_id}/unpin";
pub const PATH_ARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/archive";
pub const PATH_UNARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/unarchive";
/// Move a conversation to this reader's Trash: out of every other list,
/// kept until restored, deleted for good, or 30 days pass.
pub const PATH_TRASH: &str = "/v1/users/{user}/threads/{thread_id}/trash";
/// Take it back out of the Trash, to the list it left.
pub const PATH_UNTRASH: &str = "/v1/users/{user}/threads/{thread_id}/untrash";
/// `POST` — fetch the remote images of the thread's mail now, for mail that
/// arrived before images were fetched on arrival. 409 for a thread in Junk
/// or held for review, unless `?anyway=true`: the reader asked for them
/// knowing it tells the sender the address is read.
pub const PATH_PREFETCH_IMAGES: &str = "/v1/users/{user}/threads/{thread_id}/images";
/// Delete every conversation in this reader's Trash for good.
/// Answers `{"affected": n}`.
pub const PATH_EMPTY_TRASH: &str = "/v1/users/{user}/trash:empty";
/// The review list: what this reader is holding as suspected fraud,
/// newest first.
///
/// Its own route rather than a flag on the ordinary list, because it
/// is a different question — "what was taken away from me, and why" —
/// asked from a screen a reader arrives at in a hurry, usually
/// because a message they expected never appeared.
pub const PATH_QUARANTINE_LIST: &str = "/v1/users/{user}/quarantine";

/// What the review list answers with.
///
/// Its own type rather than [`crate::method::conversation::ListConversationsResponse`]
/// because it carries a **total**, and that is the whole point: the
/// route computed one all along — the index count the same walk
/// produces — and threw it away. A reader who was served a page and
/// no count could not tell "this is everything" from "this is the
/// first page of many", and on 2026-08-31 that difference was 200
/// against 439.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineListResponse {
    /// This page, newest first.
    pub items: Vec<crate::types::ConversationSummaryWire>,
    /// How many are held in total, not how many are on this page.
    ///
    /// From the index, not from counting the rows returned — so it is
    /// right even when the page is the last one and short.
    pub total: usize,
}

/// Release one held conversation: it was not fraud.
///
/// Clears the flag, returns the conversation to whatever list it
/// belonged to, and records that a person disagreed — which is the
/// only training signal a false positive ever produces.
pub const PATH_QUARANTINE_RELEASE: &str = "/v1/users/{user}/quarantine/{thread_id}/release";

/// Every time a held conversation was let go, oldest first.
///
/// A fact per release, never rewritten: who let it go, when, and whether
/// a person did (`person`), an administrator's batch put-back (`batch`) or
/// a rule change (`rejudge`). A rejudge holds again only what a person has
/// not released, so this is what it consults.
pub const PATH_THREAD_RELEASES: &str = "/v1/users/{user}/threads/{thread_id}/releases";

/// One release of a held conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseFact {
    /// Unix seconds.
    pub at: i64,
    /// `person`, `batch` or `rejudge`.
    pub source: String,
    /// The reader or administrator, or `system` for a rule change.
    pub actor: String,
    /// The rule pack that let it go; empty unless `source` is `rejudge`.
    #[serde(default)]
    pub rules_version: String,
}

/// What [`PATH_THREAD_RELEASES`] answers with.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleasesResponse {
    pub items: Vec<ReleaseFact>,
}

/// The stored four-layer verdict for one message, as it was decided.
pub const PATH_FRAUD_VERDICT: &str = "/v1/users/{user}/messages/{message_id}/fraud-verdict";

pub const PATH_SNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/snooze";
pub const PATH_UNSNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/unsnooze";
pub const PATH_DELETE_THREAD: &str = "/v1/users/{user}/threads/{thread_id}";

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as junk.
/// Route + fastcore handler: move to `user_threads_junk` zset,
/// stamp `category = "spam"` on the row. Does NOT modify the
/// recipient's whitelist / blacklist (per §D4 — "只移这封,不动名单").
pub const PATH_MARK_JUNK: &str = "/v1/users/{user}/threads/{thread_id}/mark-junk";

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as not junk.
/// Route + fastcore handler: move to `user_threads_inbox` zset,
/// stamp `category = "inbox"`. The webapi handler additionally
/// SADDs the thread's sender into `spam:{user}:whitelist` so
/// future arrivals from the same sender bypass the score
/// threshold when authed.
pub const PATH_MARK_NOT_JUNK: &str = "/v1/users/{user}/threads/{thread_id}/mark-not-junk";

/// v2.9 triage — move a thread into the Notifications bucket
/// (`set_bucket(Notifications)`, stamp `category = "notification"`),
/// and train the triage classifier on this correction.
pub const PATH_MARK_NOTIFICATION: &str = "/v1/users/{user}/threads/{thread_id}/mark-notification";

/// v2.9 triage — move a thread into the Promotions bucket
/// (`set_bucket(Promotions)`, stamp `category = "promotion"`), and
/// train the triage classifier on this correction.
pub const PATH_MARK_PROMOTION: &str = "/v1/users/{user}/threads/{thread_id}/mark-promotion";

/// v2.9 triage — move a thread into the Inbox bucket
/// (`set_bucket(Inbox)`, stamp `category = "inbox"`), and train the
/// triage classifier on this correction. Also clears junk if the
/// thread was previously in Junk.
pub const PATH_MOVE_TO_INBOX: &str = "/v1/users/{user}/threads/{thread_id}/move-to-inbox";

/// `POST /v1/users/{user}/threads/{thread_id}/messages` — deliver a
/// message whose metadata the caller supplies (an import, a sync from the
/// other core) into the user's view. Fires the same
/// `record_message_arrival` + `upsert_thread` sequence inbound delivery
/// uses. A user's own sent copy goes through `sent::PATH_FILE_SENT`.
pub const PATH_DELIVER_MESSAGE: &str = "/v1/users/{user}/threads/{thread_id}/messages";

// ── req/resp ────────────────────────────────────────────────────────

/// Response body for `GET /v1/users/{user}/threads/{thread_id}/messages`.
///
/// `MessageWire` defined in `method::message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListThreadMessagesResponse {
    /// Message rows in this thread, ordered by `internal_date` ASC.
    pub items: Vec<crate::method::message::MessageWire>,
}

/// One outbound message in the Sent view — message granularity, not
/// thread. Carries the recipient (To) so the list shows who it went to,
/// and `thread_id` + `uid` so a click opens the thread and focuses this
/// exact message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentMessageSummary {
    pub uid: u32,
    pub message_id: String,
    pub thread_id: String,
    /// Raw `To:` header — who this message was sent to.
    pub to: String,
    pub subject: String,
    /// Epoch seconds.
    pub internal_date: i64,
}

/// Response body for `GET /v1/users/{user}/sent-messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentMessagesResponse {
    pub items: Vec<SentMessageSummary>,
}

/// Request body for `PUT /v1/users/{user}/threads/{thread_id}/snooze`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnoozeRequest {
    /// Epoch seconds to wake the thread up at. `0` brings it back now,
    /// which is what `unsnooze` sends.
    pub snoozed_until: i64,
}

/// Generic "thread mutate" affected-row count response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ThreadActionResponse {
    /// Number of messages modified by this action.
    pub affected: u32,
    /// New `highest_modseq` of the affected mailbox (for CONDSTORE clients).
    /// 0 when the action didn't bump modseq (rare).
    pub new_modseq: u64,
}

/// One conversation emptying the Trash deleted for good.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletedThreadWire {
    pub thread_id: String,
    /// Message files deleted with it.
    pub messages: u32,
}

/// Response of `POST /v1/users/{user}/trash:empty`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EmptyTrashResponse {
    /// Conversations deleted for good.
    pub affected: u32,
    pub new_modseq: u64,
    /// Each conversation deleted, by thread id.
    #[serde(default)]
    pub deleted: Vec<DeletedThreadWire>,
}

/// Request body for `POST /v1/users/{user}/threads/{thread_id}/messages`.
///
/// The webapi caller has already produced the RFC 5322 envelope (for
/// send/reply) or the draft body (for save-draft), delivered it to the
/// maildir (`.Sent` / `.Drafts`), and holds the resulting blob_ref
/// (the maildir filename). Here it hands the fastcore side the
/// metadata needed to:
///
///   - populate `mailrs:thread:<tid>` aggregate + `sent_count`
///   - add the tid to `mailrs:user:<u>:threads:sent` (via `upsert_thread`
///     with `senders_csv` containing `user`)
///   - write the per-message `mailrs:msg:<mid>` JSON blob so
///     `list_thread_messages` returns it
///   - index the UID in `mailrs:user:<u>:msg_by_uid`
///
/// `payload_wire_json` is the pre-serialized `MessageWire` — fastcore
/// stores it verbatim; the enrichment path in the webapi
/// (`enrich_with_body`) then reads the maildir file at `blob_ref`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverMessageRequest {
    /// RFC 5322 `Message-ID:` header value (no angle brackets).
    pub message_id: String,
    /// Decoded subject; overwritten into the thread aggregate.
    pub subject: String,
    /// Comma-joined sender addresses; drives the sent-index membership
    /// check (`senders_csv_contains_user`).
    pub senders_csv: String,
    /// Epoch seconds for the thread's `latest_date` + activity zset score.
    pub latest_date: i64,
    /// Short preview (first ~120 chars of body); overwritten into the
    /// thread aggregate.
    pub latest_preview: String,
    /// Category label used by `user_threads_by_category`. For sent /
    /// draft the caller should pass `"inbox"` so it doesn't drop into a
    /// spam/scam bucket by accident.
    pub category: String,
    /// `true` = inbound-arrival (bumps `unread_count`); `false` = sent
    /// or draft (bumps `sent_count`).
    pub unread: bool,
    /// UID within the user's mailbox — must match
    /// `payload_wire_json.uid`. Fastcore mirrors it into the
    /// user-scoped uid index so `get_message_by_uid_for_user` finds it.
    pub uid: u32,
    /// Pre-serialized `MessageWire` (JSON). Stored verbatim as the
    /// message blob.
    pub payload_wire_json: String,
    /// What delivery recorded about the message for the sectioned inbox.
    /// Absent for mail carried over without them (a sync from the other
    /// core): the conversation's section is then left undecided, as for
    /// mail from before sections existed, until a backfill decides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_facts: Option<SectionFactsWire>,
}

/// [`mailrs_mailbox::section::MessageFacts`] on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SectionFactsWire {
    /// The reader's address is in `To:`.
    pub direct: bool,
    /// A person wrote it, not a machine or a list.
    pub from_person: bool,
    /// Nothing had passed between the reader and the sender before it.
    pub first_contact: bool,
    /// The reader's own message went to someone besides the reader.
    pub to_others: bool,
}

impl From<SectionFactsWire> for mailrs_mailbox::section::MessageFacts {
    fn from(w: SectionFactsWire) -> Self {
        Self {
            direct: w.direct,
            from_person: w.from_person,
            first_contact: w.first_contact,
            to_others: w.to_others,
        }
    }
}

/// Response body for `DeliverMessageRequest`. Currently just echoes
/// the resolved `thread_id` for symmetry with other wire types; a
/// future field could carry the new modseq once fastcore honours
/// CONDSTORE.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DeliverMessageResponse {
    pub thread_id: String,
    pub message_id: String,
}

/// Request body for [`PATH_MARK_LIST_READ`]: the list to mark, asked the
/// way the list itself is asked.
///
/// Wire-compatible with the [`ListConversationsRequest`] body this route
/// took before `quarantined` was added, so an older client marks what it
/// marked before.
///
/// Every conversation the list shows is marked, not one page: `limit`,
/// `before_ts` and `after_cursor` are not read.
///
/// [`ListConversationsRequest`]: crate::method::conversation::ListConversationsRequest
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarkListReadRequest {
    #[serde(flatten)]
    pub filter: crate::types::ConversationFilter,
    /// Mark the Review list — what is held as suspected fraud — which is
    /// its own route rather than an axis of the filter; `filter` is then
    /// not read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quarantined: bool,
}

impl MarkListReadRequest {
    /// The filter of the whole list: no page boundary, no page size.
    pub fn whole_list(&self) -> crate::types::ConversationFilter {
        crate::types::ConversationFilter {
            limit: 0,
            before_ts: None,
            after_cursor: None,
            ..self.filter.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snooze_request_roundtrip() {
        let req = SnoozeRequest {
            snoozed_until: 1_700_000_000,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: SnoozeRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    /// The body this route took before Review could be named reads as the
    /// same list.
    #[test]
    fn an_old_mark_list_body_reads_as_the_same_list() {
        let old = r#"{"folder":"NP","unread":true,"limit":50,"archived":false}"#;
        let req: MarkListReadRequest = serde_json::from_str(old).unwrap();
        assert_eq!(req.filter.folder.as_deref(), Some("NP"));
        assert_eq!(req.filter.unread, Some(true));
        assert!(!req.quarantined);
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("quarantined").is_none());
    }

    #[test]
    fn thread_action_default() {
        let r = ThreadActionResponse::default();
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"affected\":0"));
        assert!(s.contains("\"new_modseq\":0"));
    }
}
