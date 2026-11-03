//! Conversations nobody holds, listed and removed by an operator.

use serde::{Deserialize, Serialize};

/// `GET` — every conversation nobody holds, newest first.
pub const PATH_OWNERLESS_THREADS: &str = "/v1/admin/ownerless-threads";

/// `POST` — delete one; 409 when somebody holds it, 404 when there is no
/// such conversation.
pub const PATH_DELETE_OWNERLESS_THREAD: &str = "/v1/admin/ownerless-threads/{thread_id}/delete";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerlessThreadWire {
    pub thread_id: String,
    /// Messages in the conversation's shared index.
    pub messages: u64,
    /// The newest of them, epoch seconds; 0 when it has none.
    pub latest_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerlessThreadsResponse {
    pub items: Vec<OwnerlessThreadWire>,
}
