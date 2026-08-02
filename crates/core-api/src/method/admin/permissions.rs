//! Groups, permissions, per-account overrides, TOTP, sieve, vacation,
//! encryption keys.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

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
