//! Effective-permission computation + cache invalidation.

use super::{CACHE_TTL_SECS, DomainStore, Result};

impl DomainStore {
    /// Load effective permissions for an account, cached for
    /// [`CACHE_TTL_SECS`]. `send_as` is left empty: aliases and email groups
    /// change without passing through here, so [`Self::send_as`] reads them
    /// on every call instead.
    pub async fn load_account_permissions(
        &self,
        address: &str,
    ) -> Result<crate::permission::EffectivePermissions> {
        use crate::permission::{AccountGroup, GroupInfo, compute_effective_permissions};

        // try kevy cache
        let cache_key = format!("perms:{address}");
        if let Some(cached) = self.kevy_get::<crate::permission::EffectivePermissions>(&cache_key) {
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

        let perms = compute_effective_permissions(&groups, &override_rows, &all_domains);

        // cache
        self.kevy_set(&cache_key, &perms, CACHE_TTL_SECS);

        Ok(perms)
    }
}

impl DomainStore {
    /// The addresses `address` may send as besides its own: its grants, the
    /// active aliases pointing at it and the email groups (in the network
    /// kevy at `net_url`) it belongs to.
    pub async fn send_as(&self, address: &str, net_url: Option<&str>) -> Result<Vec<String>> {
        use mailrs_core_sidestate::families::{email_groups, send_as};
        let pool = self.pg()?;
        let grants = self.send_as_grants(address).await?;
        let aliases: Vec<(String, String)> = sqlx::query_as(
            "SELECT source_address, target_address FROM aliases \
             WHERE target_address = $1 AND alias_type = 'alias' AND active = true",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;
        let aliases = send_as::alias_sources_for(
            aliases.iter().map(|(s, t)| (s.as_str(), t.as_str())),
            address,
        );
        let groups = match net_url.map(str::to_string) {
            Some(url) => {
                let addr = address.to_string();
                tokio::task::spawn_blocking(move || {
                    let mut conn =
                        kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
                    email_groups::addresses_with_member(&mut conn, &addr)
                })
                .await
                .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?
                .map_err(sqlx::Error::Io)?
            }
            None => Vec::new(),
        };
        Ok(send_as::union(&grants, aliases, groups))
    }

    /// The addresses an administrator granted `address` to send as.
    pub async fn send_as_grants(&self, address: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT address FROM account_send_as WHERE account_address = $1 ORDER BY address",
        )
        .bind(address)
        .fetch_all(self.pg()?)
        .await?;
        Ok(rows.into_iter().map(|(a,)| a).collect())
    }

    /// Replace the grants of `address` with `grants`.
    pub async fn set_send_as_grants(&self, address: &str, grants: &[String]) -> Result<()> {
        let mut tx = self.pg()?.begin().await?;
        sqlx::query("DELETE FROM account_send_as WHERE account_address = $1")
            .bind(address)
            .execute(&mut *tx)
            .await?;
        for grant in grants {
            sqlx::query("INSERT INTO account_send_as (account_address, address) VALUES ($1, $2)")
                .bind(address)
                .bind(grant)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
