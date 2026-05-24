//! Email-group (distribution list) CRUD + membership.

use super::{DomainStore, EmailGroup, Result};

impl DomainStore {
    pub async fn list_email_groups(&self, domain: Option<&str>) -> Result<Vec<EmailGroup>> {
        let pool = self.pg()?;
        let rows = if let Some(d) = domain {
            sqlx::query_as::<_, (i64, String, String, String, String, i64)>(
                "SELECT id, address, domain, name, description, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM email_groups WHERE domain = $1 ORDER BY address",
            )
            .bind(d)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, (i64, String, String, String, String, i64)>(
                "SELECT id, address, domain, name, description, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM email_groups ORDER BY address",
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(id, address, domain, name, description, created_at)| EmailGroup {
                id, address, domain, name, description, created_at,
            })
            .collect())
    }

    pub async fn create_email_group(
        &self,
        address: &str,
        domain: &str,
        name: &str,
        description: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let (id,) = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO email_groups (address, domain, name, description) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(address)
        .bind(domain)
        .bind(name)
        .bind(description)
        .fetch_one(pool)
        .await?;
        // invalidate recipient cache for this address
        self.valkey_del(&format!("rcpt:{address}")).await;
        Ok(id)
    }

    pub async fn remove_email_group(&self, id: i64) -> Result<Option<String>> {
        let pool = self.pg()?;
        let addr = sqlx::query_as::<_, (String,)>(
            "DELETE FROM email_groups WHERE id = $1 RETURNING address",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some((ref address,)) = addr {
            self.valkey_del(&format!("rcpt:{address}")).await;
        }
        Ok(addr.map(|(a,)| a))
    }

    pub async fn list_email_group_members(&self, group_id: i64) -> Result<Vec<String>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT member_address FROM email_group_members \
             WHERE group_id = $1 ORDER BY member_address",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(a,)| a).collect())
    }

    pub async fn add_email_group_member(&self, group_id: i64, member: &str) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO email_group_members (group_id, member_address) \
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(group_id)
        .bind(member)
        .execute(pool)
        .await?;
        // invalidate group address cache + member's permissions (send_as)
        let addr = sqlx::query_as::<_, (String,)>(
            "SELECT address FROM email_groups WHERE id = $1",
        )
        .bind(group_id)
        .fetch_optional(pool)
        .await?;
        if let Some((ref address,)) = addr {
            self.valkey_del(&format!("rcpt:{address}")).await;
        }
        self.invalidate_permissions(member).await;
        Ok(())
    }

    pub async fn remove_email_group_member(
        &self,
        group_id: i64,
        member: &str,
    ) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query(
            "DELETE FROM email_group_members WHERE group_id = $1 AND member_address = $2",
        )
        .bind(group_id)
        .bind(member)
        .execute(pool)
        .await?;
        // invalidate caches
        let addr = sqlx::query_as::<_, (String,)>(
            "SELECT address FROM email_groups WHERE id = $1",
        )
        .bind(group_id)
        .fetch_optional(pool)
        .await?;
        if let Some((ref address,)) = addr {
            self.valkey_del(&format!("rcpt:{address}")).await;
        }
        self.invalidate_permissions(member).await;
        Ok(res.rows_affected() > 0)
    }
}
