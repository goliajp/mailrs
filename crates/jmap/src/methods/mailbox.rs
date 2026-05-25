//! `Mailbox/get` and `Mailbox/query` (RFC 8621 §2).

use serde_json::Value;

use crate::error::JmapMethodError;
use crate::ids::{format_mailbox_id, parse_mailbox_db_id};
use crate::store::MailStore;

/// RFC 8621 §2.3 — `Mailbox/get`.
pub async fn handle_mailbox_get(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let mailboxes = store
        .list_mailboxes(user)
        .await
        .map_err(|e| JmapMethodError::ServerFail(e.to_string()))?;

    let requested_ids = args.get("ids").and_then(|v| v.as_array());

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    let iter_mailboxes: Vec<_> = if let Some(ids) = requested_ids {
        let id_strings: Vec<String> = ids
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let mut result = Vec::new();
        for id_str in &id_strings {
            if let Some(db_id) = parse_mailbox_db_id(id_str) {
                match mailboxes.iter().find(|m| m.id == db_id) {
                    Some(m) => result.push(m.clone()),
                    None => not_found.push(Value::String(id_str.clone())),
                }
            } else {
                not_found.push(Value::String(id_str.clone()));
            }
        }
        result
    } else {
        mailboxes.clone()
    };

    for mb in &iter_mailboxes {
        let counts = store.mailbox_status(mb.id).await.unwrap_or_default();
        let mb_id = format_mailbox_id(mb.id);
        let mut obj = serde_json::json!({
            "id": mb_id,
            "name": mb.name,
            "parentId": null,
            "sortOrder": 0,
            "totalEmails": counts.total,
            "unreadEmails": counts.unread,
            "totalThreads": counts.total,
            "unreadThreads": counts.unread,
            "myRights": {
                "mayReadItems": true,
                "mayAddItems": true,
                "mayRemoveItems": true,
                "maySetSeen": true,
                "maySetKeywords": true,
                "mayCreateChild": false,
                "mayRename": false,
                "mayDelete": false,
                "maySubmit": true
            },
            "isSubscribed": true
        });

        if let Some(role) = mailbox_role(&mb.name) {
            obj["role"] = Value::String(role.to_string());
        }

        list.push(obj);
    }

    Ok((
        "Mailbox/get".to_string(),
        serde_json::json!({
            "accountId": user,
            "state": "0",
            "list": list,
            "notFound": not_found
        }),
    ))
}

/// RFC 8621 §2.4 — `Mailbox/query`. Currently returns the entire mailbox
/// list; filter / sort are accepted but ignored.
pub async fn handle_mailbox_query(
    _args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let mailboxes = store
        .list_mailboxes(user)
        .await
        .map_err(|e| JmapMethodError::ServerFail(e.to_string()))?;

    let ids: Vec<String> = mailboxes
        .iter()
        .map(|mb| format_mailbox_id(mb.id))
        .collect();

    Ok((
        "Mailbox/query".to_string(),
        serde_json::json!({
            "accountId": user,
            "queryState": "0",
            "canCalculateChanges": false,
            "position": 0,
            "total": ids.len(),
            "ids": ids
        }),
    ))
}

/// Map a mailbox name to its JMAP role (RFC 8621 §2.1). Returns `None` for
/// custom folders; the dispatcher omits the field in that case.
pub fn mailbox_role(name: &str) -> Option<&'static str> {
    match name {
        "INBOX" => Some("inbox"),
        "Sent" => Some("sent"),
        "Drafts" => Some("drafts"),
        "Trash" => Some("trash"),
        "Junk" => Some("junk"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_known_names() {
        assert_eq!(mailbox_role("INBOX"), Some("inbox"));
        assert_eq!(mailbox_role("Sent"), Some("sent"));
        assert_eq!(mailbox_role("Drafts"), Some("drafts"));
        assert_eq!(mailbox_role("Trash"), Some("trash"));
        assert_eq!(mailbox_role("Junk"), Some("junk"));
    }

    #[test]
    fn role_unknown_is_none() {
        assert_eq!(mailbox_role("Archive"), None);
        assert_eq!(mailbox_role(""), None);
    }
}
