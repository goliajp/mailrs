//! Top-level method dispatcher.
//!
//! Routes a single `(method, args)` pair to the right handler. The HTTP-level
//! "method calls" envelope (RFC 8620 §3.4) is the caller's job — this crate
//! is intentionally framework-agnostic.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::JmapMethodError;
use crate::methods::{
    handle_email_get, handle_email_query, handle_email_set, handle_email_submission_set,
    handle_mailbox_get, handle_mailbox_query, handle_thread_get,
};
use crate::refs::resolve_references;
use crate::store::MailStore;

/// JMAP capability URIs we advertise / understand (RFC 8620 §2, RFC 8621 §1).
pub const JMAP_CORE_CAP: &str = "urn:ietf:params:jmap:core";
pub const JMAP_MAIL_CAP: &str = "urn:ietf:params:jmap:mail";
pub const JMAP_SUBMISSION_CAP: &str = "urn:ietf:params:jmap:submission";

/// Wire shape of an inbound JMAP request (RFC 8620 §3.4).
///
/// `method_calls` is `[(name, args, call_id)]` per the spec. `using` lists
/// capability URIs the client claims to need; we accept any.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapRequest {
    #[serde(default)]
    pub using: Vec<String>,
    pub method_calls: Vec<(String, Value, String)>,
}

/// Wire shape of the response envelope (RFC 8620 §3.4).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapResponse {
    pub method_responses: Vec<(String, Value, String)>,
    pub session_state: String,
}

/// Dispatch a single method call.
///
/// Returns either `(method_name, result_value)` for success or a method-error
/// envelope (already in JMAP wire shape) on failure. The dispatcher itself
/// only converts unknown method names; per-method handlers own all other
/// error mapping.
pub async fn dispatch_method(
    method: &str,
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    match method {
        "Mailbox/get" => handle_mailbox_get(args, user, store).await,
        "Mailbox/query" => handle_mailbox_query(args, user, store).await,
        "Email/get" => handle_email_get(args, user, store).await,
        "Email/query" => handle_email_query(args, user, store).await,
        "Email/set" => handle_email_set(args, user, store).await,
        "Thread/get" => handle_thread_get(args, user, store).await,
        "EmailSubmission/set" => handle_email_submission_set(args, user, store).await,
        other => Err(JmapMethodError::UnknownMethod(other.to_string())),
    }
}

/// Run an entire JMAP request through the dispatcher and return the response
/// envelope. Back-references between method calls are resolved before each
/// dispatch.
pub async fn dispatch_request(
    request: JmapRequest,
    user: &str,
    store: &dyn MailStore,
) -> JmapResponse {
    let mut responses: Vec<(String, Value, String)> = Vec::new();

    for (method, mut args, call_id) in request.method_calls {
        resolve_references(&mut args, &responses);

        match dispatch_method(&method, &args, user, store).await {
            Ok((name, value)) => responses.push((name, value, call_id)),
            Err(err) => responses.push(("error".to_string(), err.to_json(), call_id)),
        }
    }

    JmapResponse {
        method_responses: responses,
        session_state: "0".to_string(),
    }
}
