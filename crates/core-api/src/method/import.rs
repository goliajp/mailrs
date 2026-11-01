//! A message brought over from another mailbox, filed by the core that
//! owns the user's mailbox (`mailrs_intake::import`): into INBOX, indexed
//! as received, with nothing a delivery sets off.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// `POST /v1/users/{user}/import`
pub const PATH_IMPORT_MESSAGE: &str = "/v1/users/{user}/import";

/// The request body limit, the same as a sent copy's.
pub const MAX_REQUEST_BYTES: usize = crate::method::sent::MAX_REQUEST_BYTES;

/// Request body for [`PATH_IMPORT_MESSAGE`]. The answer is a
/// [`crate::method::sent::FileSentResponse`]: where it was filed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportMessageRequest {
    /// RFC 5322 bytes, standard base64.
    pub message_base64: String,
    /// The fraud verdict the caller reached, as `mailrs_inbound::FraudVerdict`
    /// JSON, the way the receiver hands one to a delivery. The core stores it
    /// and holds the conversation when it says to; absent when the rules
    /// found nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraud_verdict: Option<String>,
}

impl ImportMessageRequest {
    pub fn new(message: &[u8], fraud_verdict: Option<String>) -> Self {
        Self {
            message_base64: base64::engine::general_purpose::STANDARD.encode(message),
            fraud_verdict,
        }
    }

    /// The message bytes; an error when the base64 does not decode.
    pub fn message(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.message_base64)
    }
}
