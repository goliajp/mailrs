//! Phase 3 of `run()`: the Postgres-backed stores, plus the two
//! backfills that run once at boot behind them.
//!
//! Lifted out of `lib.rs` verbatim on 2026-08-02.

use std::sync::Arc;

use crate::*;

pub(crate) struct Stores {
    pub(crate) alias_store: Option<Arc<dyn mailrs_alias_store::AliasStore>>,
    pub(crate) dmarc_report_store: Option<Arc<dmarc_report::DmarcReportStore>>,
    pub(crate) domain_store: Option<Arc<domain_store::DomainStore>>,
    pub(crate) mailbox_store: Option<Arc<PgMailboxStore>>,
}

pub(crate) async fn build(
    cfg: &ServerConfig,
    pg_pool: &Option<pg::BackendPool>,
    kevy_embedded_store: &Option<kevy_store::KevyStore>,
    health_state: &health::HealthState,
) -> Stores {
    // mailbox store for IMAP (PG-backed)
    let mailbox_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(PgMailboxStore::new(pool.clone())));

    // AliasStore backend selector — RFC 20260705 Step 3. Constructed
    // ahead of DomainStore so it can be attached to both the store's
    // internal alias path AND the WebState top-level field, keeping
    // both sides on the same Arc. Env `MAILRS_ALIAS_STORE_BACKEND`:
    // `network` + `MAILRS_KEVY_URL` = shared network kevy (v2 dual-mode
    // sync); anything else = None → legacy PG-backed DomainStore.aliases.
    let alias_store: Option<Arc<dyn mailrs_alias_store::AliasStore>> = match std::env::var(
        "MAILRS_ALIAS_STORE_BACKEND",
    )
    .as_deref()
    {
        Ok("network") => match std::env::var("MAILRS_KEVY_URL") {
            Ok(url) if !url.is_empty() => {
                tracing::info!(
                    url = %url,
                    "alias-store backend = network kevy (monolith, RFC 20260705 Step 3)"
                );
                Some(Arc::new(
                    mailrs_alias_store_net::NetworkKevyAliasStore::new(url),
                ))
            }
            _ => {
                tracing::warn!(
                    "MAILRS_ALIAS_STORE_BACKEND=network but MAILRS_KEVY_URL is unset — falling back to PG-backed DomainStore.aliases"
                );
                None
            }
        },
        _ => {
            tracing::info!("alias-store backend = PG (DomainStore.aliases, default)");
            None
        }
    };

    // domain store (PG + Kevy + process cache); attach the alias_store
    // seam so its `resolve_recipient` alias steps + CRUD go through the
    // trait when a network backend is configured.
    let domain_store = if pg_pool.is_some() {
        let mut ds = domain_store::DomainStore::new(
            pg_pool.clone(),
            kevy_embedded_store.clone(),
            health_state.clone(),
        );
        if let Some(ref store) = alias_store {
            ds = ds.with_alias_store(store.clone());
        }
        let ds = Arc::new(ds);
        ds.preload_accounts().await;
        tracing::info!("domain store ready (PG-backed)");
        Some(ds)
    } else {
        None
    };

    // OIDC provider: ensure signing key exists
    if let Some(pool) = pg_pool
        && let Err(e) = oidc_jwt::ensure_signing_key(pool).await
    {
        tracing::warn!(error = %e, "failed to ensure oidc signing key");
    }

    // DMARC report store (PG-backed)
    let dmarc_report_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(dmarc_report::DmarcReportStore::new(pool.clone())));

    // backfill threading data for existing messages
    if let Some(ref mb) = mailbox_store {
        let maildir = cfg.maildir_root.clone();
        let count = mb.backfill_threading(&maildir).await;
        if count > 0 {
            tracing::info!(event = "threading_backfill_complete", count);
        }
    }

    // periodic maildir reconcile (S2.2): the "never lose a message"
    // backstop for the notification-driven post-delivery path.
    if let Some(ref mb) = mailbox_store {
        reconcile_task::spawn_periodic_reconcile(mb.clone(), cfg.maildir_root.clone());
    }

    Stores {
        alias_store,
        dmarc_report_store,
        domain_store,
        mailbox_store,
    }
}
