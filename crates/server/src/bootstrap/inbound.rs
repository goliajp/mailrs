//! Build the inbound pipeline + the four DNS resolvers driving the
//! mailrs-* auth crates (SPF / DKIM / ARC / DMARC).

use std::sync::Arc;

use crate::config;
use crate::dmarc_report;
use crate::inbound::stages::mail_auth::MailAuthResolvers;

/// Build the inbound pipeline. When `cfg.antispam_enabled`, wrap the
/// shared hickory resolver in the four shapes each mailrs-* auth crate
/// expects and pass them through to the [`MailAuthStage`]; otherwise the
/// mail-auth stage is skipped entirely (deployment-side antispam=false
/// = "trust upstream MX, do no auth checks here").
///
/// [`MailAuthStage`]: crate::inbound::stages::MailAuthStage
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_inbound_pipeline_with_shadows(
    greylist_db: &Option<Arc<mailrs_shield::greylist::GreylistDb>>,
    greylist_config: &mailrs_shield::greylist::GreylistConfig,
    greylist_whitelist: &crate::greylist_sync::GreylistListsHandle,
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    dmarc_report_store: &Option<Arc<dmarc_report::DmarcReportStore>>,
    cfg: &config::ServerConfig,
    llm_provider: &Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    kevy_embed: &Option<crate::kevy_store::KevyStore>,
) -> mailrs_inbound::Pipeline {
    let mail_auth_resolvers = if cfg.antispam_enabled {
        resolver.as_ref().map(|r| {
            let dkim = Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone()));
            MailAuthResolvers {
                spf: Arc::new(mailrs_spf::HickoryResolver::new((**r).clone())),
                // ARC reuses the DKIM resolver shape.
                dkim: dkim.clone(),
                arc: dkim,
                dmarc: r.clone(),
            }
        })
    } else {
        None
    };

    crate::inbound::pipeline::build_inbound_pipeline(
        greylist_db.clone(),
        greylist_config.clone(),
        greylist_whitelist.clone(),
        resolver.clone(),
        mail_auth_resolvers,
        dmarc_report_store.clone(),
        cfg.clamav_addr.clone(),
        llm_provider.clone(),
        kevy_embed.clone(),
        cfg.spam_score_threshold,
    )
}
