//! Admin endpoints — domain_store CRUD + reconcile / export / audit / system_config + api_keys + webhooks + oauth.
//!
//! Source: `crates/server/src/domain_store/*.rs` (57 fn across 12 files) +
//! `api_key_store.rs` + `webhook/store.rs` + `oidc_store.rs`. See
//! `docs/CURRENT_STATE_FROZEN.md` §0.5.

use serde::{Deserialize, Serialize};

use crate::types::UserAddress;

// ════════════════════════════════════════════════════════════════════
// Path constants
// ════════════════════════════════════════════════════════════════════

// ── accounts ─────────────────────────────────────────────────────────
pub const PATH_LIST_ACCOUNTS: &str = "/v1/admin/accounts";
pub const PATH_ADD_ACCOUNT: &str = "/v1/admin/accounts";
pub const PATH_GET_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_UPDATE_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_REMOVE_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_GET_ACCOUNT_HASH: &str = "/v1/admin/accounts/{address}/credentials";
pub const PATH_GET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";
pub const PATH_SET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";
pub const PATH_UPDATE_RECOVERY_EMAIL: &str = "/v1/admin/accounts/{address}/recovery-email";

// ── aliases ──────────────────────────────────────────────────────────
pub const PATH_LIST_ALIASES: &str = "/v1/admin/aliases";
pub const PATH_ADD_ALIAS: &str = "/v1/admin/aliases";
pub const PATH_REMOVE_ALIAS: &str = "/v1/admin/aliases/{id}";
pub const PATH_RESOLVE_RECIPIENT: &str = "/v1/admin/aliases:resolve-recipient";

// ── apps ─────────────────────────────────────────────────────────────
pub const PATH_LIST_APPS: &str = "/v1/admin/apps";
pub const PATH_CREATE_APP: &str = "/v1/admin/apps";
pub const PATH_GET_APP: &str = "/v1/admin/apps/{app_id}";
pub const PATH_GET_APP_BY_ID: &str = "/v1/admin/apps/by-id/{id}";
pub const PATH_REMOVE_APP: &str = "/v1/admin/apps/{app_id}";
pub const PATH_UPDATE_APP_SCOPES: &str = "/v1/admin/apps/{app_id}/scopes";

// ── audit log ────────────────────────────────────────────────────────
pub const PATH_LOG_AUDIT: &str = "/v1/admin/audit-log";
pub const PATH_LIST_AUDIT_LOG: &str = "/v1/admin/audit-log";
pub const PATH_CLEANUP_AUDIT: &str = "/v1/admin/audit-log:cleanup";

// ── domains ──────────────────────────────────────────────────────────
pub const PATH_LIST_DOMAINS: &str = "/v1/admin/domains";
pub const PATH_ADD_DOMAIN: &str = "/v1/admin/domains";
pub const PATH_REMOVE_DOMAIN: &str = "/v1/admin/domains/{name}";

// ── email groups ─────────────────────────────────────────────────────
pub const PATH_LIST_EMAIL_GROUPS: &str = "/v1/admin/email-groups";
pub const PATH_CREATE_EMAIL_GROUP: &str = "/v1/admin/email-groups";
pub const PATH_REMOVE_EMAIL_GROUP: &str = "/v1/admin/email-groups/{id}";
pub const PATH_LIST_EMAIL_GROUP_MEMBERS: &str = "/v1/admin/email-groups/{id}/members";
pub const PATH_ADD_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members";
pub const PATH_REMOVE_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members/{address}";

// ── encryption keys ──────────────────────────────────────────────────
pub const PATH_GET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_SET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_DELETE_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_LIST_ENCRYPTION_KEYS: &str = "/v1/admin/encryption-keys/{address}";

// ── groups + permissions ─────────────────────────────────────────────
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

// ── sieve ────────────────────────────────────────────────────────────
pub const PATH_GET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";
pub const PATH_SET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";
pub const PATH_DELETE_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";

// ── totp ─────────────────────────────────────────────────────────────
pub const PATH_GET_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_SAVE_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_ENABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp:enable";
pub const PATH_DISABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_CONSUME_RECOVERY_CODE: &str =
    "/v1/admin/accounts/{address}/totp:consume-recovery-code";

// ── vacation dedup ───────────────────────────────────────────────────
pub const PATH_SHOULD_SEND_VACATION: &str = "/v1/admin/vacation-dedup:should-send";
pub const PATH_RECORD_VACATION_REPLY: &str = "/v1/admin/vacation-dedup";

// ── system config ────────────────────────────────────────────────────
pub const PATH_LIST_SYSTEM_CONFIG: &str = "/v1/admin/system-config";
pub const PATH_UPDATE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";
pub const PATH_DELETE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";

// ── reconcile + backfill + export ────────────────────────────────────
pub const PATH_RECONCILE_MAILDIR: &str = "/v1/admin/reconcile";
pub const PATH_BACKFILL_THREADING: &str = "/v1/admin/backfill-threading";
pub const PATH_EXPORT_MESSAGES: &str = "/v1/admin/export";

// ── api keys (consumed by webapi auth — hot path) ────────────────────
pub const PATH_CREATE_API_KEY: &str = "/v1/admin/api-keys";
pub const PATH_LIST_API_KEYS: &str = "/v1/admin/accounts/{address}/api-keys";
pub const PATH_REVOKE_API_KEY: &str = "/v1/admin/api-keys/{id}";
pub const PATH_GET_API_KEY_BY_PREFIX: &str = "/v1/admin/api-keys/by-prefix/{prefix}";
pub const PATH_TOUCH_API_KEY: &str = "/v1/admin/api-keys/{id}/touch";

// ── webhook subscriptions ────────────────────────────────────────────
pub const PATH_CREATE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions";
pub const PATH_LIST_WEBHOOKS: &str = "/v1/admin/accounts/{address}/webhook-subscriptions";
pub const PATH_DELETE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions/{id}";

// ── oauth / oidc provider ────────────────────────────────────────────
pub const PATH_LIST_OAUTH_CLIENTS: &str = "/v1/admin/oauth-clients";
pub const PATH_CREATE_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients";
pub const PATH_GET_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients/{client_id}";
pub const PATH_DELETE_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients/{client_id}";
pub const PATH_LIST_SIGNING_KEYS: &str = "/v1/admin/oauth-signing-keys";
pub const PATH_OAUTH_AUTH_CODE: &str = "/v1/admin/oauth-auth-codes";
pub const PATH_OAUTH_REFRESH_TOKEN: &str = "/v1/admin/oauth-refresh-tokens";

// ════════════════════════════════════════════════════════════════════
// Wire types — accounts
// ════════════════════════════════════════════════════════════════════

/// One row in the accounts table — public shape (no password hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountWire {
    pub address: UserAddress,
    pub domain: String,
    pub display_name: String,
    pub active: bool,
    /// Epoch seconds.
    pub created_at: i64,
    pub quota_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_email: Option<String>,
}

/// Internal lookup response — includes Argon2 hash. Only the SMTP/IMAP/POP3/
/// MgSieve AUTH path reads this; never exposed to webapi public API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountWithHashWire {
    #[serde(flatten)]
    pub public: AccountWire,
    /// Argon2 password hash (sensitive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAccountRequest {
    pub address: UserAddress,
    pub display_name: String,
    /// Plaintext password — server hashes with Argon2 before insert.
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccountRequest {
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GetQuotaResponse {
    pub quota_bytes: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SetQuotaRequest {
    pub quota_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecoveryEmailRequest {
    pub recovery_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountListResponse {
    pub items: Vec<AccountWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — aliases
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasWire {
    pub id: i64,
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    /// One of: `alias` / `forward` / etc.
    pub alias_type: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAliasRequest {
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    pub alias_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AddAliasResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasListResponse {
    pub items: Vec<AliasWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRecipientRequest {
    /// Incoming SMTP RCPT address.
    pub recipient: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRecipientResponse {
    /// Final delivery target addresses (alias / group fan-out resolved).
    pub targets: Vec<String>,
    /// True if at least one target is a local account.
    pub local: bool,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — apps
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppWire {
    pub id: i64,
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub owner_address: UserAddress,
    pub scopes: Vec<String>,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppRequest {
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub owner_address: UserAddress,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAppScopesRequest {
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppListResponse {
    pub items: Vec<AppWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — audit log
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRowWire {
    pub id: i64,
    pub timestamp: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAuditRequest {
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ListAuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditListResponse {
    pub items: Vec<AuditRowWire>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CleanupAuditRequest {
    /// Delete rows older than this many days.
    pub older_than_days: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CleanupAuditResponse {
    pub deleted: u32,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — domains
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainWire {
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDomainRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainListResponse {
    pub items: Vec<DomainWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — email groups
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailGroupWire {
    pub id: i64,
    pub address: String,
    pub domain: String,
    pub name: String,
    pub description: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEmailGroupRequest {
    pub address: String,
    pub domain: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailGroupMemberRequest {
    pub member_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailGroupListResponse {
    pub items: Vec<EmailGroupWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailGroupMembersResponse {
    /// Member email addresses.
    pub members: Vec<String>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — encryption keys
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKeyWire {
    pub id: i64,
    /// `pgp` or `smime`.
    pub key_type: String,
    /// Public key armor (or raw bytes base64).
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetEncryptionKeyRequest {
    pub public_key: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKeyListResponse {
    pub items: Vec<EncryptionKeyWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — groups + permissions
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupWire {
    pub id: i64,
    pub name: String,
    /// `None` = cross-domain builtin group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub description: String,
    pub is_builtin: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupListResponse {
    pub items: Vec<GroupWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddGroupRequest {
    pub name: String,
    pub domain: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPermissionsResponse {
    /// Permission strings (e.g. `admin.accounts`, `internal.rpc`).
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetGroupPermissionsRequest {
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembersResponse {
    pub members: Vec<UserAddress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddGroupMemberRequest {
    pub account_address: UserAddress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountOverridesRow {
    pub permission: String,
    /// `true` = explicitly granted, `false` = explicitly denied.
    pub granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountOverridesResponse {
    pub items: Vec<AccountOverridesRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetAccountOverridesRequest {
    pub items: Vec<AccountOverridesRow>,
}

/// Effective permissions snapshot — what `auth_me` returns to the
/// frontend, also what every authed request boundary checks against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePermissionsResponse {
    pub address: UserAddress,
    pub permissions: Vec<String>,
    pub groups: Vec<GroupWire>,
    /// True if user is super-admin (member of any builtin "admin" group).
    pub is_super: bool,
    /// Addresses the user may "send as" (via alias + email_group fanout).
    pub send_as: Vec<String>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — sieve
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SieveScriptResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSieveRequest {
    pub script: String,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — totp
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpStatusResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    pub enabled: bool,
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTotpRequest {
    pub secret: String,
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumeRecoveryCodeRequest {
    pub code: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConsumeRecoveryCodeResponse {
    pub accepted: bool,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — vacation dedup
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShouldSendVacationRequest {
    pub recipient: String,
    pub sender: String,
    pub handle: String,
    /// Suppression window in seconds (e.g. 86400 for 1 day).
    pub window_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ShouldSendVacationResponse {
    pub should_send: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordVacationReplyRequest {
    pub recipient: String,
    pub sender: String,
    pub handle: String,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — system config
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigRow {
    pub key: String,
    pub value: String,
    /// `string` / `int` / `bool` / `float` / `json`.
    pub value_type: String,
    pub updated_at: i64,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigListResponse {
    pub items: Vec<SystemConfigRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSystemConfigRequest {
    pub value: String,
    pub value_type: String,
    pub updated_by: String,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — reconcile + backfill + export
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ReconcileRequest {
    /// `true` = report the gap without writing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub scanned: u64,
    pub missing: u64,
    pub repaired: u64,
    /// Per-mailbox error messages.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileResponse {
    pub dry_run: bool,
    pub report: ReconcileReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExportRequest {
    /// Required: user to export.
    pub user: UserAddress,
    /// Optional epoch-seconds lower bound (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
    /// Optional epoch-seconds upper bound (exclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Optional ILIKE-style filter over subject + text_body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Max rows.
    #[serde(default = "default_export_limit")]
    pub limit: u32,
}

fn default_export_limit() -> u32 {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedMessageRow {
    pub message_id: String,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub internal_date: i64,
    pub size: u32,
    pub text_body: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResponse {
    pub items: Vec<ExportedMessageRow>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — API keys
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyWire {
    pub id: i64,
    pub prefix: String,
    /// Full key (only returned at create-time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_key: Option<String>,
    /// Argon2 hash of the full key (server-side only).
    #[serde(skip)]
    pub key_hash: String,
    pub account_address: UserAddress,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<i64>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub account_address: UserAddress,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Optional app binding (e.g. when issued from /api/agent/keys flow).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub id: i64,
    pub prefix: String,
    /// Full key shown once.
    pub full_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyListResponse {
    pub items: Vec<ApiKeyWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — webhook subscriptions
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubWire {
    pub id: i64,
    pub account_address: UserAddress,
    pub url: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_thread_id: Option<String>,
    pub signing_secret: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWebhookRequest {
    pub account_address: UserAddress,
    pub url: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWebhookResponse {
    pub id: i64,
    pub signing_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookListResponse {
    pub items: Vec<WebhookSubWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — oauth / oidc provider
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientWire {
    pub client_id: String,
    /// Hash of client secret (Argon2). Never round-trip to UI.
    #[serde(skip)]
    pub secret_hash: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub trusted: bool,
    pub active: bool,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOAuthClientRequest {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub trusted: bool,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOAuthClientResponse {
    pub client_id: String,
    /// Plaintext secret shown once.
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientListResponse {
    pub items: Vec<OAuthClientWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyWire {
    pub kid: String,
    pub public_key_pem: String,
    /// Private key — only for core internal use.
    #[serde(skip)]
    pub private_key_pem: String,
    pub algorithm: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyListResponse {
    pub items: Vec<SigningKeyWire>,
}

// ════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_serde_with_hash_flatten() {
        let a = AccountWithHashWire {
            public: AccountWire {
                address: "u@x.com".into(),
                domain: "x.com".into(),
                display_name: "User".into(),
                active: true,
                created_at: 1_700_000_000,
                quota_bytes: 0,
                recovery_email: None,
            },
            password_hash: Some("$argon2id$...".into()),
        };
        let s = serde_json::to_string(&a).unwrap();
        // flatten should put `address` and `password_hash` at the top level
        assert!(s.contains("\"address\":\"u@x.com\""));
        assert!(s.contains("\"password_hash\""));
        let back: AccountWithHashWire = serde_json::from_str(&s).unwrap();
        assert_eq!(back.public.address, "u@x.com");
        assert!(back.password_hash.is_some());
    }

    #[test]
    fn audit_default_limit_is_100() {
        let q: ListAuditQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn effective_permissions_roundtrip() {
        let p = EffectivePermissionsResponse {
            address: "admin@x.com".into(),
            permissions: vec!["admin.accounts".into(), "internal.rpc".into()],
            groups: vec![],
            is_super: true,
            send_as: vec!["billing@x.com".into()],
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: EffectivePermissionsResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back.permissions.len(), 2);
        assert!(back.is_super);
        assert_eq!(back.send_as.len(), 1);
    }

    #[test]
    fn api_key_omits_full_key_when_none() {
        let k = ApiKeyWire {
            id: 1,
            prefix: "mk_aBcD".into(),
            full_key: None,
            key_hash: "$argon2id$...".into(),
            account_address: "u@x.com".into(),
            name: "ci".into(),
            expires_at: None,
            last_used_at: None,
            revoked_at: None,
            created_at: 0,
            app_id: None,
        };
        let s = serde_json::to_string(&k).unwrap();
        assert!(!s.contains("full_key"));
        // key_hash is `#[serde(skip)]` so it must NOT leak
        assert!(!s.contains("key_hash"));
    }

    #[test]
    fn export_request_omits_optional() {
        let r = ExportRequest {
            user: "u@x.com".into(),
            ..Default::default()
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"since\""));
        assert!(!s.contains("\"until\""));
        // Default::default() for u32 is 0 — but on the wire, when deserializing
        // a request that omits "limit", `#[serde(default = "default_export_limit")]`
        // gives us 1000. Verify the deser side:
        let r2: ExportRequest = serde_json::from_str(r#"{"user":"u@x.com"}"#).unwrap();
        assert_eq!(r2.limit, 1000);
    }
}
