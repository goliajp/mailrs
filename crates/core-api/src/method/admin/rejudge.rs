//! Conversations taken back into Review when stored mail was judged again,
//! by the round (one observation) that took them, for an administrator to
//! read and put back whole.

use serde::{Deserialize, Serialize};

/// `GET` — every batch, newest first.
pub const PATH_REJUDGE_BATCHES: &str = "/v1/admin/rejudge-batches";

/// `POST` — put every conversation of one batch back; 404 when there is no
/// such batch, 409 while it is pending.
pub const PATH_RELEASE_REJUDGE_BATCH: &str = "/v1/admin/rejudge-batches/{observation}/release";

/// `POST` — hold what a pending batch recorded; 404 when there is no such
/// batch, 409 when it is not pending.
pub const PATH_CONFIRM_REJUDGE_BATCH: &str = "/v1/admin/rejudge-batches/{observation}/confirm";

/// `POST` — hold nothing of a pending batch; 404 when there is no such
/// batch, 409 when it is not pending.
pub const PATH_DISCARD_REJUDGE_BATCH: &str = "/v1/admin/rejudge-batches/{observation}/discard";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejudgeBatchWire {
    /// The observation whose round took them, `r<n>`.
    pub observation: String,
    /// When the first was taken, Unix seconds.
    pub at: i64,
    /// Conversations the round took back.
    pub conversations: u64,
    /// When the batch was put back; 0 while it has not been.
    pub released_at: i64,
    /// The round would hold more than one observation may on its own, so
    /// it held nothing and waits for a super administrator to confirm or
    /// discard it.
    pub pending: bool,
    /// Conversations the round found to hold when it was judged.
    pub would_hold: u64,
    /// The rules it holds them for, by id.
    pub rules: Vec<String>,
    /// When a pending batch was discarded; 0 while it has not been.
    pub discarded_at: i64,
    /// When a pending batch was confirmed; 0 while it has not been, and
    /// for a batch that held on its own or was confirmed before this
    /// was recorded.
    #[serde(default)]
    pub confirmed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejudgeBatchesResponse {
    pub items: Vec<RejudgeBatchWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRejudgeBatchRequest {
    /// Who is putting it back, for the audit.
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRejudgeBatchResponse {
    /// Conversations put back: those the batch still held.
    pub released: u64,
}

/// The body of a confirm or a discard: who decided, for the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideRejudgeBatchRequest {
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmRejudgeBatchResponse {
    /// Conversations held: those of the recorded set that may still be
    /// (not held already, not released by a person since they came).
    pub held: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardRejudgeBatchResponse {
    /// Conversations the batch would have held.
    pub would_hold: u64,
}
