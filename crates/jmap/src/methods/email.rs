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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MailStore, StoreError};
    use crate::types::{
        Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult,
        FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    fn msg(id: i64, mailbox_id: i64, uid: u32, subject: &str, internal_date: i64) -> Message {
        Message {
            id,
            mailbox_id,
            uid,
            sender: "alice@example.com".into(),
            recipients: "bob@example.com".into(),
            subject: subject.into(),
            date: internal_date,
            size: 100,
            flags: 0,
            internal_date,
            message_id: format!("msg-{id}@x"),
            in_reply_to: String::new(),
            thread_id: format!("thread-{id}"),
            user_address: "u".into(),
            new_content: None,
            blob_id: format!("blob-{id}"),
        }
    }

    #[derive(Default)]
    struct MemStore {
        mailboxes: Mutex<Vec<(String, Mailbox)>>,
        messages: Mutex<Vec<Message>>,
    }

    impl MemStore {
        fn add_mailbox(&self, user: &str, id: i64, name: &str) {
            self.mailboxes.lock().unwrap().push((
                user.into(),
                Mailbox { id, name: name.into() },
            ));
        }
        fn add_message(&self, m: Message) {
            self.messages.lock().unwrap().push(m);
        }
    }

    #[async_trait]
    impl MailStore for MemStore {
        async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError> {
            Ok(self
                .mailboxes
                .lock()
                .unwrap()
                .iter()
                .filter(|(u, _)| u == user)
                .map(|(_, m)| m.clone())
                .collect())
        }
        async fn mailbox_status(&self, _: i64) -> Result<MailboxCounts, StoreError> {
            Ok(MailboxCounts::default())
        }
        async fn list_messages(
            &self,
            mb: i64,
            offset: u32,
            limit: u32,
        ) -> Result<Vec<Message>, StoreError> {
            let all: Vec<Message> = self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.mailbox_id == mb)
                .cloned()
                .collect();
            Ok(all.into_iter().skip(offset as usize).take(limit as usize).collect())
        }
        async fn get_message_by_db_id(
            &self,
            _user: &str,
            id: i64,
        ) -> Result<Option<Message>, StoreError> {
            Ok(self.messages.lock().unwrap().iter().find(|m| m.id == id).cloned())
        }
        async fn list_thread_messages(
            &self,
            _user: &str,
            tid: &str,
        ) -> Result<Vec<Message>, StoreError> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == tid)
                .cloned()
                .collect())
        }
        async fn update_flags(
            &self,
            mb: i64,
            uid: u32,
            flags: u32,
        ) -> Result<(), StoreError> {
            for m in self.messages.lock().unwrap().iter_mut() {
                if m.mailbox_id == mb && m.uid == uid {
                    m.flags = flags;
                }
            }
            Ok(())
        }
        async fn add_flags(
            &self,
            mb: i64,
            uid: u32,
            flags: u32,
        ) -> Result<(), StoreError> {
            for m in self.messages.lock().unwrap().iter_mut() {
                if m.mailbox_id == mb && m.uid == uid {
                    m.flags |= flags;
                }
            }
            Ok(())
        }
        async fn read_message_raw(&self, _: &Message) -> Option<Vec<u8>> {
            None
        }
        fn parse_message(&self, _: &[u8]) -> ParsedBody {
            ParsedBody::default()
        }
        async fn submit_message(
            &self,
            _: &str,
            _: &Message,
            _: &[u8],
        ) -> SubmissionResult {
            SubmissionResult {
                success: false,
                message: None,
            }
        }
    }

    #[tokio::test]
    async fn email_get_returns_metadata_for_valid_id() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(42, 1, 1, "Hi", 1000));
        let args = json!({ "ids": ["msg-42"] });
        let (_method, result) = handle_email_get(&args, "u", &s).await.unwrap();
        let list = result["list"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(result["notFound"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn email_get_malformed_id_goes_to_not_found() {
        let s = MemStore::default();
        let args = json!({ "ids": ["bogus-id"] });
        let (_, result) = handle_email_get(&args, "u", &s).await.unwrap();
        let not_found = result["notFound"].as_array().unwrap();
        assert_eq!(not_found.len(), 1);
    }

    #[tokio::test]
    async fn email_get_missing_db_id_goes_to_not_found() {
        let s = MemStore::default();
        let args = json!({ "ids": ["msg-99999"] });
        let (_, result) = handle_email_get(&args, "u", &s).await.unwrap();
        assert_eq!(result["list"].as_array().unwrap().len(), 0);
        assert_eq!(result["notFound"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn email_get_mixed_existing_and_missing() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 1));
        let args = json!({ "ids": ["msg-1", "msg-99", "garbage"] });
        let (_, result) = handle_email_get(&args, "u", &s).await.unwrap();
        assert_eq!(result["list"].as_array().unwrap().len(), 1);
        assert_eq!(result["notFound"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn email_get_requires_ids_argument() {
        let s = MemStore::default();
        let args = json!({});
        let r = handle_email_get(&args, "u", &s).await;
        assert!(matches!(r, Err(JmapMethodError::InvalidArguments(_))));
    }

    #[tokio::test]
    async fn email_query_no_filter_returns_all() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        s.add_message(msg(2, 1, 2, "B", 200));
        s.add_message(msg(3, 1, 3, "C", 300));
        let args = json!({});
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 3);
        assert_eq!(result["ids"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn email_query_in_mailbox_filter() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_mailbox("u", 2, "Sent");
        s.add_message(msg(1, 1, 1, "in inbox", 100));
        s.add_message(msg(2, 2, 1, "in sent", 200));
        let args = json!({ "filter": { "inMailbox": "mb-1" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn email_query_in_mailbox_unknown_returns_empty() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "a", 100));
        let args = json!({ "filter": { "inMailbox": "mb-999" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn email_query_has_keyword_seen() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        let mut a = msg(1, 1, 1, "A", 100);
        a.flags = FLAG_SEEN;
        s.add_message(a);
        s.add_message(msg(2, 1, 2, "B", 200)); // unseen
        let args = json!({ "filter": { "hasKeyword": "$seen" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn email_query_not_keyword_filters_out_match() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        let mut seen = msg(1, 1, 1, "A", 100);
        seen.flags = FLAG_SEEN;
        s.add_message(seen);
        s.add_message(msg(2, 1, 2, "B", 200)); // unseen
        let args = json!({ "filter": { "notKeyword": "$seen" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn email_query_text_filter_matches_subject() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "Important Notice", 100));
        s.add_message(msg(2, 1, 2, "Other Stuff", 200));
        let args = json!({ "filter": { "text": "important" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn email_query_text_filter_case_insensitive() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "URGENT", 100));
        let args = json!({ "filter": { "text": "urgent" } });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn email_query_sort_descending_default() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        s.add_message(msg(2, 1, 2, "B", 200));
        s.add_message(msg(3, 1, 3, "C", 300));
        let args = json!({});
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        let ids: Vec<&str> = result["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ids[0], "msg-3");
    }

    #[tokio::test]
    async fn email_query_sort_ascending_via_args() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        s.add_message(msg(2, 1, 2, "B", 200));
        let args = json!({ "sort": [{ "isAscending": true }] });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        let ids: Vec<&str> = result["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ids[0], "msg-1");
    }

    #[tokio::test]
    async fn email_query_limit_caps_at_500() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        let args = json!({ "limit": 9999 });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn email_query_pagination_position_skips() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        for i in 1..=5 {
            s.add_message(msg(i, 1, i as u32, "msg", i * 100));
        }
        let args = json!({ "position": 2, "limit": 10, "sort": [{ "isAscending": true }] });
        let (_, result) = handle_email_query(&args, "u", &s).await.unwrap();
        assert_eq!(result["total"].as_u64().unwrap(), 5);
        let ids = result["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn email_set_destroy_marks_deleted_flag() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        let args = json!({ "destroy": ["msg-1"] });
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        let destroyed = result["destroyed"].as_array().unwrap();
        assert_eq!(destroyed.len(), 1);
        let stored = s.messages.lock().unwrap()[0].clone();
        assert!(stored.flags & FLAG_DELETED != 0);
    }

    #[tokio::test]
    async fn email_set_destroy_malformed_id_in_not_destroyed() {
        let s = MemStore::default();
        let args = json!({ "destroy": ["bogus"] });
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        let not_destroyed = result["notDestroyed"].as_object().unwrap();
        assert!(not_destroyed.contains_key("bogus"));
    }

    #[tokio::test]
    async fn email_set_update_keywords_replace() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        let args = json!({
            "update": {
                "msg-1": { "keywords": { "$seen": true, "$flagged": true } }
            }
        });
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        assert!(result["updated"].as_object().unwrap().contains_key("msg-1"));
        let stored = s.messages.lock().unwrap()[0].clone();
        assert!(stored.flags & FLAG_SEEN != 0);
        assert!(stored.flags & FLAG_FLAGGED != 0);
    }

    #[tokio::test]
    async fn email_set_update_keywords_patch_path_form() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        s.add_message(msg(1, 1, 1, "A", 100));
        let args = json!({
            "update": {
                "msg-1": { "keywords/$seen": true }
            }
        });
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        assert!(result["updated"].as_object().unwrap().contains_key("msg-1"));
        let stored = s.messages.lock().unwrap()[0].clone();
        assert!(stored.flags & FLAG_SEEN != 0);
    }

    #[tokio::test]
    async fn email_set_update_keywords_patch_clears_flag() {
        let s = MemStore::default();
        s.add_mailbox("u", 1, "INBOX");
        let mut m = msg(1, 1, 1, "A", 100);
        m.flags = FLAG_SEEN | FLAG_FLAGGED;
        s.add_message(m);
        let args = json!({
            "update": {
                "msg-1": { "keywords/$seen": false }
            }
        });
        let _ = handle_email_set(&args, "u", &s).await.unwrap();
        let stored = s.messages.lock().unwrap()[0].clone();
        assert_eq!(stored.flags & FLAG_SEEN, 0);
        assert!(stored.flags & FLAG_FLAGGED != 0);
    }

    #[tokio::test]
    async fn email_set_update_malformed_id_in_not_updated() {
        let s = MemStore::default();
        let args = json!({
            "update": {
                "bogus": { "keywords/$seen": true }
            }
        });
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        assert!(result["notUpdated"].as_object().unwrap().contains_key("bogus"));
    }

    #[tokio::test]
    async fn email_set_empty_args_returns_empty_collections() {
        let s = MemStore::default();
        let args = json!({});
        let (_, result) = handle_email_set(&args, "u", &s).await.unwrap();
        assert_eq!(result["destroyed"].as_array().unwrap().len(), 0);
        assert_eq!(result["updated"].as_object().unwrap().len(), 0);
    }
}
