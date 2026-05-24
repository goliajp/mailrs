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
#[path = "tools_tests.rs"]
mod tests;
