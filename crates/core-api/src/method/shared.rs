//! Team inboxes: assigning a team conversation, and claiming the reply to
//! it before sending so two members do not both answer.
//!
//! The team conversation is named by its group id and number (`gid`,
//! `conv`), as a member's thread carries it in `shared_conv`. Only a
//! current member of a shared group may call these; anyone else is
//! refused with 403.

use serde::{Deserialize, Serialize};

/// `POST /v1/users/{user}/shared:assign`
pub const PATH_ASSIGN: &str = "/v1/users/{user}/shared:assign";
/// `POST /v1/users/{user}/shared:state`
pub const PATH_STATE: &str = "/v1/users/{user}/shared:state";
/// `POST /v1/users/{user}/shared:claim-reply`
pub const PATH_CLAIM_REPLY: &str = "/v1/users/{user}/shared:claim-reply";
/// `POST /v1/users/{user}/shared:release-reply`
pub const PATH_RELEASE_REPLY: &str = "/v1/users/{user}/shared:release-reply";

/// Request body for [`PATH_ASSIGN`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignRequest {
    pub gid: i64,
    pub conv: i64,
    /// The member to assign it to; `None` takes the assignment back.
    #[serde(default)]
    pub to: Option<String>,
    /// Epoch seconds.
    pub at: i64,
}

/// Answer to [`PATH_ASSIGN`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AssignResponse {
    /// The assignee now; `None` for nobody.
    pub assignee: Option<String>,
    /// How many members' threads carry it.
    pub rows: u32,
}

/// Request body for [`PATH_STATE`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateRequest {
    pub gid: i64,
    pub conv: i64,
}

/// Who answered the conversation last, as the reply claims record it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReplyStateWire {
    /// How many replies the team has claimed; what a composer remembers
    /// when it opens (`seen_seq`).
    pub seq: u64,
    /// The member who claimed the last one; empty before any.
    pub by: String,
    /// When, in epoch seconds; 0 before any.
    pub at: i64,
}

/// Answer to [`PATH_STATE`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StateResponse {
    pub reply: ReplyStateWire,
    pub assignee: Option<String>,
}

/// Request body for [`PATH_CLAIM_REPLY`]: the member is about to send a
/// reply to the conversation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimReplyRequest {
    pub gid: i64,
    pub conv: i64,
    /// [`ReplyStateWire::seq`] when the composer opened.
    pub seen_seq: u64,
    /// Send even though another member answered since: a deliberate
    /// second reply.
    #[serde(default)]
    pub force: bool,
    /// Epoch seconds.
    pub at: i64,
}

/// Answer to [`PATH_CLAIM_REPLY`]. A refusal is an answer, not an error:
/// it says who answered first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ClaimReplyResponse {
    /// The reply is the team's; `claim_id` releases it if the send is
    /// undone.
    Claimed { claim_id: String, seq: u64 },
    /// Another member answered after `seen_seq`.
    Conflict { by: String, at: i64, seq: u64 },
}

/// Request body for [`PATH_RELEASE_REPLY`]: the claimed reply was not
/// sent after all (the send was undone).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReplyRequest {
    pub gid: i64,
    pub conv: i64,
    pub claim_id: String,
}

/// Answer to [`PATH_RELEASE_REPLY`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReleaseReplyResponse {
    /// False when the claim is no longer the last one: someone answered
    /// after it, and their claim stands.
    pub released: bool,
}
