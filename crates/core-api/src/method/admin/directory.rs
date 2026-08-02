//! Accounts, aliases, domains, email groups — the directory.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

pub const PATH_LIST_TEMPLATES: &str = "/v1/users/{user}/templates";

pub const PATH_SAVE_TEMPLATE: &str = "/v1/users/{user}/templates";

pub const PATH_DELETE_TEMPLATE: &str = "/v1/users/{user}/templates/{id}";

pub const PATH_CREATE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions";

pub const PATH_LIST_WEBHOOKS: &str = "/v1/admin/accounts/{address}/webhook-subscriptions";

pub const PATH_DELETE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions/{id}";

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

/// Request body for `POST /v1/admin/accounts/{address}/password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPasswordRequest {
    /// Argon2-hashed password. Webapi hashes the plaintext locally so
    /// fastcore only ever sees the hash on the wire.
    pub password_hash: String,
}

/// Request body for `POST /v1/users/{user}/messages/{uid}/flags`.
///
/// Fastcore reads the current wire from the per-user uid index,
/// rewrites `flags` verbatim, and (if `\Seen` toggled) mirrors the
/// change to the thread's `has_unread` zset via `mark_seen` /
/// `mark_unread`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SetMessageFlagsRequest {
    pub flags: u32,
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
