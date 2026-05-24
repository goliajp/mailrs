//! Tests for `tools` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;
use schemars::schema_for;

#[test]
fn send_email_params_schema_generation() {
    let schema = schema_for!(SendEmailParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("to"));
    assert!(json.contains("subject"));
    assert!(json.contains("body"));
}

#[test]
fn read_email_params_schema_generation() {
    let schema = schema_for!(ReadEmailParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("uid"));
}

#[test]
fn search_emails_params_schema_generation() {
    let schema = schema_for!(SearchEmailsParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("query"));
    assert!(json.contains("limit"));
}

#[test]
fn reply_email_params_schema_generation() {
    let schema = schema_for!(ReplyEmailParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
    assert!(json.contains("body"));
}

#[test]
fn list_conversations_params_schema_generation() {
    let schema = schema_for!(ListConversationsParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("limit"));
    assert!(json.contains("category"));
}

#[test]
fn send_email_params_deserialize_empty_to() {
    let json = r#"{"to": [], "subject": "test", "body": "hello"}"#;
    let params: SendEmailParams = serde_json::from_str(json).unwrap();
    assert!(params.to.is_empty());
}

#[test]
fn send_email_params_deserialize_with_optional_fields() {
    let json = r#"{"to": ["a@b.com"], "subject": "test", "body": "hello", "from": "x@y.com", "cc": ["c@d.com"]}"#;
    let params: SendEmailParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.from.as_deref(), Some("x@y.com"));
    assert_eq!(params.cc.as_ref().unwrap().len(), 1);
}

#[test]
fn list_conversations_params_defaults() {
    let json = r#"{}"#;
    let params: ListConversationsParams = serde_json::from_str(json).unwrap();
    assert!(params.limit.is_none());
    assert!(params.category.is_none());
}

#[test]
fn create_account_params_schema_generation() {
    let schema = schema_for!(CreateAccountParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("address"));
    assert!(json.contains("domain"));
}

#[test]
fn create_account_params_deserialize() {
    let json = r#"{"address": "new@golia.jp", "domain": "golia.jp", "display_name": "New", "password": "secret"}"#;
    let params: CreateAccountParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.address, "new@golia.jp");
    assert_eq!(params.domain, "golia.jp");
}

#[test]
fn create_account_params_defaults() {
    let json = r#"{"address": "a@b.com", "domain": "b.com"}"#;
    let params: CreateAccountParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.display_name, "");
    assert_eq!(params.password, "");
}

#[test]
fn add_account_to_group_params_schema() {
    let schema = schema_for!(AddAccountToGroupParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("address"));
    assert!(json.contains("group_id"));
}

#[test]
fn get_account_permissions_params_schema() {
    let schema = schema_for!(GetAccountPermissionsParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("address"));
}

#[test]
fn list_accounts_params_empty() {
    let json = r#"{}"#;
    let _params: ListAccountsParams = serde_json::from_str(json).unwrap();
}

#[test]
fn list_groups_params_empty() {
    let json = r#"{}"#;
    let _params: ListGroupsParams = serde_json::from_str(json).unwrap();
}

#[test]
fn list_domains_params_empty() {
    let json = r#"{}"#;
    let _params: ListDomainsParams = serde_json::from_str(json).unwrap();
}

#[test]
fn add_domain_params_schema() {
    let schema = schema_for!(AddDomainParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("name"));
}

#[test]
fn remove_domain_params_schema() {
    let schema = schema_for!(RemoveDomainParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("name"));
}

#[test]
fn list_aliases_params_empty() {
    let json = r#"{}"#;
    let _params: ListAliasesParams = serde_json::from_str(json).unwrap();
}

#[test]
fn add_alias_params_schema() {
    let schema = schema_for!(AddAliasParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("source_address"));
    assert!(json.contains("target_address"));
    assert!(json.contains("domain"));
    assert!(json.contains("alias_type"));
}

#[test]
fn add_alias_params_default_type() {
    let json = r#"{"source_address": "a@b.com", "target_address": "c@b.com", "domain": "b.com"}"#;
    let params: AddAliasParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.alias_type, "alias");
}

#[test]
fn remove_alias_params_schema() {
    let schema = schema_for!(RemoveAliasParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("id"));
}

#[test]
fn list_apps_params_empty() {
    let json = r#"{}"#;
    let _params: ListAppsParams = serde_json::from_str(json).unwrap();
}

#[test]
fn create_app_params_schema() {
    let schema = schema_for!(CreateAppParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("name"));
    assert!(json.contains("scopes"));
}

#[test]
fn create_app_params_default_description() {
    let json = r#"{"name": "My App", "scopes": "read,write"}"#;
    let params: CreateAppParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.description, "");
}

#[test]
fn delete_app_params_schema() {
    let schema = schema_for!(DeleteAppParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("app_id"));
}

#[test]
fn list_webhooks_params_empty() {
    let json = r#"{}"#;
    let _params: ListWebhooksParams = serde_json::from_str(json).unwrap();
}

#[test]
fn create_webhook_params_schema() {
    let schema = schema_for!(CreateWebhookParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("url"));
    assert!(json.contains("event_type"));
}

#[test]
fn create_webhook_params_defaults() {
    let json = r#"{"url": "https://example.com/hook"}"#;
    let params: CreateWebhookParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.event_type, "new_message");
    assert!(params.filter_sender.is_none());
    assert!(params.filter_thread_id.is_none());
}

#[test]
fn delete_webhook_params_schema() {
    let schema = schema_for!(DeleteWebhookParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("id"));
}

#[test]
fn get_folders_params_empty() {
    let json = r#"{}"#;
    let _params: GetFoldersParams = serde_json::from_str(json).unwrap();
}

#[test]
fn mark_thread_read_params_schema() {
    let schema = schema_for!(MarkThreadReadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn mark_thread_unread_params_schema() {
    let schema = schema_for!(MarkThreadUnreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn star_thread_params_schema() {
    let schema = schema_for!(StarThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn unstar_thread_params_schema() {
    let schema = schema_for!(UnstarThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn archive_thread_params_schema() {
    let schema = schema_for!(ArchiveThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn unarchive_thread_params_schema() {
    let schema = schema_for!(UnarchiveThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn delete_thread_params_schema() {
    let schema = schema_for!(DeleteThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("thread_id"));
}

#[test]
fn get_categories_params_empty() {
    let json = r#"{}"#;
    let _params: GetCategoriesParams = serde_json::from_str(json).unwrap();
}

#[test]
fn get_contacts_params_empty() {
    let json = r#"{}"#;
    let _params: GetContactsParams = serde_json::from_str(json).unwrap();
}

#[test]
fn get_queue_params_empty() {
    let json = r#"{}"#;
    let _params: GetQueueParams = serde_json::from_str(json).unwrap();
}

#[test]
fn retry_queue_message_params_schema() {
    let schema = schema_for!(RetryQueueMessageParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("id"));
}

#[test]
fn send_scheduled_email_params_schema() {
    let schema = schema_for!(SendScheduledEmailParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("to"));
    assert!(json.contains("subject"));
    assert!(json.contains("scheduled_at"));
}

#[test]
fn send_scheduled_email_params_deserialize() {
    let json = r#"{"to": ["a@b.com"], "subject": "test", "body": "hi", "scheduled_at": "2026-03-16T09:00:00Z"}"#;
    let params: SendScheduledEmailParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.to, vec!["a@b.com"]);
    assert!(params.from.is_none());
}

#[test]
fn get_audit_log_params_defaults() {
    let json = r#"{}"#;
    let params: GetAuditLogParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.limit, 50);
}

#[test]
fn get_audit_log_params_custom_limit() {
    let json = r#"{"limit": 10}"#;
    let params: GetAuditLogParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.limit, 10);
}

#[test]
fn list_signatures_params_empty() {
    let json = r#"{}"#;
    let _params: ListSignaturesParams = serde_json::from_str(json).unwrap();
}

#[test]
fn save_signature_params_schema() {
    let schema = schema_for!(SaveSignatureParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("name"));
    assert!(json.contains("html"));
    assert!(json.contains("is_default"));
}

#[test]
fn save_signature_params_defaults() {
    let json = r#"{}"#;
    let params: SaveSignatureParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.name, "Default");
    assert!(params.id.is_none());
    assert!(!params.is_default);
}

#[test]
fn delete_signature_params_schema() {
    let schema = schema_for!(DeleteSignatureParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("id"));
}

#[test]
fn list_encryption_keys_params_empty() {
    let json = r#"{}"#;
    let _params: ListEncryptionKeysParams = serde_json::from_str(json).unwrap();
}

#[test]
fn set_encryption_key_params_schema() {
    let schema = schema_for!(SetEncryptionKeyParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("key_type"));
    assert!(json.contains("public_key"));
    assert!(json.contains("fingerprint"));
}

#[test]
fn set_encryption_key_params_deserialize() {
    let json = r#"{"key_type": "pgp", "public_key": "-----BEGIN PGP PUBLIC KEY BLOCK-----\n...", "fingerprint": "ABCD1234"}"#;
    let params: SetEncryptionKeyParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.key_type, "pgp");
    assert!(!params.public_key.is_empty());
    assert_eq!(params.fingerprint, "ABCD1234");
}

#[test]
fn set_encryption_key_params_default_fingerprint() {
    let json = r#"{"key_type": "smime", "public_key": "cert-data"}"#;
    let params: SetEncryptionKeyParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.fingerprint, "");
}

#[test]
fn delete_encryption_key_params_schema() {
    let schema = schema_for!(DeleteEncryptionKeyParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("key_type"));
}

#[test]
fn get_recipient_key_params_schema() {
    let schema = schema_for!(GetRecipientKeyParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("address"));
    assert!(json.contains("key_type"));
}

#[test]
fn get_recipient_key_params_deserialize() {
    let json = r#"{"address": "alice@example.com", "key_type": "pgp"}"#;
    let params: GetRecipientKeyParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.address, "alice@example.com");
    assert_eq!(params.key_type, "pgp");
}

#[test]
fn audit_list_conversations_params_schema() {
    let schema = schema_for!(AuditListConversationsParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("target_user"));
}

#[test]
fn audit_list_conversations_params_deserialize() {
    let json = r#"{"target_user": "roro@golia.jp"}"#;
    let params: AuditListConversationsParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.target_user, "roro@golia.jp");
    assert!(params.limit.is_none());
}

#[test]
fn audit_read_thread_params_schema() {
    let schema = schema_for!(AuditReadThreadParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("target_user"));
    assert!(json.contains("thread_id"));
}

#[test]
fn audit_read_thread_params_deserialize() {
    let json = r#"{"target_user": "roro@golia.jp", "thread_id": "abc123"}"#;
    let params: AuditReadThreadParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.target_user, "roro@golia.jp");
    assert_eq!(params.thread_id, "abc123");
}

#[test]
fn get_system_config_params_empty() {
    let json = r#"{}"#;
    let _params: GetSystemConfigParams = serde_json::from_str(json).unwrap();
}

#[test]
fn set_system_config_params_schema() {
    let schema = schema_for!(SetSystemConfigParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("key"));
    assert!(json.contains("value"));
}

#[test]
fn set_system_config_params_deserialize() {
    let json = r#"{"key": "webhook_url", "value": "https://example.com/hook"}"#;
    let params: SetSystemConfigParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.key, "webhook_url");
    assert_eq!(params.value, "https://example.com/hook");
}

#[test]
fn reset_system_config_params_schema() {
    let schema = schema_for!(ResetSystemConfigParams);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("key"));
}

#[test]
fn reset_system_config_params_deserialize() {
    let json = r#"{"key": "ai_analysis_enabled"}"#;
    let params: ResetSystemConfigParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.key, "ai_analysis_enabled");
}
