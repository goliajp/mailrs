//! Permission group + membership CRUD.

use super::{DomainStore, Result};

impl DomainStore {
    /// list all groups, optionally filtered by domain
    pub async fn list_groups(
        &self,
        domain: Option<&str>,
    ) -> Result<Vec<crate::permission::GroupInfo>> {
        let pool = self.pg()?;
        let rows = if let Some(d) = domain {
            sqlx::query_as::<_, (i64, String, Option<String>, String, bool, i64)>(
                "SELECT id, name, domain, description, is_builtin, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM groups WHERE domain = $1 OR domain IS NULL ORDER BY domain NULLS FIRST, name",
            )
            .bind(d)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, (i64, String, Option<String>, String, bool, i64)>(
                "SELECT id, name, domain, description, is_builtin, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM groups ORDER BY domain NULLS FIRST, name",
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|(id, name, domain, description, is_builtin, created_at)| {
                crate::permission::GroupInfo {
                    id,
                    name,
                    domain,
                    description,
                    is_builtin,
                    created_at,
                }
            })
            .collect())
    }

    /// get permissions for a group
    pub async fn get_group_permissions(&self, group_id: i64) -> Result<Vec<String>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT permission FROM group_permissions WHERE group_id = $1 ORDER BY permission",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(p,)| p).collect())
    }

    /// create a new group
    pub async fn add_group(
        &self,
        name: &str,
        domain: Option<&str>,
        description: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let id = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO groups (name, domain, description) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(name)
        .bind(domain)
        .bind(description)
        .fetch_one(pool)
        .await?;
        Ok(id.0)
    }

    /// remove a group (only non-builtin)
    pub async fn remove_group(&self, id: i64) -> Result<bool> {
        let pool = self.pg()?;
        // protect builtin groups
        let is_builtin =
            sqlx::query_as::<_, (bool,)>("SELECT is_builtin FROM groups WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        if is_builtin == Some((true,)) {
            return Ok(false);
        }
        self.invalidate_group_permissions(id).await;
        let res = sqlx::query("DELETE FROM groups WHERE id = $1 AND NOT is_builtin")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// set permissions for a group (replace all)
    pub async fn set_group_permissions(&self, group_id: i64, permissions: &[String]) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query("DELETE FROM group_permissions WHERE group_id = $1")
            .bind(group_id)
            .execute(pool)
            .await?;
        for perm in permissions {
            sqlx::query(
                "INSERT INTO group_permissions (group_id, permission) VALUES ($1, $2) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(group_id)
            .bind(perm)
            .execute(pool)
            .await?;
        }
        self.invalidate_group_permissions(group_id).await;
        Ok(())
    }

    /// list members of a group
    pub async fn list_group_members(&self, group_id: i64) -> Result<Vec<String>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT account_address FROM account_groups WHERE group_id = $1 ORDER BY account_address",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(a,)| a).collect())
    }

    /// add an account to a group
    pub async fn add_account_to_group(&self, address: &str, group_id: i64) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO account_groups (account_address, group_id) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(address)
        .bind(group_id)
        .execute(pool)
        .await?;
        self.invalidate_permissions(address).await;
        Ok(())
    }

    /// remove an account from a group
    pub async fn remove_account_from_group(&self, address: &str, group_id: i64) -> Result<bool> {
        let pool = self.pg()?;
        let res =
            sqlx::query("DELETE FROM account_groups WHERE account_address = $1 AND group_id = $2")
                .bind(address)
                .bind(group_id)
                .execute(pool)
                .await?;
        self.invalidate_permissions(address).await;
        Ok(res.rows_affected() > 0)
    }

    /// get groups an account belongs to
    pub async fn get_account_groups(
        &self,
        address: &str,
    ) -> Result<Vec<crate::permission::GroupInfo>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, bool, i64)>(
            "SELECT g.id, g.name, g.domain, g.description, g.is_builtin, \
             EXTRACT(EPOCH FROM g.created_at)::bigint \
             FROM account_groups ag \
             JOIN groups g ON g.id = ag.group_id \
             WHERE ag.account_address = $1 \
             ORDER BY g.domain NULLS FIRST, g.name",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, name, domain, description, is_builtin, created_at)| {
                crate::permission::GroupInfo {
                    id,
                    name,
                    domain,
                    description,
                    is_builtin,
                    created_at,
                }
            })
            .collect())
    }

    /// get permission overrides for an account
    pub async fn get_account_overrides(&self, address: &str) -> Result<Vec<(String, bool)>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String, bool)>(
            "SELECT permission, granted FROM account_permission_overrides \
             WHERE account_address = $1 ORDER BY permission",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// set permission overrides for an account (replace all)
    pub async fn set_account_overrides(
        &self,
        address: &str,
        overrides: &[(String, bool)],
    ) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query("DELETE FROM account_permission_overrides WHERE account_address = $1")
            .bind(address)
            .execute(pool)
            .await?;
        for (perm, granted) in overrides {
            sqlx::query(
                "INSERT INTO account_permission_overrides (account_address, permission, granted) \
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(address)
            .bind(perm)
            .bind(granted)
            .execute(pool)
            .await?;
        }
        self.invalidate_permissions(address).await;
        Ok(())
    }
}
