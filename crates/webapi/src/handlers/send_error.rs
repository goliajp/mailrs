//! A failed send as JSON, so a client can tell an oversized form from a
//! core that could not file the copy instead of reading an empty body.

use axum::Json;
use axum::extract::multipart::MultipartError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The largest request body the web tier takes. Axum's default is 2 MiB,
/// which turns away any form with a sizeable attachment. The message
/// built from a full form, attachments in base64, stays under the SMTP
/// limit the core files sent copies to.
pub const MAX_MULTIPART_BODY: usize = 25 * 1024 * 1024;

#[derive(Debug)]
pub struct SendError {
    status: StatusCode,
    body: serde_json::Value,
}

impl SendError {
    pub fn new(status: StatusCode, error: &str) -> Self {
        Self {
            status,
            body: serde_json::json!({ "error": error }),
        }
    }

    pub fn too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            body: serde_json::json!({
                "error": format!(
                    "the message is larger than the {} MiB limit",
                    MAX_MULTIPART_BODY / (1024 * 1024)
                ),
                "limit_bytes": MAX_MULTIPART_BODY,
            }),
        }
    }

    /// A form field that could not be read: too large, or malformed.
    pub fn form(e: MultipartError) -> Self {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return Self::too_large();
        }
        Self::new(StatusCode::BAD_REQUEST, &e.body_text())
    }
}

impl From<StatusCode> for SendError {
    fn from(status: StatusCode) -> Self {
        Self::new(status, status.canonical_reason().unwrap_or("error"))
    }
}

impl IntoResponse for SendError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_core_api::method::sent;

    #[test]
    fn every_hop_after_the_form_takes_what_a_full_form_becomes() {
        assert_eq!(
            sent::MAX_MESSAGE_BYTES as u64,
            mailrs_smtp_proto::MAX_MESSAGE_SIZE,
            "the core files copies up to the size the SMTP side announces"
        );
        // attachments go into the message in base64, lines of 76 + CRLF
        let message = MAX_MULTIPART_BODY / 57 * 78 + 64 * 1024;
        assert!(message < sent::MAX_MESSAGE_BYTES, "{message}");
        assert!(sent::MAX_REQUEST_BYTES >= sent::MAX_MESSAGE_BYTES.div_ceil(3) * 4);
    }

    #[test]
    fn an_oversized_form_names_the_limit() {
        let e = SendError::too_large();
        assert_eq!(e.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(e.body["limit_bytes"], MAX_MULTIPART_BODY);
        assert_eq!(
            e.body["error"],
            "the message is larger than the 25 MiB limit"
        );
    }
}
