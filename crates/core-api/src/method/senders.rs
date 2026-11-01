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
