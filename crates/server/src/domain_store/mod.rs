use std::time::Instant;

use crate::pg::BackendPool;
use dashmap::DashMap;
use serde::Serialize;

// DomainStore — split across submodules in this directory:
//   accounts, domains, aliases, permissions, recipients. Each submodule
//   attaches `impl DomainStore` blocks for its entity group;
//   mod.rs owns the struct + types + constructor + cache + the
//   pg() / kevy accessor helpers used by every handler.

mod accounts;
mod aliases;
mod domains;
mod permissions;
pub(crate) mod recipients;

/// cache entries expire after 5 minutes
pub(super) const CACHE_TTL_SECS: u64 = 300;

pub struct DomainStore {
    pub(super) pg: Option<BackendPool>,
    pub(super) kevy: Option<crate::kevy_store::KevyStore>,
    // process-level cache for L3 degradation
    pub(super) account_cache: DashMap<String, CachedAccount>,
    /// Optional backend-agnostic alias table. When set, recipient routing
    /// and the alias CRUD methods use it instead of the SQL table.
    pub(super) alias_store: Option<std::sync::Arc<dyn mailrs_alias_store::AliasStore>>,
}

#[derive(Clone)]
pub(super) struct CachedAccount {
    pub(super) account: Account,
    pub(super) password_hash: String,
    pub(super) cached_at: Instant,
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

pub(super) type Result<T> = std::result::Result<T, StoreError>;

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
    pub fn new(pg: Option<BackendPool>, kevy: Option<crate::kevy_store::KevyStore>) -> Self {
        Self {
            pg,
            kevy,
            account_cache: DashMap::new(),
            alias_store: None,
        }
    }

    /// Attach a backend-agnostic `AliasStore`. When set, alias reads and
    /// writes go through this trait — a `NetworkKevyAliasStore` here makes
    /// this core and fastcore share one alias source of truth.
    pub fn with_alias_store(
        mut self,
        alias_store: std::sync::Arc<dyn mailrs_alias_store::AliasStore>,
    ) -> Self {
        self.alias_store = Some(alias_store);
        self
    }

    pub(super) fn pg(&self) -> Result<&BackendPool> {
        self.pg.as_ref().ok_or(StoreError::Unavailable)
    }

    // --- Kevy cache helpers (in-process embed store) ---

    fn kevy_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let store = self.kevy.as_ref()?;
        let bytes = store.get(key.as_bytes()).ok()??;
        let s = String::from_utf8(bytes).ok()?;
        serde_json::from_str(&s).ok()
    }

    fn kevy_set(&self, key: &str, val: &impl serde::Serialize, ttl_secs: u64) {
        if let Some(ref store) = self.kevy
            && let Ok(json) = serde_json::to_string(val)
        {
            let _ = store.set_with_ttl(
                key.as_bytes(),
                json.as_bytes(),
                std::time::Duration::from_secs(ttl_secs),
            );
        }
    }

    fn kevy_del(&self, key: &str) {
        if let Some(ref store) = self.kevy {
            let _ = store.del(&[key.as_bytes()]);
        }
    }

    // --- domains ---

    // --- accounts ---

    // --- sieve scripts ---

    // --- aliases ---

    // --- RBAC: groups & permissions ---
}

// serde for CachedAccount (kevy)
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

// Account needs Deserialize for kevy cache
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

#[cfg(test)]
mod tests {
    use super::*;

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

    // helper to build a DomainStore without PG/Kevy
    fn make_store() -> DomainStore {
        DomainStore::new(None, None)
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
}
