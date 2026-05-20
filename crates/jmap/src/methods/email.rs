//! `Email/get`, `Email/query`, `Email/set` (RFC 8621 §4).

use serde_json::Value;

use crate::build::{build_email_meta, extend_with_body, wants_body};
use crate::error::JmapMethodError;
use crate::flags::{keyword_to_flag, keywords_to_flags};
use crate::ids::{format_email_id, parse_email_db_id, parse_mailbox_db_id};
use crate::store::MailStore;
use crate::types::FLAG_DELETED;

/// RFC 8621 §4.2 — `Email/get`.
pub async fn handle_email_get(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let ids = args
        .get("ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| JmapMethodError::InvalidArguments("ids is required".into()))?;

    let properties: Option<Vec<&str>> = args
        .get("properties")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    for id_val in ids {
        let Some(id_str) = id_val.as_str() else {
            continue;
        };

        let Some(db_id) = parse_email_db_id(id_str) else {
            not_found.push(Value::String(id_str.to_string()));
            continue;
        };

        let msg = match store.get_message_by_db_id(user, db_id).await {
            Ok(Some(m)) => m,
            _ => {
                not_found.push(Value::String(id_str.to_string()));
                continue;
            }
        };

        let mut obj = build_email_meta(&msg, id_str, &properties);

        if wants_body(&properties) {
            let raw = store.read_message_raw(&msg).await;
            let parsed = raw.as_deref().map(|bytes| store.parse_message(bytes));
            extend_with_body(&mut obj, parsed.as_ref(), &properties);
        }

        list.push(obj);
    }

    Ok((
        "Email/get".to_string(),
        serde_json::json!({
            "accountId": user,
            "state": "0",
            "list": list,
            "notFound": not_found
        }),
    ))
}

/// RFC 8621 §4.4 — `Email/query`. Filters supported: `inMailbox`, `text`,
/// `hasKeyword`, `notKeyword`. Sorts by `receivedAt` direction only.
pub async fn handle_email_query(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let filter = args.get("filter");

    let mailbox_id_filter = filter
        .and_then(|f| f.get("inMailbox"))
        .and_then(|v| v.as_str())
        .and_then(parse_mailbox_db_id);

    let text_filter = filter
        .and_then(|f| f.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    let has_keyword = filter
        .and_then(|f| f.get("hasKeyword"))
        .and_then(|v| v.as_str());

    let not_keyword = filter
        .and_then(|f| f.get("notKeyword"))
        .and_then(|v| v.as_str());

    let position = args.get("position").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(500) as u32;

    let sort_desc = args
        .get("sort")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("isAscending"))
        .and_then(|v| v.as_bool())
        .map(|asc| !asc)
        .unwrap_or(true);

    let mailboxes = store
        .list_mailboxes(user)
        .await
        .map_err(|e| JmapMethodError::ServerFail(e.to_string()))?;

    let mailbox_ids: Vec<i64> = if let Some(mb_id) = mailbox_id_filter {
        if mailboxes.iter().any(|m| m.id == mb_id) {
            vec![mb_id]
        } else {
            vec![]
        }
    } else {
        mailboxes.iter().map(|m| m.id).collect()
    };

    let mut all_messages = Vec::new();
    for mb_id in &mailbox_ids {
        let msgs = store
            .list_messages(*mb_id, 0, 10_000)
            .await
            .unwrap_or_default();
        all_messages.extend(msgs);
    }

    if let Some(kw) = has_keyword {
        let flag_bit = keyword_to_flag(kw);
        if flag_bit != 0 {
            all_messages.retain(|m| m.flags & flag_bit != 0);
        }
    }
    if let Some(kw) = not_keyword {
        let flag_bit = keyword_to_flag(kw);
        if flag_bit != 0 {
            all_messages.retain(|m| m.flags & flag_bit == 0);
        }
    }

    if let Some(ref query) = text_filter {
        all_messages.retain(|m| {
            m.subject.to_lowercase().contains(query)
                || m.sender.to_lowercase().contains(query)
                || m.recipients.to_lowercase().contains(query)
        });
    }

    if sort_desc {
        all_messages.sort_by_key(|a| std::cmp::Reverse(a.internal_date));
    } else {
        all_messages.sort_by_key(|a| a.internal_date);
    }

    let total = all_messages.len();
    let ids: Vec<String> = all_messages
        .iter()
        .skip(position as usize)
        .take(limit as usize)
        .map(|m| format_email_id(m.id))
        .collect();

    Ok((
        "Email/query".to_string(),
        serde_json::json!({
            "accountId": user,
            "queryState": "0",
            "canCalculateChanges": false,
            "position": position,
            "total": total,
            "ids": ids
        }),
    ))
}

/// RFC 8621 §4.6 — `Email/set`. Supports `update` (keyword changes only) and
/// `destroy` (sets `\Deleted`). Creates are not supported — clients should
/// use the standard SMTP submission path or `EmailSubmission/set` with a
/// staged draft.
pub async fn handle_email_set(
    args: &Value,
    user: &str,
    store: &dyn MailStore,
) -> Result<(String, Value), JmapMethodError> {
    let mut updated = serde_json::Map::new();
    let mut destroyed: Vec<String> = Vec::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();

    if let Some(updates) = args.get("update").and_then(|v| v.as_object()) {
        for (email_id, patch) in updates {
            let Some(db_id) = parse_email_db_id(email_id) else {
                not_updated.insert(
                    email_id.clone(),
                    serde_json::json!({"type": "notFound"}),
                );
                continue;
            };

            let msg = match store.get_message_by_db_id(user, db_id).await {
                Ok(Some(m)) => m,
                _ => {
                    not_updated.insert(
                        email_id.clone(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            let new_flags = if let Some(kw) = patch.get("keywords") {
                keywords_to_flags(kw)
            } else {
                // patch shape `keywords/$seen: true|false`
                let mut flags = msg.flags;
                if let Some(obj) = patch.as_object() {
                    for (key, val) in obj {
                        if let Some(kw_name) = key.strip_prefix("keywords/") {
                            let flag_bit = keyword_to_flag(kw_name);
                            if flag_bit != 0 {
                                if val.as_bool() == Some(true) {
                                    flags |= flag_bit;
                                } else {
                                    flags &= !flag_bit;
                                }
                            }
                        }
                    }
                }
                flags
            };

            match store.update_flags(msg.mailbox_id, msg.uid, new_flags).await {
                Ok(_) => {
                    updated.insert(email_id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(
                        email_id.clone(),
                        serde_json::json!({"type": "serverFail", "description": e.to_string()}),
                    );
                }
            }
        }
    }

    if let Some(destroy_ids) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_ids {
            let Some(email_id) = id_val.as_str() else {
                continue;
            };

            let Some(db_id) = parse_email_db_id(email_id) else {
                not_destroyed.insert(
                    email_id.to_string(),
                    serde_json::json!({"type": "notFound"}),
                );
                continue;
            };

            let msg = match store.get_message_by_db_id(user, db_id).await {
                Ok(Some(m)) => m,
                _ => {
                    not_destroyed.insert(
                        email_id.to_string(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            match store.add_flags(msg.mailbox_id, msg.uid, FLAG_DELETED).await {
                Ok(_) => destroyed.push(email_id.to_string()),
                Err(e) => {
                    not_destroyed.insert(
                        email_id.to_string(),
                        serde_json::json!({"type": "serverFail", "description": e.to_string()}),
                    );
                }
            }
        }
    }

    Ok((
        "Email/set".to_string(),
        serde_json::json!({
            "accountId": user,
            "oldState": "0",
            "newState": "1",
            "updated": updated,
            "destroyed": destroyed,
            "notUpdated": not_updated,
            "notDestroyed": not_destroyed
        }),
    ))
}
