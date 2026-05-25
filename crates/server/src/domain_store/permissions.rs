//! Effective-permission computation + cache invalidation.

use super::{CACHE_TTL_SECS, DomainStore, Result};

impl DomainStore {
    /// load effective permissions for an account
    pub async fn load_account_permissions(
        &self,
        address: &str,
    ) -> Result<crate::permission::EffectivePermissions> {
        use crate::permission::{AccountGroup, GroupInfo, compute_effective_permissions};

        // try valkey cache
        let cache_key = format!("perms:{address}");
        if let Some(cached) = self
            .valkey_get::<crate::permission::EffectivePermissions>(&cache_key)
            .await
        {
            return Ok(cached);
        }

        let pool = self.pg()?;

        // load groups with their permissions
        let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, bool, i64, String)>(
            "SELECT g.id, g.name, g.domain, g.description, g.is_builtin, \
             EXTRACT(EPOCH FROM g.created_at)::bigint, gp.permission \
             FROM account_groups ag \
             JOIN groups g ON g.id = ag.group_id \
             JOIN group_permissions gp ON gp.group_id = g.id \
             WHERE ag.account_address = $1",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;

        // group rows by group id
        let mut groups_map: std::collections::HashMap<i64, AccountGroup> =
            std::collections::HashMap::new();
        for (id, name, domain, description, is_builtin, created_at, permission) in rows {
            let entry = groups_map.entry(id).or_insert_with(|| AccountGroup {
                group: GroupInfo {
                    id,
                    name,
                    domain,
                    description,
                    is_builtin,
                    created_at,
                },
                permissions: Vec::new(),
            });
            entry.permissions.push(permission);
        }
        let groups: Vec<AccountGroup> = groups_map.into_values().collect();

        // load overrides
        let override_rows = sqlx::query_as::<_, (String, bool)>(
            "SELECT permission, granted FROM account_permission_overrides \
             WHERE account_address = $1",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;

        // get all domains for super user domain list
        let all_domains: Vec<String> =
            sqlx::query_as::<_, (String,)>("SELECT name FROM domains ORDER BY name")
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(|(n,)| n)
                .collect();

        // reverse alias lookup: addresses that alias TO this account
        let mut send_as: Vec<String> = sqlx::query_as::<_, (String,)>(
            "SELECT source_address FROM aliases \
             WHERE target_address = $1 AND alias_type = 'alias' AND active = true \
             ORDER BY source_address",
        )
        .bind(address)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(a,)| a)
        .collect();

        // email group memberships: member can send as group address
        let group_addrs: Vec<(String,)> = sqlx::query_as(
            "SELECT g.address FROM email_groups g \
             JOIN email_group_members m ON m.group_id = g.id \
             WHERE m.member_address = $1 \
             ORDER BY g.address",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;
        for (ga,) in group_addrs {
            if !send_as.contains(&ga) {
                send_as.push(ga);
            }
        }

        let perms = compute_effective_permissions(&groups, &override_rows, &all_domains)
            .with_send_as(send_as);

        // cache
        self.valkey_set(&cache_key, &perms, CACHE_TTL_SECS).await;

        Ok(perms)
    }

    /// invalidate permission cache for an account
    pub async fn invalidate_permissions(&self, address: &str) {
        self.valkey_del(&format!("perms:{address}")).await;
    }

    /// invalidate permission cache for all members of a group
    pub async fn invalidate_group_permissions(&self, group_id: i64) {
        let Ok(pool) = self.pg() else { return };
        let members: Vec<(String,)> =
            sqlx::query_as("SELECT account_address FROM account_groups WHERE group_id = $1")
                .bind(group_id)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        for (addr,) in members {
            self.valkey_del(&format!("perms:{addr}")).await;
        }
    }
}
