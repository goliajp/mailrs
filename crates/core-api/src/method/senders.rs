//! Accepting senders for the sectioned inbox: a first contact the reader
//! accepts stops being new, for the conversations already in the New
//! section and for mail still to come.

use serde::{Deserialize, Serialize};

/// `POST /v1/users/{user}/senders:accept`
pub const PATH_ACCEPT_SENDERS: &str = "/v1/users/{user}/senders:accept";

/// Request body for [`PATH_ACCEPT_SENDERS`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptSendersRequest {
    /// Bare addresses; compared case-insensitively.
    pub addresses: Vec<String>,
    /// Epoch seconds the reader accepted them.
    pub accepted_at: i64,
}

/// Answer to [`PATH_ACCEPT_SENDERS`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AcceptSendersResponse {
    /// How many distinct addresses were recorded as accepted.
    pub accepted: u32,
}

/// The addresses as both cores record them: trimmed, lowercased, once each,
/// empty ones left out.
pub fn normalized(addresses: &[String]) -> Vec<String> {
    let mut out: Vec<String> = addresses
        .iter()
        .map(|a| a.trim().to_ascii_lowercase())
        .filter(|a| !a.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `POST /v1/users/{user}/senders:reclassify` — move every conversation of
/// one sender that is in the ordinary lists to another bucket.
pub const PATH_RECLASSIFY_SENDER: &str = "/v1/users/{user}/senders:reclassify";

/// `POST /v1/users/{user}/senders:restore` — put back what a reclassify
/// moved, where it is still where it was moved to.
pub const PATH_RESTORE_SENDER: &str = "/v1/users/{user}/senders:restore";

/// `POST /v1/users/{user}/senders:retain` — apply a retention rule to one
/// sender's conversations now.
pub const PATH_RETAIN_SENDER: &str = "/v1/users/{user}/senders:retain";

/// One conversation a reclassify moved, and where it came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovedWire {
    pub thread_id: String,
    pub from_bucket: String,
}

/// Request body for [`PATH_RECLASSIFY_SENDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReclassifySenderRequest {
    pub sender: String,
    /// `inbox`, `notifications` or `promotions`.
    pub bucket: String,
}

/// Answer to [`PATH_RECLASSIFY_SENDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReclassifySenderResponse {
    pub moved: Vec<MovedWire>,
}

/// Request body for [`PATH_RESTORE_SENDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreSenderRequest {
    pub sender: String,
    /// The bucket the reclassify moved them to.
    pub to_bucket: String,
    pub moved: Vec<MovedWire>,
}

/// Answer to [`PATH_RESTORE_SENDER`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RestoreSenderResponse {
    pub restored: u32,
}

/// Request body for [`PATH_RETAIN_SENDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetainSenderRequest {
    pub sender: String,
    #[serde(default)]
    pub keep_latest: Option<u32>,
    #[serde(default)]
    pub older_than_days: Option<u32>,
    /// `archive` or `trash`.
    pub disposal: String,
    /// Epoch seconds the ages are measured from.
    pub now: i64,
}

/// Answer to [`PATH_RETAIN_SENDER`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RetainSenderResponse {
    pub disposed: u32,
}
