//! The sender's own copy of a message the web tier sent.
//!
//! The core that owns the user's mailbox files it: the maildir file in
//! Sent, the index as the user's own message, the usage counter, and the
//! contacts. The web tier only hands over the bytes it queued.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// `POST /v1/users/{user}/sent`
pub const PATH_FILE_SENT: &str = "/v1/users/{user}/sent";

/// The message as queued, and its envelope recipients.
///
/// JSON with the bytes in base64 rather than a raw body, because the Bcc
/// recipients travel beside the message and are not in it: the outbound
/// bytes carry no `Bcc:` header, and the sent copy must be those bytes
/// exactly, since a resend re-queues them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSentRequest {
    /// RFC 5322 bytes, standard base64.
    pub message_base64: String,
    /// To, Cc and Bcc as the user wrote them (`Name <addr>` or `addr`).
    pub recipients: Vec<String>,
}

impl FileSentRequest {
    pub fn new(message: &[u8], recipients: &[String]) -> Self {
        Self {
            message_base64: base64::engine::general_purpose::STANDARD.encode(message),
            recipients: recipients.to_vec(),
        }
    }

    /// The message bytes; an error when the base64 does not decode.
    pub fn message(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.message_base64)
    }
}

/// Where the copy was filed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSentResponse {
    /// The conversation it joined.
    pub thread_id: String,
    pub message_id: String,
    /// The maildir file, `.Sent/<name>`: what a resend reads back.
    pub blob_ref: String,
}
