//! The anti subsystems and the antispam pipeline, all backed by the
//! shared network kevy (the receiver has no embedded kevy).

use std::sync::Arc;

use hickory_resolver::TokioResolver;
use mailrs_auth_guard::AuthGuardConfig;
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

use crate::config::ReceiverConfig;
use crate::greylist_local::GreylistLocalHandle;
use crate::greylist_sync::GreylistListsHandle;
use crate::inbound::auth_guard::AuthGuardStore;
use crate::inbound::kevy_backends::{KevyServerAuthGuardStore, KevyServerRateLimitStore};
use crate::inbound::pipeline::build_inbound_pipeline;
use crate::inbound::rate_limit::{RateLimitStore, TokenBucketConfig};
use crate::inbound::stages::mail_auth::MailAuthResolvers;
use crate::kevy_net::KevyNetClient;

pub(super) fn rate_limiter(
    cfg: &ReceiverConfig,
    kevy_client: &Arc<KevyNetClient>,
) -> Arc<dyn RateLimitStore> {
    Arc::new(KevyServerRateLimitStore::new(
        kevy_client.clone(),
        TokenBucketConfig {
            capacity: cfg.rate_limit_capacity,
            refill_rate: cfg.rate_limit_refill,
        },
    ))
}

pub(super) fn auth_guard(kevy_client: &Arc<KevyNetClient>) -> Arc<dyn AuthGuardStore> {
    Arc::new(KevyServerAuthGuardStore::new(
        kevy_client.clone(),
        AuthGuardConfig::default(),
    ))
}

/// DNS resolver for PTR + SPF/DKIM/DMARC.
pub(super) fn dns_resolver() -> Option<Arc<TokioResolver>> {
    hickory_resolver::TokioResolver::builder_tokio()
        .ok()
        .and_then(|mut b| {
            b.options_mut().cache_size = 4096;
            b.build().ok()
        })
        .map(Arc::new)
}

pub(super) fn mail_auth_resolvers(
    cfg: &ReceiverConfig,
    resolver: Option<&Arc<TokioResolver>>,
) -> Option<MailAuthResolvers> {
    if cfg.antispam_enabled {
        resolver.map(|r| {
            let dkim = Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone()));
            MailAuthResolvers {
                spf: Arc::new(mailrs_spf::HickoryResolver::new((**r).clone())),
                dkim: dkim.clone(),
                arc: dkim,
                dmarc: r.clone(),
            }
        })
    } else {
        None
    }
}

pub(super) fn pipeline(
    cfg: &ReceiverConfig,
    kevy_client: &Arc<KevyNetClient>,
    (greylist_db, greylist_lists): (Option<Arc<GreylistDb>>, GreylistListsHandle),
    greylist_local: GreylistLocalHandle,
    resolver: Option<Arc<TokioResolver>>,
    mail_auth_resolvers: Option<MailAuthResolvers>,
) -> mailrs_inbound::Pipeline {
    build_inbound_pipeline(
        greylist_db,
        GreylistConfig {
            initial_delay_secs: cfg.greylist_delay_secs,
            ..Default::default()
        },
        greylist_lists,
        greylist_local,
        Some(kevy_client.clone()), // greylist_contacts
        Some(kevy_client.clone()), // quota_client
        Some(kevy_client.clone()), // bayes_client — v2.8.1
        resolver,
        mail_auth_resolvers,
        // Record per-message DMARC results into the shared network
        // kevy so admin tooling (and a future aggregate reporter) can
        // consume them. Errors are swallowed inside the sink.
        Some(
            Arc::new(crate::kevy_dmarc::KevyDmarcSink::new(&cfg.kevy_url))
                as Arc<dyn crate::inbound::stages::mail_auth::DmarcReportSink>,
        ),
        cfg.clamav_addr.clone(),
        None, // LLM scoring is the core's post-delivery job
        None, // spam cache is core-side
        cfg.spam_score_threshold,
    )
}
