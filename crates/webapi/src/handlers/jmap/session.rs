//! The session document (RFC 8620 §2).
//!
//! Its URLs start from `MAILRS_PUBLIC_BASE_URL` (or `https://` +
//! `MAILRS_HOSTNAME`), never from the request's Host header, which the
//! client controls.

use axum::Json;
use axum::extract::Extension;
use axum::response::IntoResponse;
use mailrs_jmap::dispatch::{JMAP_CORE_CAP, JMAP_MAIL_CAP, JMAP_SUBMISSION_CAP};
use serde_json::{Value, json};

use super::limits;
use super::masked::MASKED_CAP;
use super::submission::MAX_DELAYED_SEND;
use super::vacation::VACATION_CAP;
use crate::handlers::conversations::AuthedUser;

/// Where clients reach this server, without a trailing slash.
pub fn base_url() -> String {
    let configured = std::env::var("MAILRS_PUBLIC_BASE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty());
    match configured {
        Some(u) => u.trim().trim_end_matches('/').to_string(),
        None => {
            let host = std::env::var("MAILRS_HOSTNAME").unwrap_or_default();
            format!("https://{}", host.trim())
        }
    }
}

fn capabilities() -> Value {
    json!({
        JMAP_CORE_CAP: {
            "maxSizeUpload": limits::MAX_SIZE_UPLOAD,
            "maxConcurrentUpload": limits::MAX_CONCURRENT_UPLOAD,
            "maxSizeRequest": limits::MAX_SIZE_REQUEST,
            "maxConcurrentRequests": limits::MAX_CONCURRENT_REQUESTS,
            "maxCallsInRequest": limits::MAX_CALLS,
            "maxObjectsInGet": limits::MAX_OBJECTS,
            "maxObjectsInSet": limits::MAX_OBJECTS,
            "collationAlgorithms": [],
        },
        JMAP_MAIL_CAP: {},
        JMAP_SUBMISSION_CAP: {},
        VACATION_CAP: {},
        MASKED_CAP: {},
    })
}

fn account_capabilities() -> Value {
    json!({
        JMAP_MAIL_CAP: {
            "maxMailboxesPerEmail": null,
            "maxMailboxDepth": 1,
            "maxSizeMailboxName": 255,
            "maxSizeAttachmentsPerEmail": limits::MAX_SIZE_UPLOAD,
            "emailQuerySortOptions": ["receivedAt"],
            "mayCreateTopLevelMailbox": true,
        },
        JMAP_SUBMISSION_CAP: {
            "maxDelayedSend": MAX_DELAYED_SEND,
            "submissionExtensions": { "FUTURERELEASE": [MAX_DELAYED_SEND.to_string()] },
        },
        VACATION_CAP: {},
        MASKED_CAP: {},
    })
}

fn document(user: &str) -> Value {
    let base = base_url();
    json!({
        "capabilities": capabilities(),
        "accounts": {
            user: {
                "name": user,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": account_capabilities(),
            }
        },
        "primaryAccounts": {
            JMAP_MAIL_CAP: user,
            JMAP_SUBMISSION_CAP: user,
            VACATION_CAP: user,
            MASKED_CAP: user,
        },
        "username": user,
        "apiUrl": format!("{base}/jmap/api/"),
        "downloadUrl": format!("{base}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}"),
        "uploadUrl": format!("{base}/jmap/upload/{{accountId}}/"),
        "eventSourceUrl": format!("{base}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
    })
}

/// The session's own state: it changes only when the document does.
pub fn state_of(user: &str) -> String {
    let doc = document(user).to_string();
    format!(
        "{:016x}",
        mailrs_core_api::method::message_index::fnv64(doc.as_bytes())
    )
}

/// GET /.well-known/jmap and GET /jmap/session.
pub async fn session(Extension(AuthedUser(user)): Extension<AuthedUser>) -> impl IntoResponse {
    let mut doc = document(&user);
    doc["state"] = json!(state_of(&user));
    Json(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_come_from_configuration_and_the_state_is_stable() {
        let doc = document("a@x.test");
        let api = doc["apiUrl"].as_str().unwrap();
        assert!(
            api.starts_with("https://") && api.ends_with("/jmap/api/"),
            "{api}"
        );
        assert!(doc["downloadUrl"].as_str().unwrap().contains("{blobId}"));
        assert_eq!(state_of("a@x.test"), state_of("a@x.test"));
        assert_ne!(state_of("a@x.test"), state_of("b@x.test"));
        assert!(
            doc["capabilities"]
                .get("urn:ietf:params:jmap:websocket")
                .is_none()
        );
    }
}
