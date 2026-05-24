#![allow(unused_imports)]
//! Build the inbound pipeline + 4 shadow-mode resolvers (SPF/DKIM/ARC/DMARC).

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, render_preview, search_index, smtp_session,
    system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

/// Build the inbound pipeline + the four shadow-mode resolvers
/// (SPF / DKIM / ARC / DMARC) used to validate our in-house
/// `mailrs-*` crates against `mail-auth` on real prod traffic.
/// Each shadow runs in parallel; comparison happens inside
/// `MailAuthStage`. NO impact on production decisions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_inbound_pipeline_with_shadows(
    greylist_db: &Option<Arc<mailrs_shield::greylist::GreylistDb>>,
    greylist_config: &mailrs_shield::greylist::GreylistConfig,
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    mail_authenticator: &Option<Arc<mail_auth::MessageAuthenticator>>,
    dmarc_report_store: &Option<Arc<dmarc_report::DmarcReportStore>>,
    cfg: &config::ServerConfig,
    llm_provider: &Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    valkey_conn: &Option<redis::aio::ConnectionManager>,
) -> mailrs_inbound::Pipeline {
    let shadow_spf_resolver = resolver
        .as_ref()
        .map(|r| Arc::new(mailrs_spf::HickoryResolver::new((**r).clone())));
    let shadow_dkim_resolver = resolver
        .as_ref()
        .map(|r| Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone())));
    // ARC reuses the DKIM resolver (ArcResolver = DkimResolver).
    // One hickory bind, two stones.
    let shadow_arc_resolver = shadow_dkim_resolver.clone();
    // DMARC's shadow path needs raw TXT lookup against the
    // hickory TokioResolver (mailrs-dmarc itself is a pure
    // evaluator with no DNS). Same resolver mail-auth uses.
    let shadow_dmarc_resolver = resolver.clone();

    crate::inbound::pipeline::build_inbound_pipeline(
        greylist_db.clone(),
        greylist_config.clone(),
        resolver.clone(),
        mail_authenticator.clone(),
        dmarc_report_store.clone(),
        shadow_spf_resolver,
        shadow_dkim_resolver,
        shadow_arc_resolver,
        shadow_dmarc_resolver,
        cfg.clamav_addr.clone(),
        llm_provider.clone(),
        valkey_conn.clone(),
        cfg.spam_score_threshold,
    )
}
