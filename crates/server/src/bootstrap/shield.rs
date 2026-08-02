//! Phase 2 of `run()`: the DNS resolver and the inbound gates that
//! depend on it — the PTR check, greylisting, and the auth guard.
//!
//! Lifted out of `lib.rs` verbatim on 2026-08-02.

use std::sync::Arc;

use super::*;
use crate::inbound::auth_guard::AuthGuardStore;
use crate::*;

pub(crate) struct Shield {
    pub(crate) auth_guard: Arc<dyn AuthGuardStore>,
    pub(crate) greylist_config: GreylistConfig,
    pub(crate) greylist_db: Option<Arc<GreylistDb>>,
    pub(crate) resolver: Option<Arc<TokioResolver>>,
}

pub(crate) async fn build(
    cfg: &ServerConfig,
    pg_pool: &Option<pg::BackendPool>,
    kevy_embedded_store: &Option<kevy_store::KevyStore>,
    kevy_net_client: &Option<Arc<kevy_net::KevyNetClient>>,
) -> Shield {
    // DNS resolver for DNSBL and other lookups. Bumped cache from
    // hickory's default 32 entries to 4096 — at modest steady-state
    // mail traffic, 32 entries holds maybe a minute of unique
    // lookups; SPF/DKIM/DMARC queries hammer the same sender domain
    // up to four ways and benefit hugely from staying in cache. The
    // working set is bounded by unique sender domains × policy
    // record types (~4) which for any realistic load fits in 4096.
    let resolver = TokioResolver::builder_tokio()
        .ok()
        .and_then(|mut b| {
            b.options_mut().cache_size = 4096;
            b.build().ok()
        })
        .map(Arc::new);

    // PTR record check
    if let Some(ref r) = resolver {
        mailrs_shield::ptr::check_ptr_record(r, &cfg.hostname).await;
    }

    // greylisting (in-process kevy only; kevy AOF is durable).
    //
    // Until v1.7.108 we wired GreylistDb.with_pg(pool) so every hot-path
    // check mirrored to the PG `greylist_triplets` table — a belt-and-
    // suspenders durability hedge from the pre-AOF era. kevy-embedded
    // 1.1.6 ships forward-compat AOF persistence, so the mirror is no
    // longer earning its cost (one PG INSERT per inbound check is
    // measurable at SMTP-peak load).
    //
    // We still want the historical reputation in PG — months of
    // legitimate-sender first_seen timestamps — so on startup we
    // backfill the table into kevy once (idempotent via a sentinel
    // key). After that the hot path is pure kevy and the PG table
    // is read-only / archival.
    let greylist_config = GreylistConfig {
        initial_delay_secs: cfg.greylist_delay_secs,
        ..Default::default()
    };

    // Backfill PG reputation into the embedded store only when greylist
    // actually reads that store. In network mode greylist reads the
    // shared kevy-server, so warming the embedded store would be wasted.
    if kevy_net_client.is_none()
        && let (Some(store), Some(pool)) = (kevy_embedded_store.as_ref(), pg_pool.as_ref())
    {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        greylist_backfill::backfill_from_pg_best_effort(
            pool,
            store.as_ref(),
            greylist_config.pass_ttl_secs,
            now_secs,
        )
        .await;
    }

    // greylist reads the shared network kevy-server when MAILRS_KEVY_URL
    // is set, else the in-process embedded store.
    let greylist_db = match kevy_net_client.as_ref() {
        Some(client) => Some(Arc::new(GreylistDb::with_backend(Arc::new(
            crate::inbound::kevy_backends::KevyServerGreylistBackend::new(client.clone()),
        )))),
        None => kevy_embedded_store
            .as_ref()
            .map(|store| Arc::new(GreylistDb::new(store.as_ref().clone()))),
    };

    // shared kevy-server → distributed lockout shared across the fleet;
    // else the in-process AuthGuard (with its periodic cleanup task).
    let auth_guard: Arc<dyn crate::inbound::auth_guard::AuthGuardStore> =
        match kevy_net_client.as_ref() {
            Some(client) => Arc::new(
                crate::inbound::kevy_backends::KevyServerAuthGuardStore::new(
                    client.clone(),
                    auth_guard_config(cfg),
                ),
            ),
            None => init_auth_guard(cfg),
        };

    Shield {
        auth_guard,
        greylist_config,
        greylist_db,
        resolver,
    }
}
