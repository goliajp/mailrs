//! Phase 4 of `run()`: the optional services — the LLM provider, LDAP,
//! the system-config store, and the local greylist handle.
//!
//! Lifted out of `lib.rs` verbatim on 2026-08-02.

use std::sync::Arc;

use crate::*;

pub(crate) struct Services {
    pub(crate) greylist_local_handle: greylist_local::GreylistLocalHandle,
    pub(crate) ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub(crate) llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    pub(crate) system_config_store: Arc<system_config::SystemConfigStore>,
}

pub(crate) async fn build(
    cfg: &ServerConfig,
    pg_pool: &Option<pg::BackendPool>,
    kevy_embedded_store: &Option<kevy_store::KevyStore>,
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    event_bus: &EventBus,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Services {
    let llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>> =
        if cfg.ai_analysis_enabled {
            let model_id = format!(
                "qwen3.5-9b/{}",
                mailrs_intelligence::analyze::PROMPT_VERSION
            );
            Some(Arc::new(
                mailrs_intelligence::OpenAiCompatibleProvider::new(
                    cfg.llm_url.clone(),
                    cfg.llm_api_key.clone(),
                    model_id,
                ),
            ))
        } else {
            None
        };

    // AI email analyzer (background) — uses self-hosted LLM
    if let (Some(provider), Some(mb)) = (llm_provider.as_ref(), mailbox_store.as_ref()) {
        ai_analyzer::spawn_analyzer(
            provider.clone(),
            mb.clone(),
            event_bus.clone(),
            cfg.maildir_root.clone(),
        );
    }

    // content extraction worker (OCR, PDF text)
    if let Some(pool) = pg_pool {
        content_worker::spawn_content_worker(pool.clone(), cfg.maildir_root.clone());
    }

    // LDAP authentication backend (optional)
    let ldap_config = cfg.ldap_config().map(Arc::new);
    if ldap_config.is_some() {
        tracing::info!("LDAP authentication enabled");
    }

    let system_config_store =
        init_system_config_store(cfg, pg_pool, kevy_embedded_store, shutdown_rx.clone()).await;

    // Phase 2 local greylist lists: load synchronously before WebState +
    // pipeline are wired so the very first inbound mail honors operator
    // policy. PG unavailable at boot = empty handle, same degradation
    // posture as Phase 1 sync.
    let greylist_local_handle = greylist_local::empty();
    if let Some(pool) = pg_pool {
        let started = std::time::Instant::now();
        greylist_local::reload(&greylist_local_handle, pool).await;
        let snapshot = greylist_local_handle.read().await;
        tracing::info!(
            event = "subsystem_started",
            subsystem = "greylist_local",
            white = snapshot.white_count(),
            black = snapshot.black_count(),
            total = snapshot.total(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            reload_secs = cfg.greylist_local_reload_secs,
            "greylist_local: snapshot loaded",
        );
        drop(snapshot);
        greylist_local::spawn_reload_task(
            greylist_local_handle.clone(),
            pool.clone(),
            cfg.greylist_local_reload_secs,
        );
    } else {
        tracing::info!(
            event = "subsystem_skipped",
            subsystem = "greylist_local",
            reason = "no postgres pool",
        );
    }

    Services {
        greylist_local_handle,
        ldap_config,
        llm_provider,
        system_config_store,
    }
}
