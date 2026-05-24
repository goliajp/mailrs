use std::sync::Arc;

use mail_auth::MessageAuthenticator;

// Core types + RFC 8601 helpers come from the published mailrs-inbound crate.
// Re-exported here so existing in-crate callers can keep using
// `crate::inbound::pipeline::DeliveryDecision` etc.
pub use mailrs_inbound::{
    AuthResult, AuthResults, DeliveryDecision, DmarcPolicy, PipelineInput, build_auth_header,
    format_auth_results_header, make_delivery_decision,
};

use super::stages::{
    AiScoringStage, ClamavStage, ContentScanStage, GreylistStage, MailAuthStage, PtrStage,
};
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

use hickory_resolver::TokioResolver;

use crate::dmarc_report::DmarcReportStore;

/// Build the inbound `mailrs_inbound::Pipeline` from the optional backends
/// configured at server startup. Each backend, when present, contributes its
/// corresponding `Stage` to the pipeline in fixed evaluation order:
/// `greylist → ptr → mail_auth → clamav → content_scan → ai_scoring`.
///
/// `content_scan` always runs (no external dependency); the others are
/// gated on `Some(_)` of the matching backend.
#[allow(clippy::too_many_arguments)]
pub fn build_inbound_pipeline(
    greylist_db: Option<Arc<GreylistDb>>,
    greylist_config: GreylistConfig,
    resolver: Option<Arc<TokioResolver>>,
    mail_authenticator: Option<Arc<MessageAuthenticator>>,
    dmarc_report_store: Option<Arc<DmarcReportStore>>,
    shadow_spf_resolver: Option<Arc<mailrs_spf::HickoryResolver>>,
    shadow_dkim_resolver: Option<Arc<mailrs_dkim::HickoryDkimResolver>>,
    shadow_arc_resolver: Option<Arc<mailrs_dkim::HickoryDkimResolver>>,
    shadow_dmarc_resolver: Option<Arc<TokioResolver>>,
    clamav_addr: Option<String>,
    llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    valkey: Option<redis::aio::ConnectionManager>,
    spam_score_threshold: f64,
) -> mailrs_inbound::Pipeline {
    let mut builder = mailrs_inbound::Pipeline::builder().spam_threshold(spam_score_threshold);

    if let Some(db) = greylist_db {
        builder = builder.add(GreylistStage::new(db, greylist_config));
    }
    if let Some(r) = resolver {
        builder = builder.add(PtrStage::new(r));
    }
    if let Some(auth) = mail_authenticator {
        let mut stage = MailAuthStage::new(auth, dmarc_report_store);
        if let Some(shadow) = shadow_spf_resolver {
            stage = stage.with_shadow_spf(shadow);
        }
        if let Some(shadow) = shadow_dkim_resolver {
            stage = stage.with_shadow_dkim(shadow);
        }
        if let Some(shadow) = shadow_arc_resolver {
            stage = stage.with_shadow_arc(shadow);
        }
        if let Some(shadow) = shadow_dmarc_resolver {
            stage = stage.with_shadow_dmarc(shadow);
        }
        builder = builder.add(stage);
    }
    if let Some(addr) = clamav_addr {
        builder = builder.add(ClamavStage::new(addr));
    }
    builder = builder.add(ContentScanStage::new());
    if let Some(provider) = llm_provider {
        builder = builder.add(AiScoringStage::new(provider, valkey, spam_score_threshold));
    }

    builder.build()
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
