//! Web sessions: the in-memory map, its kevy mirror, and the sweep
//! that expires both.

use super::*;

/// session token TTL: 7 days
pub(crate) const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// kevy key prefix for individual session blobs (`session:<token>`).
pub(crate) const KEVY_SESSION_PREFIX: &[u8] = b"session:";

/// kevy SET key holding every live session token, used to enumerate
/// sessions at boot for [`SessionStore::restore_from_kevy`].
pub(crate) const KEVY_SESSION_INDEX: &[u8] = b"session:index";

pub(crate) struct SessionInfo {
    pub(crate) address: String,
    pub(crate) display_name: String,
    pub(crate) permissions: Arc<crate::permission::EffectivePermissions>,
    /// Unix epoch seconds at session creation; replaces a previous
    /// `created_at: Instant` so the value round-trips through kevy.
    pub(crate) created_at_unix: u64,
}

/// Wire form for kevy persistence — same as [`SessionInfo`] but with the
/// permissions Arc unwrapped so serde-json can borrow the inner value.
#[derive(Serialize, Deserialize)]
struct SessionInfoWire {
    address: String,
    display_name: String,
    permissions: crate::permission::EffectivePermissions,
    created_at_unix: u64,
}

impl From<&SessionInfo> for SessionInfoWire {
    fn from(s: &SessionInfo) -> Self {
        Self {
            address: s.address.clone(),
            display_name: s.display_name.clone(),
            permissions: (*s.permissions).clone(),
            created_at_unix: s.created_at_unix,
        }
    }
}

impl From<SessionInfoWire> for SessionInfo {
    fn from(w: SessionInfoWire) -> Self {
        Self {
            address: w.address,
            display_name: w.display_name,
            permissions: Arc::new(w.permissions),
            created_at_unix: w.created_at_unix,
        }
    }
}

/// In-memory DashMap of live sessions, optionally mirrored to a kevy
/// store so they survive a `mailrs-server` restart (the
/// mem-watchdog graceful restart or a deploy).
///
/// All write paths are best-effort against kevy — if the mirror fails
/// the DashMap copy still works; sessions are not the source of truth
/// for anything else. On boot, [`Self::restore_from_kevy`] repopulates
/// the cache from the persisted side.
pub(crate) struct SessionStore {
    cache: DashMap<String, SessionInfo>,
    kevy: ArcSwapOption<crate::kevy_store::KevyStore>,
}

impl SessionStore {
    pub(crate) fn new() -> Self {
        Self {
            cache: DashMap::new(),
            kevy: ArcSwapOption::from(None),
        }
    }

    /// Attach the kevy handle once it's been opened by bootstrap.
    /// Subsequent inserts/removes will mirror through; existing entries
    /// are not retroactively pushed.
    pub(crate) fn attach_kevy(&self, kevy: crate::kevy_store::KevyStore) {
        self.kevy.store(Some(Arc::new(kevy)));
    }

    fn kevy(&self) -> Option<crate::kevy_store::KevyStore> {
        self.kevy.load_full().map(|arc| (*arc).clone())
    }

    pub(crate) fn insert(&self, token: String, info: SessionInfo) {
        if let Some(kevy) = self.kevy()
            && let Ok(bytes) = serde_json::to_vec(&SessionInfoWire::from(&info))
        {
            // NB: kevy 3.17 AtomicCtx exposes set() without a TTL
            // variant, so we cannot land set_with_ttl + sadd inside
            // one atomic closure. Kept as two Store calls — token is
            // a fresh unique random value with no concurrent producer,
            // so there is no real race here; a crash between the two
            // orphans a key that TTL will reap after 7d anyway.
            let key: Vec<u8> = [KEVY_SESSION_PREFIX, token.as_bytes()].concat();
            let _ = kevy.set_with_ttl(&key, &bytes, SESSION_TTL);
            let _ = kevy.sadd(KEVY_SESSION_INDEX, &[token.as_bytes()]);
        }
        self.cache.insert(token, info);
    }

    pub(crate) fn get(
        &self,
        token: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, SessionInfo>> {
        self.cache.get(token)
    }

    pub(crate) fn remove(&self, token: &str) {
        if let Some(kevy) = self.kevy() {
            // v2 Stage B.1: del value + srem index in one atomic closure.
            let key: Vec<u8> = [KEVY_SESSION_PREFIX, token.as_bytes()].concat();
            let _ = kevy.atomic(|ctx| {
                ctx.del(&[&key]);
                ctx.srem(KEVY_SESSION_INDEX, &[token.as_bytes()])?;
                Ok(())
            });
        }
        self.cache.remove(token);
    }

    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    /// Sweep expired entries from the cache AND mirror the removal back
    /// to kevy so the index doesn't accumulate stale tokens.
    pub(crate) fn retain_unexpired(&self, ttl_secs: u64) {
        let kevy = self.kevy();
        let now = crate::inbound::auth_guard::unix_now();
        self.cache.retain(|token, info| {
            let keep = now.saturating_sub(info.created_at_unix) < ttl_secs;
            if !keep && let Some(ref k) = kevy {
                // v2 Stage B.1: del + srem inside one atomic closure.
                let key: Vec<u8> = [KEVY_SESSION_PREFIX, token.as_bytes()].concat();
                let _ = k.atomic(|ctx| {
                    ctx.del(&[&key]);
                    ctx.srem(KEVY_SESSION_INDEX, &[token.as_bytes()])?;
                    Ok(())
                });
            }
            keep
        });
    }

    /// Rehydrate the DashMap from kevy at boot. Expired-on-disk entries
    /// (TTL already evicted the value but the index entry lingers) are
    /// cleaned up here. Returns the number of restored sessions for the
    /// startup log line.
    pub(crate) fn restore_from_kevy(&self) -> usize {
        let Some(kevy) = self.kevy() else {
            return 0;
        };
        let Ok(tokens) = kevy.smembers(KEVY_SESSION_INDEX) else {
            return 0;
        };
        let now = crate::inbound::auth_guard::unix_now();
        let ttl_secs = SESSION_TTL.as_secs();
        let mut restored = 0usize;
        for tok_bytes in tokens {
            let Ok(token) = String::from_utf8(tok_bytes.clone()) else {
                continue;
            };
            let key: Vec<u8> = [KEVY_SESSION_PREFIX, token.as_bytes()].concat();
            match kevy.get(&key) {
                Ok(Some(bytes)) => match serde_json::from_slice::<SessionInfoWire>(&bytes) {
                    Ok(wire) if now.saturating_sub(wire.created_at_unix) < ttl_secs => {
                        self.cache.insert(token, SessionInfo::from(wire));
                        restored += 1;
                    }
                    _ => {
                        // v2 Stage B.1: cleanup del + srem atomic.
                        let _ = kevy.atomic(|ctx| {
                            ctx.del(&[&key]);
                            ctx.srem(KEVY_SESSION_INDEX, &[&tok_bytes])?;
                            Ok(())
                        });
                    }
                },
                Ok(None) => {
                    let _ = kevy.srem(KEVY_SESSION_INDEX, &[&tok_bytes]);
                }
                Err(_) => {}
            }
        }
        restored
    }
}

/// spawn a background task to clean up expired sessions and stale rate-limit buckets every hour
pub fn spawn_session_cleanup(state: Arc<WebState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state.sessions.retain_unexpired(SESSION_TTL.as_secs());
            // purge rate-limit buckets not seen in the last hour
            let stale_before_unix_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .saturating_sub(3600);
            state.web_rate_limiter.cleanup(stale_before_unix_secs).await;
            // clean up expired OIDC auth codes and refresh tokens
            if let Some(ref pool) = state.pg_pool {
                let _ = crate::oidc_store::cleanup_expired_codes(pool).await;
                let _ = crate::oidc_store::cleanup_expired_refresh_tokens(pool).await;
            }
            // clean up audit log entries older than 90 days
            if let Some(ref ds) = state.domain_store {
                ds.cleanup_audit_log(90).await;
            }
        }
    });
}
