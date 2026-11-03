//! Conversations taken back into Review when stored mail was judged again,
//! by the round (one observation) that took them, for an administrator to
//! read and put back whole.

use serde::{Deserialize, Serialize};

/// `GET` — every batch, newest first.
pub const PATH_REJUDGE_BATCHES: &str = "/v1/admin/rejudge-batches";

/// `POST` — put every conversation of one batch back; 404 when there is no
/// such batch.
pub const PATH_RELEASE_REJUDGE_BATCH: &str = "/v1/admin/rejudge-batches/{observation}/release";

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
