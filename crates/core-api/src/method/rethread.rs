//! Repairing which conversation a message belongs to.

use serde::{Deserialize, Serialize};

/// `POST /v1/admin/threads:split-message` — move one of a user's messages
/// out of its conversation into one of its own, named by its Message-ID:
/// the repair for a reply that was joined to the wrong conversation.
pub const PATH_SPLIT_MESSAGE: &str = "/v1/admin/threads:split-message";

/// Request body for [`PATH_SPLIT_MESSAGE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitMessageRequest {
    pub user: String,
    pub message_id: String,
}

/// Response body for [`PATH_SPLIT_MESSAGE`]: the conversation the message
/// is in now, named by its own Message-ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SplitMessageResponse {
    pub thread_id: String,
}
