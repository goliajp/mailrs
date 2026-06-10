use std::time::Instant;

use crate::pg::BackendPool;
use dashmap::DashMap;
use serde::Serialize;

use crate::health::HealthState;

// DomainStore — split across submodules in this directory:
//   audit, totp, accounts, domains, aliases, sieve, permissions,
//   groups, apps, email_groups, encryption. Each submodule
//   attaches `impl DomainStore` blocks for its entity group;
//   mod.rs owns the struct + types + constructor + cache + the
//   pg() / kevy accessor helpers used by every handler.

mod accounts;
mod aliases;
mod apps;
mod audit;
mod domains;
mod email_groups;
mod encryption;
mod groups;
mod permissions;
mod sieve;
mod totp;
mod vacation;

/// cache entries expire after 5 minutes
pub(super) const CACHE_TTL_SECS: u64 = 300;

pub struct DomainStore {
    pub(super) pg: Option<BackendPool>,
    pub(super) kevy: Option<crate::kevy_store::KevyStore>,
    pub(super) health: HealthState,
    // process-level cache for L3 degradation
    pub(super) account_cache: DashMap<String, CachedAccount>,
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
    pub fn new(
        pg: Option<BackendPool>,
        kevy: Option<crate::kevy_store::KevyStore>,
        health: HealthState,
    ) -> Self {
        Self {
            pg,
            kevy,
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

    pub(super) fn pg(&self) -> Result<&BackendPool> {
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

    // --- apps ---

    // --- email groups ---

    // --- encryption keys (PGP / S/MIME) ---
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

// --- cached resolution for kevy ---

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

    // helper to build a DomainStore without PG/Kevy
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
        store.account_cache.insert(
            "fresh@example.com".into(),
            make_cached("fresh@example.com", "h"),
        );
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
