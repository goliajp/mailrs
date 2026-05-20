//! `EmailSubmission/set` (RFC 8621 §7).

use serde_json::Value;

use crate::error::JmapMethodError;
use crate::ids::parse_email_db_id;
use crate::store::MailStore;

/// RFC 8621 §7.3 — `EmailSubmission/set`. Only `create` is implemented;
/// updates and destroys on submission objects are ignored.
pub async fn handle_email_submission_set(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let Some(create) = args.get("create").and_then(|v| v.as_object()) else {
        return Ok((
            "EmailSubmission/set".to_string(),
            serde_json::json!({
                "accountId": user,
                "oldState": "0",
                "newState": "0",
                "created": {},
                "notCreated": {}
            }),
        ));
    };

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    for (creation_id, submission) in create {
        let Some(email_id) = submission.get("emailId").and_then(|v| v.as_str()) else {
            not_created.insert(
                creation_id.clone(),
                serde_json::json!({
                    "type": "invalidProperties",
                    "description": "emailId is required"
                }),
            );
            continue;
        };

        let Some(db_id) = parse_email_db_id(email_id) else {
            not_created.insert(
                creation_id.clone(),
                serde_json::json!({"type": "invalidProperties", "description": "invalid emailId"}),
            );
            continue;
        };

        let msg = match store.get_message_by_db_id(user, db_id).await {
            Ok(Some(m)) => m,
            _ => {
                not_created.insert(
                    creation_id.clone(),
                    serde_json::json!({"type": "notFound", "description": "email not found"}),
                );
                continue;
            }
        };

        let Some(raw) = store.read_message_raw(&msg).await else {
            not_created.insert(
                creation_id.clone(),
                serde_json::json!({"type": "serverFail", "description": "could not read message"}),
            );
            continue;
        };

        let result = store.submit_message(user, &msg, &raw).await;

        if result.success {
            created.insert(
                creation_id.clone(),
                serde_json::json!({
                    "id": format!("sub-{}", db_id),
                    "emailId": email_id,
                    "undoStatus": "final"
                }),
            );
        } else {
            let err_msg = result.message.as_deref().unwrap_or("delivery failed");
            not_created.insert(
                creation_id.clone(),
                serde_json::json!({"type": "serverFail", "description": err_msg}),
            );
        }
    }

    Ok((
        "EmailSubmission/set".to_string(),
        serde_json::json!({
            "accountId": user,
            "oldState": "0",
            "newState": "1",
            "created": created,
            "notCreated": not_created
        }),
    ))
}
