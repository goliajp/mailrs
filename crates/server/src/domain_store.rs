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
    pub super_domains: String,
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

#[derive(Debug, Clone)]
pub enum ResolvedRecipient {
    Account(String),
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

    /// preload all accounts into process cache for L3 degradation
    pub async fn preload_accounts(&self) {
        let Ok(pool) = self.pg() else { return };
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
            "SELECT address, domain, display_name, active, \
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, super_domains \
             FROM accounts",
        )
        .fetch_all(pool)
        .await;

        if let Ok(rows) = rows {
            for (
                address,
                domain,
                display_name,
                active,
                created_at,
                quota_bytes,
                password_hash,
                super_domains,
            ) in rows
            {
                let account = Account {
                    address: address.clone(),
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    super_domains,
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
        if let Some(mut conn) = self.valkey.clone() {
            if let Ok(json) = serde_json::to_string(val) {
                let _: std::result::Result<(), _> = redis::cmd("SET")
                    .arg(key)
                    .arg(&json)
                    .arg("EX")
                    .arg(ttl_secs)
                    .query_async(&mut conn)
                    .await;
            }
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
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, super_domains \
             FROM accounts ORDER BY address",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    address,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    super_domains,
                )| Account {
                    address,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    super_domains,
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
        let (active, created_epoch, quota_bytes): (bool, i64, i64) = sqlx::query_as(
            "INSERT INTO accounts (address, domain, display_name, password_hash) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (address) DO UPDATE SET \
             domain = EXCLUDED.domain, display_name = EXCLUDED.display_name, \
             password_hash = EXCLUDED.password_hash \
             RETURNING active, EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes",
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
            super_domains: String::new(),
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

    pub async fn get_account_with_hash(&self, address: &str) -> Result<Option<(Account, String)>> {
        // try valkey cache
        let cache_key = format!("acct:{address}");
        if let Some(cached) = self.valkey_get::<CachedAccount>(&cache_key).await {
            return Ok(Some((cached.account, cached.password_hash)));
        }

        // try PG
        if let Ok(pool) = self.pg() {
            let row =
                sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
                    "SELECT address, domain, display_name, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, super_domains \
                 FROM accounts WHERE address = $1",
                )
                .bind(address)
                .fetch_optional(pool)
                .await?;

            if let Some((
                addr,
                domain,
                display_name,
                active,
                created_at,
                quota_bytes,
                hash,
                super_domains,
            )) = row
            {
                let account = Account {
                    address: addr,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    super_domains,
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

        // 2. exact alias match
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
            super_domains: String::new(),
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
    fn account_deserialize_with_super_domains_default() {
        // super_domains is marked #[serde(default)] in the Deserialize impl
        let json = r#"{
            "address": "nosuper@example.com",
            "domain": "example.com",
            "display_name": "No Super",
            "active": true,
            "created_at": 1700000000,
            "quota_bytes": 0
        }"#;
        let account: Account = serde_json::from_str(json).unwrap();
        assert_eq!(account.super_domains, "");
    }

    #[test]
    fn account_deserialize_with_super_domains_present() {
        let json = r#"{
            "address": "admin@example.com",
            "domain": "example.com",
            "display_name": "Admin",
            "active": true,
            "created_at": 1700000000,
            "quota_bytes": 1073741824,
            "super_domains": "example.com,other.com"
        }"#;
        let account: Account = serde_json::from_str(json).unwrap();
        assert_eq!(account.super_domains, "example.com,other.com");
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
            super_domains: "a.com".into(),
        };
        let json = serde_json::to_string(&account).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["address"], "full@example.com");
        assert_eq!(parsed["domain"], "example.com");
        assert_eq!(parsed["display_name"], "Full Account");
        assert_eq!(parsed["active"], false);
        assert_eq!(parsed["created_at"], 1234567890);
        assert_eq!(parsed["quota_bytes"], 500_000_000);
        assert_eq!(parsed["super_domains"], "a.com");
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
        assert_eq!(deserialized.super_domains, original.super_domains);
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
            super_domains: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(Account {
            address: h.address,
            domain: h.domain,
            display_name: h.display_name,
            active: h.active,
            created_at: h.created_at,
            quota_bytes: h.quota_bytes,
            super_domains: h.super_domains,
        })
    }
}
