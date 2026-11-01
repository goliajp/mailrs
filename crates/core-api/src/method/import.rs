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
}

impl ImportMessageRequest {
    pub fn new(message: &[u8]) -> Self {
        Self {
            message_base64: base64::engine::general_purpose::STANDARD.encode(message),
        }
    }

    /// The message bytes; an error when the base64 does not decode.
    pub fn message(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.message_base64)
    }
}
