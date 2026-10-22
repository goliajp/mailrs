//! Alias CRUD.

use super::{Alias, DomainStore, Result};

/// Stable i64 id derived from an alias source address. Matches the
/// `alias_id` helper in `fastcore/src/routes/mail_admin.rs` so
/// consumers who look up by id (webapi remove) can carry an id
/// across backends — network-kevy has no serial PK to lean on.
fn alias_id_of(source: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    (h.finish() >> 1) as i64
}

fn domain_of(addr: &str) -> String {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("").into()
}

impl DomainStore {
    pub async fn list_aliases(&self) -> Result<Vec<Alias>> {
        // RFC 20260705 Step 3: when the alias_store seam is wired
        // (network kevy backend), it is authoritative — the PG rows
        // stop being written and reading both would show a stale mix.
        if let Some(store) = self.alias_store.clone() {
            let pairs = tokio::task::spawn_blocking(move || store.list())
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            let mut out: Vec<Alias> = pairs
                .into_iter()
                .map(|(source, target)| Alias {
                    id: alias_id_of(&source),
                    domain: domain_of(&source),
                    source_address: source,
                    target_address: target,
                    alias_type: "alias".into(),
                    active: true,
                    created_at: 0,
                })
                .collect();
            out.sort_by(|a, b| a.source_address.cmp(&b.source_address));
            return Ok(out);
        }

        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, String, String, String, String, bool, i64)>(
            "SELECT id, source_address, target_address, domain, alias_type, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM aliases ORDER BY source_address",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, source_address, target_address, domain, alias_type, active, created_at)| {
                    Alias {
                        id,
                        source_address,
                        target_address,
                        domain,
                        alias_type,
                        active,
                        created_at,
                    }
                },
            )
            .collect())
    }

    /// Source-keyed upsert — the backend-neutral alias API both cores
    /// serve for the v2 switchable-core boundary (kevy is source-keyed;
    /// PG gets this so webapi + `mailrs-core-sync` drive one identical
    /// route on either backend). `domain` is derived from the source's
    /// `@` part; `alias_type` defaults to `alias`. Idempotent per source.
    pub async fn upsert_alias_by_source(&self, source: &str, target: &str) -> Result<()> {
        if let Some(store) = self.alias_store.clone() {
            let source_owned = source.to_string();
            let target_owned = target.to_string();
            tokio::task::spawn_blocking(move || store.upsert(&source_owned, &target_owned))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            return Ok(());
        }
        let pool = self.pg()?;
        let domain = source.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        // replaces every row for the source: the table's unique key is
        // (source, target), so there is no conflict target for "by source"
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM aliases WHERE source_address = $1")
            .bind(source)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO aliases (source_address, target_address, domain, alias_type) \
             VALUES ($1, $2, $3, 'alias')",
        )
        .bind(source)
        .bind(target)
        .bind(domain)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Source-keyed delete — companion to [`Self::upsert_alias_by_source`].
    pub async fn remove_alias_by_source(&self, source: &str) -> Result<bool> {
        if let Some(store) = self.alias_store.clone() {
            let source_owned = source.to_string();
            let removed = tokio::task::spawn_blocking(move || store.delete(&source_owned))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            return Ok(removed);
        }
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM aliases WHERE source_address = $1")
            .bind(source)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
