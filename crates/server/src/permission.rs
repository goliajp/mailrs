use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// all known permissions in the system
pub const ALL_PERMISSIONS: &[&str] = &[
    "mail.send",
    "mail.read",
    "mail.read_domain",
    "admin.domains",
    "admin.accounts",
    "admin.aliases",
    "admin.groups",
    "admin.queue",
    "admin.sieve",
    "admin.impersonate",
    "internal.rpc",
    "admin.oauth_clients",
    "admin.system_config",
];

/// group info loaded from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    pub id: i64,
    pub name: String,
    pub domain: Option<String>,
    pub description: String,
    pub is_builtin: bool,
    pub created_at: i64,
}

/// a group's membership entry for an account
#[derive(Debug, Clone)]
pub struct AccountGroup {
    pub group: GroupInfo,
    pub permissions: Vec<String>,
}

/// computed effective permissions for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePermissions {
    permissions: HashSet<String>,
    is_super: bool,
    accessible_domains: Vec<String>,
    /// alias addresses this account can send as (reverse alias lookup)
    #[serde(default)]
    send_as: Vec<String>,
}

impl EffectivePermissions {
    /// check if the user has a specific permission
    pub fn has(&self, perm: &str) -> bool {
        self.is_super || self.permissions.contains(perm)
    }

    /// check if the user has any admin permission
    #[cfg(test)]
    pub fn has_any_admin(&self) -> bool {
        self.is_super
            || self
                .permissions
                .iter()
                .any(|p| p.starts_with("admin."))
    }

    /// whether this user is a super user (global group with all perms)
    pub fn is_super(&self) -> bool {
        self.is_super
    }

    /// domains the user can access (empty = own domain only)
    pub fn accessible_domains(&self) -> &[String] {
        &self.accessible_domains
    }

    /// alias addresses this account can send as
    pub fn send_as(&self) -> &[String] {
        &self.send_as
    }

    /// set send_as addresses (called after construction)
    pub fn with_send_as(mut self, send_as: Vec<String>) -> Self {
        self.send_as = send_as;
        self
    }

    /// list of permission strings for serialization
    pub fn permission_list(&self) -> Vec<String> {
        if self.is_super {
            ALL_PERMISSIONS.iter().map(|s| (*s).to_string()).collect()
        } else {
            let mut perms: Vec<String> = self.permissions.iter().cloned().collect();
            perms.sort();
            perms
        }
    }
}

/// build EffectivePermissions from a flat list of scopes (for apps)
pub fn from_scopes(scopes: &[String], all_domains: &[String]) -> EffectivePermissions {
    let is_internal_rpc = scopes.iter().any(|s| s == "internal.rpc");
    if is_internal_rpc {
        return EffectivePermissions {
            permissions: ALL_PERMISSIONS.iter().map(|s| (*s).to_string()).collect(),
            is_super: true,
            accessible_domains: all_domains.to_vec(),
            send_as: Vec::new(),
        };
    }

    let permissions: HashSet<String> = scopes
        .iter()
        .filter(|s| ALL_PERMISSIONS.contains(&s.as_str()))
        .cloned()
        .collect();

    // app scopes grant access to all domains (they act cross-domain)
    EffectivePermissions {
        permissions,
        is_super: false,
        accessible_domains: all_domains.to_vec(),
        send_as: Vec::new(),
    }
}

/// compute effective permissions from groups and overrides
pub fn compute_effective_permissions(
    groups: &[AccountGroup],
    overrides: &[(String, bool)],
    all_domains: &[String],
) -> EffectivePermissions {
    // check if any global group has all permissions (= super)
    let is_super = groups.iter().any(|ag| {
        ag.group.domain.is_none()
            && ALL_PERMISSIONS
                .iter()
                .all(|p| ag.permissions.iter().any(|gp| gp == p))
    });

    if is_super {
        // super user: check if any override revokes a permission
        let revoked: HashSet<&str> = overrides
            .iter()
            .filter(|(_, granted)| !granted)
            .map(|(p, _)| p.as_str())
            .collect();

        if revoked.is_empty() {
            return EffectivePermissions {
                permissions: ALL_PERMISSIONS
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                is_super: true,
                accessible_domains: all_domains.to_vec(),
                send_as: Vec::new(),
            };
        }

        // super with revocations — downgrade
        let permissions: HashSet<String> = ALL_PERMISSIONS
            .iter()
            .filter(|p| !revoked.contains(*p))
            .map(|s| (*s).to_string())
            .collect();

        return EffectivePermissions {
            permissions,
            is_super: false,
            accessible_domains: all_domains.to_vec(),
            send_as: Vec::new(),
        };
    }

    // non-super: union all group permissions
    let mut permissions: HashSet<String> = HashSet::new();
    let mut domains: HashSet<String> = HashSet::new();

    for ag in groups {
        for perm in &ag.permissions {
            permissions.insert(perm.clone());
        }
        if let Some(ref domain) = ag.group.domain {
            domains.insert(domain.clone());
        }
        // global (non-super) group: grants perms on all domains
        if ag.group.domain.is_none() {
            for d in all_domains {
                domains.insert(d.clone());
            }
        }
    }

    // apply overrides
    for (perm, granted) in overrides {
        if *granted {
            permissions.insert(perm.clone());
        } else {
            permissions.remove(perm);
        }
    }

    let mut accessible_domains: Vec<String> = domains.into_iter().collect();
    accessible_domains.sort();

    EffectivePermissions {
        permissions,
        is_super: false,
        accessible_domains,
        send_as: Vec::new(),
    }
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;
