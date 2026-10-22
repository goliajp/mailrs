//! Groups and the effective permissions they grant.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

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
