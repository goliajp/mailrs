use std::time::Instant;

use dashmap::DashMap;
use serde::Serialize;
use sqlx::PgPool;

use crate::health::HealthState;

/// cache entries expire after 5 minutes
const CACHE_TTL_SECS: u64 = 300;

pub struct DomainStore {
    pg: Option<PgPool>,
    valkey: Option<redis::aio::ConnectionManager>,
    health: HealthState,
    // process-level cache for L3 degradation
    account_cache: DashMap<String, CachedAccount>,
}

#[derive(Clone)]
struct CachedAccount {
    account: Account,
    password_hash: String,
    cached_at: Instant,
}

#[derive(Debug, Serialize, Clone)]
pub struct Domain {
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct Account {
    pub address: String,
    pub domain: String,
    pub display_name: String,
    pub active: bool,
    pub created_at: i64,
    pub quota_bytes: i64,
    pub recovery_email: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Alias {
    pub id: i64,
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    pub alias_type: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum ResolvedRecipient {
    Account(String),
    /// group email: deliver a copy to each member's mailbox
    Group(Vec<String>),
    Forward(Vec<String>),
    Reject,
}

type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug)]
pub enum StoreError {
    Pg(sqlx::Error),
    Unavailable,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Pg(e) => write!(f, "database error: {e}"),
            StoreError::Unavailable => write!(f, "storage unavailable"),
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Pg(e)
    }
}

impl DomainStore {
    pub fn new(
        pg: Option<PgPool>,
        valkey: Option<redis::aio::ConnectionManager>,
        health: HealthState,
    ) -> Self {
        Self {
            pg,
            valkey,
            health,
            account_cache: DashMap::new(),
        }
    }

    /// number of entries in the in-process account cache
    pub fn cache_size(&self) -> usize {
        self.account_cache.len()
    }

    /// evict expired entries from the in-process cache
    pub fn evict_expired(&self) -> usize {
        let before = self.account_cache.len();
        self.account_cache
            .retain(|_, v| v.cached_at.elapsed().as_secs() < CACHE_TTL_SECS);
        before - self.account_cache.len()
    }

    fn pg(&self) -> Result<&PgPool> {
        match (&self.pg, self.health.pg_up()) {
            (Some(pool), true) => Ok(pool),
            (Some(pool), false) => {
                // health check says down but pool might still work
                Ok(pool)
            }
            _ => Err(StoreError::Unavailable),
        }
    }

    // --- TOTP 2FA ---

    /// get TOTP secret, enabled status, and recovery codes for an account
    pub async fn get_totp_secret(
        &self,
        address: &str,
    ) -> Result<Option<(String, bool, String)>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (String, bool, String)>(
            "SELECT secret, enabled, recovery_codes FROM totp_secrets \
             WHERE account_address = $1",
        )
        .bind(address)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// save (upsert) a TOTP secret and recovery codes for an account
    pub async fn save_totp_secret(
        &self,
        address: &str,
        secret: &str,
        recovery_codes: &str,
    ) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO totp_secrets (account_address, secret, recovery_codes) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (account_address) \
             DO UPDATE SET secret = $2, recovery_codes = $3, enabled = false",
        )
        .bind(address)
        .bind(secret)
        .bind(recovery_codes)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// enable TOTP for an account (returns true if a row was updated)
    pub async fn enable_totp(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let result = sqlx::query(
            "UPDATE totp_secrets SET enabled = true WHERE account_address = $1",
        )
        .bind(address)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// disable and delete TOTP for an account (returns true if a row was deleted)
    pub async fn disable_totp(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let result = sqlx::query(
            "DELETE FROM totp_secrets WHERE account_address = $1",
        )
        .bind(address)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// consume a recovery code (remove it from the list); returns true if code was valid
    pub async fn consume_recovery_code(&self, address: &str, code: &str) -> Result<bool> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT recovery_codes FROM totp_secrets \
             WHERE account_address = $1 AND enabled = true",
        )
        .bind(address)
        .fetch_optional(pool)
        .await?;

        let Some((codes_str,)) = row else {
            return Ok(false);
        };

        let codes: Vec<&str> = codes_str.split(',').collect();
        if !codes.contains(&code) {
            return Ok(false);
        }

        let remaining: Vec<&str> = codes.into_iter().filter(|c| *c != code).collect();
        let new_codes = remaining.join(",");

        sqlx::query(
            "UPDATE totp_secrets SET recovery_codes = $1 WHERE account_address = $2",
        )
        .bind(&new_codes)
        .bind(address)
        .execute(pool)
        .await?;

        Ok(true)
    }

    /// log a security-sensitive action to the audit_log table (fire-and-forget)
    pub async fn log_audit(&self, actor: &str, action: &str, target: &str, detail: &str) {
        if let Ok(pool) = self.pg() {
            let _ = sqlx::query(
                "INSERT INTO audit_log (actor, action, target, detail) VALUES ($1, $2, $3, $4)",
            )
            .bind(actor)
            .bind(action)
            .bind(target)
            .bind(detail)
            .execute(pool)
            .await;
        }
    }

    /// query recent audit log entries
    pub async fn list_audit_log(&self, limit: i64) -> Result<Vec<AuditEntry>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, i64, String, String, String, String)>(
            "SELECT id, EXTRACT(EPOCH FROM timestamp)::bigint, actor, action, target, detail \
             FROM audit_log ORDER BY timestamp DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, timestamp, actor, action, target, detail)| AuditEntry {
                id,
                timestamp,
                actor,
                action,
                target,
                detail,
            })
            .collect())
    }

    /// delete audit log entries older than the given number of days
    pub async fn cleanup_audit_log(&self, retention_days: i64) {
        if let Ok(pool) = self.pg() {
            let _ = sqlx::query("DELETE FROM audit_log WHERE timestamp < now() - make_interval(days => $1)")
                .bind(retention_days)
                .execute(pool)
                .await;
        }
    }

    /// preload all accounts into process cache for L3 degradation
    pub async fn preload_accounts(&self) {
        let Ok(pool) = self.pg() else { return };
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
            "SELECT address, domain, display_name, active, \
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, recovery_email \
             FROM accounts",
        )
        .fetch_all(pool)
        .await;

        if let Ok(rows) = rows {
            for (address, domain, display_name, active, created_at, quota_bytes, password_hash, recovery_email) in
                rows
            {
                let account = Account {
                    address: address.clone(),
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                };
                self.account_cache.insert(
                    address,
                    CachedAccount {
                        account,
                        password_hash,
                        cached_at: Instant::now(),
                    },
                );
            }
        }
    }

    // --- Valkey cache helpers ---

    async fn valkey_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.valkey.clone()?;
        let val: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .ok()?;
        val.and_then(|s| serde_json::from_str(&s).ok())
    }

    async fn valkey_set(&self, key: &str, val: &impl serde::Serialize, ttl_secs: u64) {
        if let Some(mut conn) = self.valkey.clone()
            && let Ok(json) = serde_json::to_string(val) {
                let _: std::result::Result<(), _> = redis::cmd("SET")
                    .arg(key)
                    .arg(&json)
                    .arg("EX")
                    .arg(ttl_secs)
                    .query_async(&mut conn)
                    .await;
            }
    }

    async fn valkey_del(&self, key: &str) {
        if let Some(mut conn) = self.valkey.clone() {
            let _: std::result::Result<(), _> =
                redis::cmd("DEL").arg(key).query_async(&mut conn).await;
        }
    }

    // --- domains ---

    pub async fn list_domains(&self) -> Result<Vec<Domain>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT name, EXTRACT(EPOCH FROM created_at)::bigint FROM domains ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(name, created_at)| Domain { name, created_at })
            .collect())
    }

    pub async fn add_domain(&self, name: &str, _now: i64) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn remove_domain(&self, name: &str) -> Result<bool> {
        let pool = self.pg()?;
        // collect affected accounts before cascade delete
        let addresses: Vec<(String,)> =
            sqlx::query_as("SELECT address FROM accounts WHERE domain = $1")
                .bind(name)
                .fetch_all(pool)
                .await?;

        // cascade deletes accounts, aliases via FK
        let res = sqlx::query("DELETE FROM domains WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await?;

        // invalidate process cache
        self.account_cache.retain(|_, v| v.account.domain != name);

        // invalidate Valkey cache for all affected accounts
        for (addr,) in &addresses {
            self.valkey_del(&format!("acct:{addr}")).await;
            self.valkey_del(&format!("rcpt:{addr}")).await;
        }

        Ok(res.rows_affected() > 0)
    }

    // --- accounts ---

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String)>(
            "SELECT address, domain, display_name, active, \
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, recovery_email \
             FROM accounts ORDER BY address",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(address, domain, display_name, active, created_at, quota_bytes, recovery_email)| Account {
                    address,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                },
            )
            .collect())
    }

    pub async fn add_account(
        &self,
        address: &str,
        domain: &str,
        display_name: &str,
        password_hash: &str,
        _now: i64,
    ) -> Result<()> {
        let pool = self.pg()?;
        let (active, created_epoch, quota_bytes, recovery_email): (bool, i64, i64, String) = sqlx::query_as(
            "INSERT INTO accounts (address, domain, display_name, password_hash) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (address) DO UPDATE SET \
             domain = EXCLUDED.domain, display_name = EXCLUDED.display_name, \
             password_hash = EXCLUDED.password_hash \
             RETURNING active, EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, recovery_email",
        )
        .bind(address)
        .bind(domain)
        .bind(display_name)
        .bind(password_hash)
        .fetch_one(pool)
        .await?;

        // update caches with actual PG state
        let account = Account {
            address: address.to_string(),
            domain: domain.to_string(),
            display_name: display_name.to_string(),
            active,
            created_at: created_epoch,
            quota_bytes,
            recovery_email,
        };
        self.account_cache.insert(
            address.to_string(),
            CachedAccount {
                account,
                password_hash: password_hash.to_string(),
                cached_at: Instant::now(),
            },
        );
        self.valkey_del(&format!("acct:{address}")).await;
        self.valkey_del(&format!("rcpt:{address}")).await;

        Ok(())
    }

    pub async fn update_account_display_name(&self, address: &str, display_name: &str) -> Result<bool> {
        let pool = self.pg()?;
        let rows = sqlx::query(
            "UPDATE accounts SET display_name = $2 WHERE address = $1",
        )
        .bind(address)
        .bind(display_name)
        .execute(pool)
        .await?
        .rows_affected();
        if rows > 0 {
            // invalidate caches
            self.account_cache.remove(address);
            self.valkey_del(&format!("acct:{address}")).await;
        }
        Ok(rows > 0)
    }

    pub async fn get_account_with_hash(&self, address: &str) -> Result<Option<(Account, String)>> {
        // try valkey cache
        let cache_key = format!("acct:{address}");
        if let Some(cached) = self.valkey_get::<CachedAccount>(&cache_key).await {
            return Ok(Some((cached.account, cached.password_hash)));
        }

        // try PG
        if let Ok(pool) = self.pg() {
            let row = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
                "SELECT address, domain, display_name, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, recovery_email \
                 FROM accounts WHERE address = $1",
            )
            .bind(address)
            .fetch_optional(pool)
            .await?;

            if let Some((addr, domain, display_name, active, created_at, quota_bytes, hash, recovery_email)) = row
            {
                let account = Account {
                    address: addr,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                };
                let cached = CachedAccount {
                    account: account.clone(),
                    password_hash: hash.clone(),
                    cached_at: Instant::now(),
                };
                // backfill caches
                self.valkey_set(&cache_key, &cached, 300).await;
                self.account_cache.insert(address.to_string(), cached);
                return Ok(Some((account, hash)));
            }
            return Ok(None);
        }

        // L3 fallback: process cache
        if let Some(entry) = self.account_cache.get(address) {
            if entry.cached_at.elapsed().as_secs() < CACHE_TTL_SECS {
                return Ok(Some((entry.account.clone(), entry.password_hash.clone())));
            }
            drop(entry);
            self.account_cache.remove(address);
        }

        Ok(None)
    }

    pub async fn remove_account(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM accounts WHERE address = $1")
            .bind(address)
            .execute(pool)
            .await?;
        self.account_cache.remove(address);
        self.valkey_del(&format!("acct:{address}")).await;
        self.valkey_del(&format!("rcpt:{address}")).await;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_quota(&self, address: &str) -> Result<Option<i64>> {
        if let Ok(Some((account, _))) = self.get_account_with_hash(address).await {
            return Ok(Some(account.quota_bytes));
        }
        Ok(None)
    }

    pub async fn set_quota(&self, address: &str, quota_bytes: i64) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE accounts SET quota_bytes = $1 WHERE address = $2")
            .bind(quota_bytes)
            .bind(address)
            .execute(pool)
            .await?;
        // invalidate caches
        if let Some(mut entry) = self.account_cache.get_mut(address) {
            entry.account.quota_bytes = quota_bytes;
        }
        self.valkey_del(&format!("acct:{address}")).await;
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_recovery_email(&self, address: &str, recovery_email: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE accounts SET recovery_email = $1 WHERE address = $2")
            .bind(recovery_email)
            .bind(address)
            .execute(pool)
            .await?;
        // invalidate caches
        if let Some(mut entry) = self.account_cache.get_mut(address) {
            entry.account.recovery_email = recovery_email.to_string();
        }
        self.valkey_del(&format!("acct:{address}")).await;
        Ok(res.rows_affected() > 0)
    }

    // --- sieve scripts ---

    pub async fn get_sieve_script(&self, address: &str) -> Result<Option<String>> {
        let pool = self.pg()?;
        let row =
            sqlx::query_as::<_, (String,)>("SELECT script FROM sieve_scripts WHERE address = $1")
                .bind(address)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(s,)| s))
    }

    pub async fn set_sieve_script(&self, address: &str, script: &str, _now: i64) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO sieve_scripts (address, script) VALUES ($1, $2) \
             ON CONFLICT (address) DO UPDATE SET script = EXCLUDED.script, updated_at = now()",
        )
        .bind(address)
        .bind(script)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_sieve_script(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM sieve_scripts WHERE address = $1")
            .bind(address)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // --- aliases ---

    pub async fn list_aliases(&self) -> Result<Vec<Alias>> {
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

    pub async fn add_alias(
        &self,
        source: &str,
        target: &str,
        domain: &str,
        alias_type: &str,
        _now: i64,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO aliases (source_address, target_address, domain, alias_type) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(source)
        .bind(target)
        .bind(domain)
        .bind(alias_type)
        .fetch_one(pool)
        .await?;
        // invalidate recipient cache
        self.valkey_del(&format!("rcpt:{source}")).await;
        Ok(row.0)
    }

    pub async fn remove_alias(&self, id: i64) -> Result<bool> {
        let pool = self.pg()?;
        // get source_address before deleting for cache invalidation
        let source =
            sqlx::query_as::<_, (String,)>("SELECT source_address FROM aliases WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?;

        let res = sqlx::query("DELETE FROM aliases WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if let Some((source_addr,)) = source {
            self.valkey_del(&format!("rcpt:{source_addr}")).await;
        }
        Ok(res.rows_affected() > 0)
    }

    /// resolve a recipient address to local account, forward, or reject
    /// resolution order: exact account → exact alias → catch-all → Reject
    pub async fn resolve_recipient(&self, address: &str) -> ResolvedRecipient {
        // try valkey cache
        let cache_key = format!("rcpt:{address}");
        if let Some(cached) = self.valkey_get::<CachedResolution>(&cache_key).await {
            return cached.into();
        }

        let result = self.resolve_recipient_inner(address).await;

        // cache the result
        let cacheable: CachedResolution = (&result).into();
        self.valkey_set(&cache_key, &cacheable, 300).await;

        result
    }

    async fn resolve_recipient_inner(&self, address: &str) -> ResolvedRecipient {
        let pool = match self.pg() {
            Ok(p) => p,
            Err(_) => {
                // L3 fallback: check process cache
                if self.account_cache.contains_key(address) {
                    return ResolvedRecipient::Account(address.to_string());
                }
                // when uncertain, accept
                return ResolvedRecipient::Account(address.to_string());
            }
        };

        // 1. exact account match
        let has_account = sqlx::query_as::<_, (bool,)>(
            "SELECT true FROM accounts WHERE address = $1 AND active = true",
        )
        .bind(address)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some();

        if has_account {
            return ResolvedRecipient::Account(address.to_string());
        }

        // 2. email group match
        let group_members: Vec<(String,)> = sqlx::query_as(
            "SELECT m.member_address FROM email_group_members m \
             JOIN email_groups g ON g.id = m.group_id \
             WHERE g.address = $1",
        )
        .bind(address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !group_members.is_empty() {
            let members: Vec<String> = group_members.into_iter().map(|(a,)| a).collect();
            return ResolvedRecipient::Group(members);
        }

        // 3. exact alias match
        let targets: Vec<(String, String)> = sqlx::query_as(
            "SELECT target_address, alias_type FROM aliases \
             WHERE source_address = $1 AND active = true",
        )
        .bind(address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !targets.is_empty() {
            return self.resolve_targets(pool, &targets).await;
        }

        // 3. catch-all (*@domain)
        if let Some((_, domain)) = address.split_once('@') {
            let catchall = format!("*@{domain}");
            let targets: Vec<(String, String)> = sqlx::query_as(
                "SELECT target_address, alias_type FROM aliases \
                 WHERE source_address = $1 AND active = true",
            )
            .bind(&catchall)
            .fetch_all(pool)
            .await
            .unwrap_or_default();

            if !targets.is_empty() {
                return self.resolve_targets(pool, &targets).await;
            }
        }

        ResolvedRecipient::Reject
    }

    async fn resolve_targets(
        &self,
        pool: &PgPool,
        targets: &[(String, String)],
    ) -> ResolvedRecipient {
        let mut local_accounts = Vec::new();
        let mut forwards = Vec::new();

        for (target, alias_type) in targets {
            if alias_type == "forward" {
                forwards.push(target.clone());
            } else {
                let is_local = sqlx::query_as::<_, (bool,)>(
                    "SELECT true FROM accounts WHERE address = $1 AND active = true",
                )
                .bind(target)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .is_some();

                if is_local {
                    local_accounts.push(target.clone());
                } else {
                    forwards.push(target.clone());
                }
            }
        }

        if !local_accounts.is_empty() && forwards.is_empty() {
            ResolvedRecipient::Account(local_accounts.into_iter().next().unwrap())
        } else if !forwards.is_empty() {
            forwards.extend(local_accounts);
            ResolvedRecipient::Forward(forwards)
        } else {
            ResolvedRecipient::Reject
        }
    }

    // --- RBAC: groups & permissions ---

    /// load effective permissions for an account
    pub async fn load_account_permissions(
        &self,
        address: &str,
    ) -> Result<crate::permission::EffectivePermissions> {
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};

        // try valkey cache
        let cache_key = format!("perms:{address}");
        if let Some(cached) =
            self.valkey_get::<crate::permission::EffectivePermissions>(&cache_key)
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

        let perms =
            compute_effective_permissions(&groups, &override_rows, &all_domains).with_send_as(send_as);

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

    /// list all groups, optionally filtered by domain
    pub async fn list_groups(&self, domain: Option<&str>) -> Result<Vec<crate::permission::GroupInfo>> {
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
        let is_builtin = sqlx::query_as::<_, (bool,)>(
            "SELECT is_builtin FROM groups WHERE id = $1",
        )
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
    pub async fn set_group_permissions(
        &self,
        group_id: i64,
        permissions: &[String],
    ) -> Result<()> {
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
        let res = sqlx::query(
            "DELETE FROM account_groups WHERE account_address = $1 AND group_id = $2",
        )
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

    // --- apps ---

    /// list all apps, optionally filtered by owner
    pub async fn list_apps(&self, owner: Option<&str>) -> Result<Vec<App>> {
        let pool = self.pg()?;
        let rows = if let Some(owner_addr) = owner {
            sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
                "SELECT id, app_id, name, description, owner_address, scopes, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM apps WHERE owner_address = $1 ORDER BY name",
            )
            .bind(owner_addr)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
                "SELECT id, app_id, name, description, owner_address, scopes, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM apps ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                    id,
                    app_id,
                    name,
                    description,
                    owner_address,
                    scopes,
                    active,
                    created_at,
                },
            )
            .collect())
    }

    /// create a new app, returns the internal id
    pub async fn create_app(
        &self,
        app_id: &str,
        name: &str,
        description: &str,
        owner_address: &str,
        scopes: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let (id,) = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO apps (app_id, name, description, owner_address, scopes) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(app_id)
        .bind(name)
        .bind(description)
        .bind(owner_address)
        .bind(scopes)
        .fetch_one(pool)
        .await?;
        Ok(id)
    }

    /// get an app by app_id
    pub async fn get_app(&self, app_id: &str) -> Result<Option<App>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
            "SELECT id, app_id, name, description, owner_address, scopes, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM apps WHERE app_id = $1",
        )
        .bind(app_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(
            |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                id,
                app_id,
                name,
                description,
                owner_address,
                scopes,
                active,
                created_at,
            },
        ))
    }

    /// get an app by internal id
    pub async fn get_app_by_id(&self, id: i64) -> Result<Option<App>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
            "SELECT id, app_id, name, description, owner_address, scopes, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM apps WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(
            |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                id,
                app_id,
                name,
                description,
                owner_address,
                scopes,
                active,
                created_at,
            },
        ))
    }

    /// remove an app (cascades to its api_keys)
    pub async fn remove_app(&self, app_id: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM apps WHERE app_id = $1")
            .bind(app_id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// update app scopes
    pub async fn update_app_scopes(&self, app_id: &str, scopes: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE apps SET scopes = $1 WHERE app_id = $2")
            .bind(scopes)
            .bind(app_id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // --- email groups ---

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

    // --- encryption keys (PGP / S/MIME) ---

    /// get an encryption key by address and type; returns (id, public_key, fingerprint)
    pub async fn get_encryption_key(
        &self,
        address: &str,
        key_type: &str,
    ) -> Result<Option<(i64, String, String)>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, public_key, fingerprint FROM encryption_keys \
             WHERE account_address = $1 AND key_type = $2",
        )
        .bind(address)
        .bind(key_type)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// upsert an encryption key; returns the row id
    pub async fn set_encryption_key(
        &self,
        address: &str,
        key_type: &str,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO encryption_keys (account_address, key_type, public_key, fingerprint) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (account_address, key_type) \
             DO UPDATE SET public_key = $3, fingerprint = $4 \
             RETURNING id",
        )
        .bind(address)
        .bind(key_type)
        .bind(public_key)
        .bind(fingerprint)
        .fetch_one(pool)
        .await?;
        Ok(id)
    }

    /// delete an encryption key; returns true if a row was deleted
    pub async fn delete_encryption_key(
        &self,
        address: &str,
        key_type: &str,
    ) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query(
            "DELETE FROM encryption_keys WHERE account_address = $1 AND key_type = $2",
        )
        .bind(address)
        .bind(key_type)
        .execute(pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// list all encryption keys for an account; returns (id, key_type, fingerprint, created_at epoch)
    pub async fn list_encryption_keys(
        &self,
        address: &str,
    ) -> Result<Vec<(i64, String, String, i64)>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT id, key_type, fingerprint, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM encryption_keys WHERE account_address = $1 \
             ORDER BY created_at",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}

// --- EmailGroup struct ---

#[derive(Debug, Serialize, Clone)]
pub struct EmailGroup {
    pub id: i64,
    pub address: String,
    pub domain: String,
    pub name: String,
    pub description: String,
    pub created_at: i64,
}

// --- App struct ---

#[derive(Debug, Serialize, Clone)]
pub struct App {
    pub id: i64,
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub owner_address: String,
    pub scopes: String,
    pub active: bool,
    pub created_at: i64,
}

// --- cached resolution for valkey ---

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CachedResolution {
    kind: String,
    addresses: Vec<String>,
}

impl From<CachedResolution> for ResolvedRecipient {
    fn from(c: CachedResolution) -> Self {
        match c.kind.as_str() {
            "account" => {
                ResolvedRecipient::Account(c.addresses.into_iter().next().unwrap_or_default())
            }
            "group" => ResolvedRecipient::Group(c.addresses),
            "forward" => ResolvedRecipient::Forward(c.addresses),
            _ => ResolvedRecipient::Reject,
        }
    }
}

impl From<&ResolvedRecipient> for CachedResolution {
    fn from(r: &ResolvedRecipient) -> Self {
        match r {
            ResolvedRecipient::Account(a) => CachedResolution {
                kind: "account".into(),
                addresses: vec![a.clone()],
            },
            ResolvedRecipient::Group(members) => CachedResolution {
                kind: "group".into(),
                addresses: members.clone(),
            },
            ResolvedRecipient::Forward(addrs) => CachedResolution {
                kind: "forward".into(),
                addresses: addrs.clone(),
            },
            ResolvedRecipient::Reject => CachedResolution {
                kind: "reject".into(),
                addresses: vec![],
            },
        }
    }
}

// serde for CachedAccount (valkey)
impl serde::Serialize for CachedAccount {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CachedAccount", 2)?;
        s.serialize_field("account", &self.account)?;
        s.serialize_field("password_hash", &self.password_hash)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for CachedAccount {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            account: Account,
            password_hash: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(CachedAccount {
            account: h.account,
            password_hash: h.password_hash,
            cached_at: Instant::now(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::time::Duration;

    // helper to build an Account with sensible defaults
    fn make_account(address: &str) -> Account {
        Account {
            address: address.to_string(),
            domain: "example.com".to_string(),
            display_name: "Test User".to_string(),
            active: true,
            created_at: 1700000000,
            quota_bytes: 1_073_741_824,
            recovery_email: String::new(),
        }
    }

    // helper to build a CachedAccount
    fn make_cached(address: &str, hash: &str) -> CachedAccount {
        CachedAccount {
            account: make_account(address),
            password_hash: hash.to_string(),
            cached_at: Instant::now(),
        }
    }

    // helper to build a DomainStore without PG/Valkey
    fn make_store() -> DomainStore {
        DomainStore::new(None, None, HealthState::new())
    }

    // --- CachedAccount serde ---

    #[test]
    fn cached_account_serde_roundtrip() {
        let original = make_cached("alice@example.com", "$argon2id$hash");
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: CachedAccount = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.account.address, "alice@example.com");
        assert_eq!(deserialized.password_hash, "$argon2id$hash");
        // cached_at is reset to Instant::now() on deserialize, so just verify it exists
        assert!(deserialized.cached_at.elapsed().as_secs() < 1);
    }

    #[test]
    fn cached_account_serialized_fields() {
        let cached = make_cached("bob@example.com", "secret_hash");
        let json = serde_json::to_string(&cached).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // should have exactly "account" and "password_hash" fields
        assert!(parsed.get("account").is_some());
        assert!(parsed.get("password_hash").is_some());
        // should NOT serialize cached_at (Instant is not serializable directly)
        assert!(parsed.get("cached_at").is_none());
    }

    #[test]
    fn cached_account_clone_independence() {
        let original = make_cached("clone@example.com", "hash1");
        let cloned = original.clone();

        assert_eq!(cloned.account.address, original.account.address);
        assert_eq!(cloned.password_hash, original.password_hash);
    }

    // --- Account serde & defaults ---

    #[test]
    fn account_deserialize_all_fields() {
        let json = r#"{
            "address": "admin@example.com",
            "domain": "example.com",
            "display_name": "Admin",
            "active": true,
            "created_at": 1700000000,
            "quota_bytes": 1073741824
        }"#;
        let account: Account = serde_json::from_str(json).unwrap();
        assert_eq!(account.address, "admin@example.com");
        assert_eq!(account.quota_bytes, 1073741824);
    }

    #[test]
    fn account_serialize_all_fields() {
        let account = Account {
            address: "full@example.com".into(),
            domain: "example.com".into(),
            display_name: "Full Account".into(),
            active: false,
            created_at: 1234567890,
            quota_bytes: 500_000_000,
            recovery_email: String::new(),
        };
        let json = serde_json::to_string(&account).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["address"], "full@example.com");
        assert_eq!(parsed["domain"], "example.com");
        assert_eq!(parsed["display_name"], "Full Account");
        assert_eq!(parsed["active"], false);
        assert_eq!(parsed["created_at"], 1234567890);
        assert_eq!(parsed["quota_bytes"], 500_000_000);
    }

    #[test]
    fn account_serde_roundtrip() {
        let original = make_account("roundtrip@example.com");
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Account = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.address, original.address);
        assert_eq!(deserialized.domain, original.domain);
        assert_eq!(deserialized.display_name, original.display_name);
        assert_eq!(deserialized.active, original.active);
        assert_eq!(deserialized.created_at, original.created_at);
        assert_eq!(deserialized.quota_bytes, original.quota_bytes);
    }

    // --- DomainStore cache operations (no DB) ---

    #[test]
    fn store_empty_cache_size() {
        let store = make_store();
        assert_eq!(store.cache_size(), 0);
    }

    #[test]
    fn store_cache_insert_and_size() {
        let store = make_store();
        store
            .account_cache
            .insert("a@example.com".into(), make_cached("a@example.com", "h1"));
        store
            .account_cache
            .insert("b@example.com".into(), make_cached("b@example.com", "h2"));
        assert_eq!(store.cache_size(), 2);
    }

    #[test]
    fn store_evict_expired_none_expired() {
        let store = make_store();
        store
            .account_cache
            .insert("fresh@example.com".into(), make_cached("fresh@example.com", "h"));
        let evicted = store.evict_expired();
        assert_eq!(evicted, 0);
        assert_eq!(store.cache_size(), 1);
    }

    #[test]
    fn store_evict_expired_with_stale_entry() {
        let store = make_store();

        // insert a fresh entry
        store.account_cache.insert(
            "fresh@example.com".into(),
            make_cached("fresh@example.com", "h1"),
        );

        // insert a stale entry with cached_at far in the past
        let stale = CachedAccount {
            account: make_account("stale@example.com"),
            password_hash: "h2".into(),
            cached_at: Instant::now() - Duration::from_secs(CACHE_TTL_SECS + 10),
        };
        store
            .account_cache
            .insert("stale@example.com".into(), stale);

        assert_eq!(store.cache_size(), 2);
        let evicted = store.evict_expired();
        assert_eq!(evicted, 1);
        assert_eq!(store.cache_size(), 1);
        assert!(store.account_cache.contains_key("fresh@example.com"));
        assert!(!store.account_cache.contains_key("stale@example.com"));
    }

    #[test]
    fn store_evict_expired_all_stale() {
        let store = make_store();
        for i in 0..5 {
            let addr = format!("user{i}@example.com");
            let stale = CachedAccount {
                account: make_account(&addr),
                password_hash: "hash".into(),
                cached_at: Instant::now() - Duration::from_secs(CACHE_TTL_SECS + 1),
            };
            store.account_cache.insert(addr, stale);
        }
        assert_eq!(store.cache_size(), 5);
        let evicted = store.evict_expired();
        assert_eq!(evicted, 5);
        assert_eq!(store.cache_size(), 0);
    }

    #[test]
    fn store_evict_expired_empty_cache() {
        let store = make_store();
        let evicted = store.evict_expired();
        assert_eq!(evicted, 0);
    }

    // --- TTL boundary check ---

    #[test]
    fn ttl_boundary_not_yet_expired() {
        let store = make_store();
        // entry at exactly TTL - 1 second should survive
        let entry = CachedAccount {
            account: make_account("boundary@example.com"),
            password_hash: "hash".into(),
            cached_at: Instant::now() - Duration::from_secs(CACHE_TTL_SECS - 1),
        };
        store
            .account_cache
            .insert("boundary@example.com".into(), entry);
        let evicted = store.evict_expired();
        assert_eq!(evicted, 0);
        assert_eq!(store.cache_size(), 1);
    }

    #[test]
    fn ttl_boundary_just_expired() {
        let store = make_store();
        // entry at exactly TTL + 1 second should be evicted
        let entry = CachedAccount {
            account: make_account("expired@example.com"),
            password_hash: "hash".into(),
            cached_at: Instant::now() - Duration::from_secs(CACHE_TTL_SECS + 1),
        };
        store
            .account_cache
            .insert("expired@example.com".into(), entry);
        let evicted = store.evict_expired();
        assert_eq!(evicted, 1);
        assert_eq!(store.cache_size(), 0);
    }

    // --- DomainStore.pg() returns Unavailable without PG ---

    #[test]
    fn store_pg_unavailable_without_pool() {
        let store = make_store();
        let result = store.pg();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StoreError::Unavailable));
    }

    // --- StoreError Display ---

    #[test]
    fn store_error_unavailable_display() {
        let err = StoreError::Unavailable;
        assert_eq!(format!("{err}"), "storage unavailable");
    }

    #[test]
    fn store_error_pg_display() {
        let pg_err = sqlx::Error::RowNotFound;
        let err = StoreError::Pg(pg_err);
        let msg = format!("{err}");
        assert!(msg.starts_with("database error:"));
    }

    #[test]
    fn store_error_from_sqlx() {
        let pg_err = sqlx::Error::RowNotFound;
        let err: StoreError = pg_err.into();
        assert!(matches!(err, StoreError::Pg(_)));
    }

    // --- CACHE_TTL_SECS constant ---

    #[test]
    fn cache_ttl_is_five_minutes() {
        assert_eq!(CACHE_TTL_SECS, 300);
    }

    // --- CachedResolution (existing tests below) ---

    #[test]
    fn cached_resolution_account_roundtrip() {
        let orig = ResolvedRecipient::Account("alice@example.com".into());
        let cached = CachedResolution::from(&orig);
        let back: ResolvedRecipient = cached.into();
        assert!(matches!(back, ResolvedRecipient::Account(ref a) if a == "alice@example.com"));
    }

    #[test]
    fn cached_resolution_forward_roundtrip() {
        let targets = vec!["bob@example.com".into(), "carol@example.com".into()];
        let orig = ResolvedRecipient::Forward(targets.clone());
        let cached = CachedResolution::from(&orig);
        let back: ResolvedRecipient = cached.into();
        match back {
            ResolvedRecipient::Forward(addrs) => assert_eq!(addrs, targets),
            other => panic!("expected Forward, got {other:?}"),
        }
    }

    #[test]
    fn cached_resolution_reject_roundtrip() {
        let orig = ResolvedRecipient::Reject;
        let cached = CachedResolution::from(&orig);
        let back: ResolvedRecipient = cached.into();
        assert!(matches!(back, ResolvedRecipient::Reject));
    }

    #[test]
    fn cached_resolution_unknown_kind_becomes_reject() {
        let cached = CachedResolution {
            kind: "garbage".into(),
            addresses: vec![],
        };
        let result: ResolvedRecipient = cached.into();
        assert!(matches!(result, ResolvedRecipient::Reject));
    }

    #[test]
    fn cached_resolution_serde_roundtrip() {
        let variants = vec![
            ResolvedRecipient::Account("test@example.com".into()),
            ResolvedRecipient::Forward(vec!["a@b.com".into(), "c@d.com".into()]),
            ResolvedRecipient::Reject,
        ];
        for orig in &variants {
            let cached = CachedResolution::from(orig);
            let json = serde_json::to_string(&cached).unwrap();
            let deserialized: CachedResolution = serde_json::from_str(&json).unwrap();
            let back: ResolvedRecipient = deserialized.into();
            match (orig, &back) {
                (ResolvedRecipient::Account(a), ResolvedRecipient::Account(b)) => assert_eq!(a, b),
                (ResolvedRecipient::Forward(a), ResolvedRecipient::Forward(b)) => assert_eq!(a, b),
                (ResolvedRecipient::Reject, ResolvedRecipient::Reject) => {}
                _ => panic!("mismatch: {orig:?} vs {back:?}"),
            }
        }
    }
}

// Account needs Deserialize for valkey cache
impl<'de> serde::Deserialize<'de> for Account {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            address: String,
            domain: String,
            display_name: String,
            active: bool,
            created_at: i64,
            quota_bytes: i64,
            #[serde(default)]
            recovery_email: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(Account {
            address: h.address,
            domain: h.domain,
            display_name: h.display_name,
            active: h.active,
            created_at: h.created_at,
            quota_bytes: h.quota_bytes,
            recovery_email: h.recovery_email,
        })
    }
}
