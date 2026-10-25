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

impl EffectivePermissionsResponse {
    /// Whether this account may put `from` in a message it sends: its own
    /// address, any address in [`send_as`](Self::send_as), or anything at
    /// all for a super-admin. Compared ASCII case-insensitively. The web
    /// send and SMTP submission both decide with this, so the two cannot
    /// disagree about who may send as whom.
    pub fn may_send_as(&self, from: &str) -> bool {
        let from = from.trim();
        self.is_super
            || from.eq_ignore_ascii_case(self.address.trim())
            || self
                .send_as
                .iter()
                .any(|s| s.trim().eq_ignore_ascii_case(from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(is_super: bool, send_as: &[&str]) -> EffectivePermissionsResponse {
        EffectivePermissionsResponse {
            address: "alice@x.test".into(),
            permissions: Vec::new(),
            groups: Vec::new(),
            is_super,
            send_as: send_as.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn an_account_sends_as_itself_and_what_it_was_granted() {
        let p = perms(false, &["team@x.test"]);
        assert!(p.may_send_as("alice@x.test"));
        assert!(p.may_send_as("Alice@X.test"));
        assert!(p.may_send_as("TEAM@x.test"));
        assert!(!p.may_send_as("bob@x.test"));
        assert!(!p.may_send_as(""));
    }

    // mail to a tagged address reaches its base mailbox, but sending
    // stays exact: a tag never turns someone else's address into yours
    #[test]
    fn a_tagged_address_is_not_the_account_itself() {
        let p = perms(false, &["team@x.test"]);
        assert!(!p.may_send_as("alice+tag@x.test"));
        assert!(!p.may_send_as("team+tag@x.test"));
        assert!(!p.may_send_as("bob+alice@x.test"));
    }

    #[test]
    fn a_super_admin_sends_as_anyone() {
        assert!(perms(true, &[]).may_send_as("anyone@else.test"));
    }
}
