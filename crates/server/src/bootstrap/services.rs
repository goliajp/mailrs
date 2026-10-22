//! The optional services: the LLM analyzer, the content extraction worker,
//! and LDAP.

use std::sync::Arc;

use crate::*;

/// Start the background services and return the LDAP backend, if configured.
pub(crate) fn build(
    cfg: &ServerConfig,
    pg_pool: &Option<pg::BackendPool>,
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    event_bus: &EventBus,
) -> Option<Arc<crate::ldap_auth::LdapConfig>> {
    let llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>> =
        if cfg.ai_analysis_enabled {
            // there is no model to fall back to: the operator names one
            assert!(
                !cfg.llm_url.is_empty(),
                "MAILRS_LLM_URL is required when MAILRS_AI_ANALYSIS_ENABLED is on"
            );
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

    ldap_config
}
