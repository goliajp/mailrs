use serde::Deserialize;

// --- parameter structs ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct Attachment {
    /// filename (e.g. "photo.png") — auto-derived from URL path or file path if omitted
    #[serde(default)]
    pub filename: Option<String>,
    /// MIME type (e.g. "image/png") — auto-derived from filename extension if omitted
    #[serde(default)]
    pub content_type: Option<String>,
    /// attachment source: base64-encoded content, URL (http:// or https://), or server file path (starts with /)
    pub data: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SendEmailParams {
    /// sender email address (omit to use authenticated account)
    #[serde(default)]
    pub from: Option<String>,
    /// recipient email addresses
    pub to: Vec<String>,
    /// CC recipients
    #[serde(default)]
    pub cc: Option<Vec<String>>,
    /// email subject
    pub subject: String,
    /// plain text email body
    pub body: String,
    /// optional HTML email body
    #[serde(default)]
    pub html_body: Option<String>,
    /// optional file attachments (base64, URL, or server file path)
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReadEmailParams {
    /// message UID from list_conversations or search_emails results
    pub uid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchEmailsParams {
    /// search query string
    pub query: String,
    /// max results (default 20, max 20)
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReplyEmailParams {
    /// thread ID to reply to (from list_conversations)
    pub thread_id: String,
    /// reply text body
    pub body: String,
    /// sender email address (omit to use authenticated account)
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListConversationsParams {
    /// max results (default 20, max 20)
    #[serde(default)]
    pub limit: Option<u32>,
    /// filter by category: personal, notification, promotion, general
    #[serde(default)]
    pub category: Option<String>,
}

// --- admin / user management parameter structs ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateAccountParams {
    /// email address (e.g. "user@golia.jp")
    pub address: String,
    /// domain name (e.g. "golia.jp")
    pub domain: String,
    /// display name
    #[serde(default)]
    pub display_name: String,
    /// password (will be argon2-hashed)
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountParams {
    /// email address to remove
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAccountsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetAccountPasswordParams {
    /// email address
    pub address: String,
    /// new password (will be argon2-hashed)
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListGroupsParams {}

#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAccountGroupsParams {
    /// email address
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAccountToGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountFromGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAccountPermissionsParams {
    /// email address
    pub address: String,
}

// --- domain management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListDomainsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddDomainParams {
    /// domain name (e.g. "example.com")
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveDomainParams {
    /// domain name to remove
    pub name: String,
}

// --- alias management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAliasesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAliasParams {
    /// source address (the alias, e.g. "team@golia.jp")
    pub source_address: String,
    /// target address (receives mail, e.g. "user@golia.jp")
    pub target_address: String,
    /// domain name
    pub domain: String,
    /// "alias" (local delivery) or "forward" (remote forward)
    #[serde(default = "default_alias_type")]
    pub alias_type: String,
}

fn default_alias_type() -> String {
    "alias".into()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAliasParams {
    /// alias ID from list_aliases
    pub id: i64,
}

// --- app management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAppsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateAppParams {
    /// app display name
    pub name: String,
    /// app description
    #[serde(default)]
    pub description: String,
    /// comma-separated permission scopes
    pub scopes: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteAppParams {
    /// app_id (UUID) from list_apps
    pub app_id: String,
}

// --- webhook management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListWebhooksParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateWebhookParams {
    /// callback URL (must be https, or http for localhost)
    pub url: String,
    /// event type (default: "new_message")
    #[serde(default = "default_event_type")]
    pub event_type: String,
    /// optional: only trigger for emails from this sender
    #[serde(default)]
    pub filter_sender: Option<String>,
    /// optional: only trigger for this thread
    #[serde(default)]
    pub filter_thread_id: Option<String>,
}

fn default_event_type() -> String {
    "new_message".into()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteWebhookParams {
    /// webhook ID from list_webhooks
    pub id: i64,
}

// --- mail operations ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetFoldersParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct MarkThreadReadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct MarkThreadUnreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct StarThreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UnstarThreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ArchiveThreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UnarchiveThreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteThreadParams {
    /// thread ID
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetCategoriesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetContactsParams {}

// --- queue management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetQueueParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RetryQueueMessageParams {
    /// queue message ID
    pub id: i64,
}

// --- email group management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListEmailGroupsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateEmailGroupParams {
    /// group email address (e.g. "team@golia.jp")
    pub address: String,
    /// domain name (e.g. "golia.jp")
    pub domain: String,
    /// display name for the group
    #[serde(default)]
    pub name: String,
    /// group description
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteEmailGroupParams {
    /// email group ID from list_email_groups
    pub id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListEmailGroupMembersParams {
    /// email group ID
    pub id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddEmailGroupMemberParams {
    /// email group ID
    pub group_id: i64,
    /// account address to add
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveEmailGroupMemberParams {
    /// email group ID
    pub group_id: i64,
    /// account address to remove
    pub address: String,
}

// --- scheduled send ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SendScheduledEmailParams {
    /// recipient email addresses
    pub to: Vec<String>,
    /// email subject
    pub subject: String,
    /// plain text email body
    pub body: String,
    /// sender email address (omit to use authenticated account)
    #[serde(default)]
    pub from: Option<String>,
    /// ISO 8601 datetime when to send (e.g. "2026-03-16T09:00:00Z")
    pub scheduled_at: String,
}

// --- audit log ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAuditLogParams {
    /// max entries to return (default 50)
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    50
}

// --- signature management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListSignaturesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveSignatureParams {
    /// signature ID to update (omit to create new)
    #[serde(default)]
    pub id: Option<i64>,
    /// signature name (e.g. "Work", "Personal")
    #[serde(default = "default_signature_name")]
    pub name: String,
    /// HTML content of the signature
    #[serde(default)]
    pub html: String,
    /// plain text content of the signature
    #[serde(default)]
    pub text_content: String,
    /// set as default signature
    #[serde(default)]
    pub is_default: bool,
}

fn default_signature_name() -> String {
    "Default".into()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteSignatureParams {
    /// signature ID to delete
    pub id: i64,
}

// --- encryption key management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListEncryptionKeysParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetEncryptionKeyParams {
    /// key type: "pgp" or "smime"
    pub key_type: String,
    /// ASCII-armored PGP public key or PEM-encoded S/MIME certificate
    pub public_key: String,
    /// key fingerprint (hex string)
    #[serde(default)]
    pub fingerprint: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteEncryptionKeyParams {
    /// key type: "pgp" or "smime"
    pub key_type: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetRecipientKeyParams {
    /// email address to look up
    pub address: String,
    /// key type: "pgp" or "smime"
    pub key_type: String,
}

// --- system config ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetSystemConfigParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetSystemConfigParams {
    /// config key (e.g. "webhook_url", "ai_analysis_enabled")
    pub key: String,
    /// new value as string
    pub value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ResetSystemConfigParams {
    /// config key to reset to default
    pub key: String,
}

// --- mail audit ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AuditListConversationsParams {
    /// target user email address to audit
    pub target_user: String,
    /// max results (default 20, max 50)
    #[serde(default)]
    pub limit: Option<u32>,
    /// filter by category
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AuditReadThreadParams {
    /// target user email address to audit
    pub target_user: String,
    /// thread ID to read
    pub thread_id: String,
}

#[cfg(test)]
mod tests {
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
}
