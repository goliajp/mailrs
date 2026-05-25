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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MailStore, StoreError};
    use crate::types::{Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    fn msg(id: i64) -> Message {
        Message {
            id,
            mailbox_id: 1,
            uid: id as u32,
            sender: "alice@example.com".into(),
            recipients: "bob@example.com".into(),
            subject: "subj".into(),
            date: 0,
            size: 10,
            flags: 0,
            internal_date: 0,
            message_id: format!("m-{id}"),
            in_reply_to: String::new(),
            thread_id: format!("t-{id}"),
            user_address: "u".into(),
            new_content: None,
            blob_id: format!("b-{id}"),
        }
    }

    #[derive(Default)]
    struct MemStore {
        messages: Mutex<Vec<Message>>,
        // raw bytes by db_id; None means read_message_raw returns None
        raw: Mutex<Vec<(i64, Option<Vec<u8>>)>>,
        // submit outcome by db_id
        submit_outcome: Mutex<Vec<(i64, SubmissionResult)>>,
    }

    impl MemStore {
        fn add(&self, m: Message, raw: Option<Vec<u8>>, outcome: SubmissionResult) {
            let id = m.id;
            self.messages.lock().unwrap().push(m);
            self.raw.lock().unwrap().push((id, raw));
            self.submit_outcome.lock().unwrap().push((id, outcome));
        }
    }

    #[async_trait]
    impl MailStore for MemStore {
        async fn list_mailboxes(&self, _: &str) -> Result<Vec<Mailbox>, StoreError> {
            Ok(vec![])
        }
        async fn mailbox_status(&self, _: i64) -> Result<MailboxCounts, StoreError> {
            Ok(MailboxCounts::default())
        }
        async fn list_messages(&self, _: i64, _: u32, _: u32) -> Result<Vec<Message>, StoreError> {
            Ok(vec![])
        }
        async fn get_message_by_db_id(
            &self,
            _: &str,
            id: i64,
        ) -> Result<Option<Message>, StoreError> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id == id)
                .cloned())
        }
        async fn list_thread_messages(&self, _: &str, _: &str) -> Result<Vec<Message>, StoreError> {
            Ok(vec![])
        }
        async fn update_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> {
            Ok(())
        }
        async fn add_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> {
            Ok(())
        }
        async fn read_message_raw(&self, m: &Message) -> Option<Vec<u8>> {
            self.raw
                .lock()
                .unwrap()
                .iter()
                .find(|(id, _)| *id == m.id)
                .and_then(|(_, b)| b.clone())
        }
        fn parse_message(&self, _: &[u8]) -> ParsedBody {
            ParsedBody::default()
        }
        async fn submit_message(&self, _: &str, m: &Message, _: &[u8]) -> SubmissionResult {
            self.submit_outcome
                .lock()
                .unwrap()
                .iter()
                .find(|(id, _)| *id == m.id)
                .map(|(_, r)| r.clone())
                .unwrap_or(SubmissionResult {
                    success: false,
                    message: None,
                })
        }
    }

    #[tokio::test]
    async fn submission_no_create_section_returns_empty() {
        let s = MemStore::default();
        let args = json!({});
        let (method, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        assert_eq!(method, "EmailSubmission/set");
        assert_eq!(result["created"].as_object().unwrap().len(), 0);
        assert_eq!(result["notCreated"].as_object().unwrap().len(), 0);
        // No state change when nothing happens
        assert_eq!(result["newState"], "0");
    }

    #[tokio::test]
    async fn submission_missing_email_id_in_not_created() {
        let s = MemStore::default();
        let args = json!({ "create": { "c1": {} } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        assert!(result["notCreated"].as_object().unwrap().contains_key("c1"));
        let err = &result["notCreated"]["c1"];
        assert_eq!(err["type"], "invalidProperties");
    }

    #[tokio::test]
    async fn submission_malformed_email_id_in_not_created() {
        let s = MemStore::default();
        let args = json!({ "create": { "c1": { "emailId": "garbage" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        let err = &result["notCreated"]["c1"];
        assert_eq!(err["type"], "invalidProperties");
    }

    #[tokio::test]
    async fn submission_missing_db_id_in_not_created() {
        let s = MemStore::default();
        let args = json!({ "create": { "c1": { "emailId": "msg-9999" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        let err = &result["notCreated"]["c1"];
        assert_eq!(err["type"], "notFound");
    }

    #[tokio::test]
    async fn submission_no_raw_bytes_in_not_created() {
        let s = MemStore::default();
        s.add(
            msg(7),
            None,
            SubmissionResult {
                success: true,
                message: None,
            },
        );
        let args = json!({ "create": { "c1": { "emailId": "msg-7" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        let err = &result["notCreated"]["c1"];
        assert_eq!(err["type"], "serverFail");
    }

    #[tokio::test]
    async fn submission_success_populates_created() {
        let s = MemStore::default();
        s.add(
            msg(42),
            Some(b"From: x\r\n\r\nhi".to_vec()),
            SubmissionResult {
                success: true,
                message: None,
            },
        );
        let args = json!({ "create": { "c1": { "emailId": "msg-42" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        let created = &result["created"]["c1"];
        assert_eq!(created["id"], "sub-42");
        assert_eq!(created["emailId"], "msg-42");
        assert_eq!(created["undoStatus"], "final");
        assert_eq!(result["newState"], "1");
    }

    #[tokio::test]
    async fn submission_failure_uses_message_in_description() {
        let s = MemStore::default();
        s.add(
            msg(5),
            Some(b"raw".to_vec()),
            SubmissionResult {
                success: false,
                message: Some("relay denied".into()),
            },
        );
        let args = json!({ "create": { "c1": { "emailId": "msg-5" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        let err = &result["notCreated"]["c1"];
        assert_eq!(err["type"], "serverFail");
        assert_eq!(err["description"], "relay denied");
    }

    #[tokio::test]
    async fn submission_failure_without_message_uses_default() {
        let s = MemStore::default();
        s.add(
            msg(5),
            Some(b"raw".to_vec()),
            SubmissionResult {
                success: false,
                message: None,
            },
        );
        let args = json!({ "create": { "c1": { "emailId": "msg-5" } } });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        assert_eq!(result["notCreated"]["c1"]["description"], "delivery failed");
    }

    #[tokio::test]
    async fn submission_multiple_creates_partition() {
        let s = MemStore::default();
        s.add(
            msg(1),
            Some(b"raw1".to_vec()),
            SubmissionResult {
                success: true,
                message: None,
            },
        );
        s.add(
            msg(2),
            None,
            SubmissionResult {
                success: true,
                message: None,
            },
        );
        let args = json!({
            "create": {
                "good": { "emailId": "msg-1" },
                "no_raw": { "emailId": "msg-2" },
                "bad_id": { "emailId": "not-an-id" },
                "no_field": {}
            }
        });
        let (_, result) = handle_email_submission_set(&args, "u", &s).await.unwrap();
        assert_eq!(result["created"].as_object().unwrap().len(), 1);
        assert_eq!(result["notCreated"].as_object().unwrap().len(), 3);
        assert!(result["created"].as_object().unwrap().contains_key("good"));
    }
}
