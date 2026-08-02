//! Per-user content the UI stores: drafts, signatures, templates,
//! reactions.

use serde::{Deserialize, Serialize};

pub const PATH_LIST_ACCOUNTS: &str = "/v1/admin/accounts";

pub const PATH_ADD_ACCOUNT: &str = "/v1/admin/accounts";

pub const PATH_GET_ACCOUNT: &str = "/v1/admin/accounts/{address}";

pub const PATH_UPDATE_ACCOUNT: &str = "/v1/admin/accounts/{address}";

pub const PATH_REMOVE_ACCOUNT: &str = "/v1/admin/accounts/{address}";

pub const PATH_GET_ACCOUNT_HASH: &str = "/v1/admin/accounts/{address}/credentials";

pub const PATH_GET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";

pub const PATH_SET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";

pub const PATH_UPDATE_RECOVERY_EMAIL: &str = "/v1/admin/accounts/{address}/recovery-email";

/// `POST /v1/admin/accounts/{address}/password` — re-hash & set the
/// account's password. Used by webapi's change-password / reset-password
/// / forgot-password flows so they update fastcore's embedded kevy
/// instead of the network kevy (which fastcore never reads).
pub const PATH_SET_ACCOUNT_PASSWORD: &str = "/v1/admin/accounts/{address}/password";

/// `POST /v1/users/{user}/messages/{uid}/flags` — patch the flags
/// bitmask on a message that lives in fastcore's embedded kevy. Used
/// by webapi's update_flags / delete_message handlers.
pub const PATH_SET_MESSAGE_FLAGS: &str = "/v1/users/{user}/messages/{uid}/flags";

pub const PATH_LIST_ALIASES: &str = "/v1/admin/aliases";

pub const PATH_ADD_ALIAS: &str = "/v1/admin/aliases";

pub const PATH_REMOVE_ALIAS: &str = "/v1/admin/aliases/{id}";

pub const PATH_RESOLVE_RECIPIENT: &str = "/v1/admin/aliases:resolve-recipient";

pub const PATH_LIST_APPS: &str = "/v1/admin/apps";

pub const PATH_CREATE_APP: &str = "/v1/admin/apps";

pub const PATH_GET_APP: &str = "/v1/admin/apps/{app_id}";

pub const PATH_GET_APP_BY_ID: &str = "/v1/admin/apps/by-id/{id}";

pub const PATH_REMOVE_APP: &str = "/v1/admin/apps/{app_id}";

pub const PATH_UPDATE_APP_SCOPES: &str = "/v1/admin/apps/{app_id}/scopes";

pub const PATH_LOG_AUDIT: &str = "/v1/admin/audit-log";

pub const PATH_LIST_AUDIT_LOG: &str = "/v1/admin/audit-log";

pub const PATH_CLEANUP_AUDIT: &str = "/v1/admin/audit-log:cleanup";

pub const PATH_LIST_DOMAINS: &str = "/v1/admin/domains";

pub const PATH_ADD_DOMAIN: &str = "/v1/admin/domains";

pub const PATH_REMOVE_DOMAIN: &str = "/v1/admin/domains/{name}";

pub const PATH_LIST_EMAIL_GROUPS: &str = "/v1/admin/email-groups";

pub const PATH_CREATE_EMAIL_GROUP: &str = "/v1/admin/email-groups";

pub const PATH_REMOVE_EMAIL_GROUP: &str = "/v1/admin/email-groups/{id}";

pub const PATH_LIST_EMAIL_GROUP_MEMBERS: &str = "/v1/admin/email-groups/{id}/members";

pub const PATH_ADD_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members";

pub const PATH_REMOVE_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members/{address}";

pub const PATH_GET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";

pub const PATH_SET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";

pub const PATH_DELETE_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";

pub const PATH_LIST_ENCRYPTION_KEYS: &str = "/v1/admin/encryption-keys/{address}";

pub const PATH_LIST_GROUPS: &str = "/v1/admin/groups";

pub const PATH_GET_GROUP_PERMISSIONS: &str = "/v1/admin/groups/{id}/permissions";

pub const PATH_SET_GROUP_PERMISSIONS: &str = "/v1/admin/groups/{id}/permissions";

pub const PATH_ADD_GROUP: &str = "/v1/admin/groups";

pub const PATH_REMOVE_GROUP: &str = "/v1/admin/groups/{id}";

pub const PATH_LIST_GROUP_MEMBERS: &str = "/v1/admin/groups/{id}/members";

pub const PATH_ADD_ACCOUNT_TO_GROUP: &str = "/v1/admin/groups/{id}/members";

pub const PATH_REMOVE_ACCOUNT_FROM_GROUP: &str = "/v1/admin/groups/{id}/members/{address}";

pub const PATH_GET_ACCOUNT_GROUPS: &str = "/v1/admin/accounts/{address}/groups";

pub const PATH_GET_ACCOUNT_OVERRIDES: &str = "/v1/admin/accounts/{address}/overrides";

pub const PATH_SET_ACCOUNT_OVERRIDES: &str = "/v1/admin/accounts/{address}/overrides";

pub const PATH_EFFECTIVE_PERMISSIONS: &str = "/v1/admin/accounts/{address}/effective-permissions";

pub const PATH_INVALIDATE_PERMISSIONS: &str = "/v1/admin/permissions:invalidate";

pub const PATH_GET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";

pub const PATH_SET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";

pub const PATH_DELETE_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";

pub const PATH_GET_TOTP: &str = "/v1/admin/accounts/{address}/totp";

pub const PATH_SAVE_TOTP: &str = "/v1/admin/accounts/{address}/totp";

pub const PATH_ENABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp:enable";

pub const PATH_DISABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp";

pub const PATH_CONSUME_RECOVERY_CODE: &str =
    "/v1/admin/accounts/{address}/totp:consume-recovery-code";

pub const PATH_SHOULD_SEND_VACATION: &str = "/v1/admin/vacation-dedup:should-send";

pub const PATH_RECORD_VACATION_REPLY: &str = "/v1/admin/vacation-dedup";

pub const PATH_LIST_SYSTEM_CONFIG: &str = "/v1/admin/system-config";

pub const PATH_UPDATE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";

pub const PATH_DELETE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";

pub const PATH_RECONCILE_MAILDIR: &str = "/v1/admin/reconcile";

pub const PATH_BACKFILL_THREADING: &str = "/v1/admin/backfill-threading";

pub const PATH_EXPORT_MESSAGES: &str = "/v1/admin/export";

pub const PATH_CREATE_API_KEY: &str = "/v1/admin/api-keys";

pub const PATH_LIST_API_KEYS: &str = "/v1/admin/accounts/{address}/api-keys";

pub const PATH_REVOKE_API_KEY: &str = "/v1/admin/api-keys/{id}";

pub const PATH_GET_API_KEY_BY_PREFIX: &str = "/v1/admin/api-keys/by-prefix/{prefix}";

pub const PATH_TOUCH_API_KEY: &str = "/v1/admin/api-keys/{id}/touch";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftWire {
    pub id: i64,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_thread_id: Option<String>,
    /// Epoch seconds.
    pub created_at: i64,
    /// Epoch seconds.
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
/// The compose autosave's body, and the Draft tab's save.
///
/// `deny_unknown_fields`: this runs every three seconds while someone is
/// typing, and a field the struct does not name was silently dropped —
/// which is how a reply reopened from the Draft tab lost the conversation
/// it belonged to. A 400 naming the field is the difference between a bug
/// found in a minute and one found in a week. The shape is pinned by
/// `wire-contract/requests/draft-save.json`, checked on both sides.
#[serde(deny_unknown_fields)]
pub struct SaveDraftRequest {
    /// When present, upsert that draft in place (keeps its id) instead of
    /// allocating a new one — so a compose session's periodic autosave
    /// updates one draft rather than spawning a new one each tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveDraftResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftListResponse {
    pub items: Vec<DraftWire>,
}

pub const PATH_LIST_DRAFTS: &str = "/v1/users/{user}/drafts";

pub const PATH_SAVE_DRAFT: &str = "/v1/users/{user}/drafts";

pub const PATH_DELETE_DRAFT: &str = "/v1/users/{user}/drafts/{id}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureWire {
    pub id: i64,
    pub name: String,
    pub html: String,
    pub text_content: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SaveSignatureRequest {
    pub name: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text_content: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveSignatureResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureListResponse {
    pub items: Vec<SignatureWire>,
}

pub const PATH_LIST_SIGNATURES: &str = "/v1/users/{user}/signatures";

pub const PATH_SAVE_SIGNATURE: &str = "/v1/users/{user}/signatures";

pub const PATH_DELETE_SIGNATURE: &str = "/v1/users/{user}/signatures/{id}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionAggregateRow {
    pub message_uid: i64,
    pub emoji: String,
    pub count: i64,
    pub me: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionsResponse {
    pub reactions: Vec<ReactionAggregateRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleReactionRequest {
    pub emoji: String,
}

pub const PATH_GET_THREAD_REACTIONS: &str = "/v1/users/{user}/threads/{thread_id}/reactions";

pub const PATH_TOGGLE_REACTION: &str =
    "/v1/users/{user}/threads/{thread_id}/messages/{uid}/reactions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateWire {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
    pub category: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SaveTemplateRequest {
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub html_body: String,
    #[serde(default)]
    pub text_body: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveTemplateResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateListResponse {
    pub items: Vec<TemplateWire>,
}
