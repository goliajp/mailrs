//! `Thread/get` (RFC 8621 §3).

use serde_json::Value;

use crate::error::JmapMethodError;
use crate::ids::format_email_id;
use crate::store::MailStore;

/// RFC 8621 §3.2 — `Thread/get`.
pub async fn handle_thread_get(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let ids = args
        .get("ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| JmapMethodError::InvalidArguments("ids is required".into()))?;

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    for id_val in ids {
        let Some(thread_id) = id_val.as_str() else {
            continue;
        };

        let messages = store
            .list_thread_messages(user, thread_id)
            .await
            .unwrap_or_default();

        if messages.is_empty() {
            not_found.push(Value::String(thread_id.to_string()));
            continue;
        }

        let email_ids: Vec<String> = messages.iter().map(|m| format_email_id(m.id)).collect();

        list.push(serde_json::json!({
            "id": thread_id,
            "emailIds": email_ids
        }));
    }

    Ok((
        "Thread/get".to_string(),
        serde_json::json!({
            "accountId": user,
            "state": "0",
            "list": list,
            "notFound": not_found
        }),
    ))
}
