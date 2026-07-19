#![allow(unused_imports)]
//! TLS state initialization (ACME / manual cert / disabled).

use std::sync::Arc;

use crate::config;
use crate::config::TlsMode;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, smtp_session, system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

/// Initialize TLS state per config:
///
/// - ACME (Let's Encrypt): issues + renews certs, spawns the
///   HTTP-01 challenge responder on `:80`.
/// - Manual: loads the cert + key paths from disk.
/// - None: TLS disabled (STARTTLS unavailable too).
pub(crate) async fn init_tls_state(
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Option<tls::TlsState> {
    match cfg.tls_mode() {
        TlsMode::Acme => {
            // ACME path is best-effort at startup: if anything fails (missing
            // email/domains, LE rate-limit, network timeout, challenge port
            // conflict) we log loudly and start without TLS rather than crash
            // the whole server. Manual cert is already preferred by tls_mode()
            // so when ACME is selected there's no manual fallback to try.
            // v1.7.99 incident: LE 429 during the 13h hostname-drift window
            // panicked startup repeatedly until ACME was disabled manually.
            let Some(email) = cfg.acme_email.as_ref() else {
                tracing::warn!(
                    "tls_mode = Acme but MAILRS_ACME_EMAIL not set; starting without TLS"
                );
                return None;
            };
            let domains = &cfg.acme_domains;
            if domains.is_empty() {
                tracing::warn!(
                    "MAILRS_ACME_EMAIL is set but MAILRS_ACME_DOMAINS is empty; starting without TLS"
                );
                return None;
            }

            // challenge tokens shared between ACME init and challenge server
            let challenge_tokens: acme::ChallengeTokens = Default::default();
            use std::net::SocketAddr;
            let challenge_addr: SocketAddr = ([0, 0, 0, 0], 80).into();
            acme::spawn_challenge_server(
                challenge_tokens.clone(),
                challenge_addr,
                shutdown_rx.clone(),
            );

            let (tls, account) = match acme::init(
                email,
                domains,
                &cfg.acme_dir,
                cfg.acme_staging,
                &challenge_tokens,
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "ACME init failed; starting without TLS — check MAILRS_ACME_EMAIL/DOMAINS \
                         and Let's Encrypt reachability/rate-limit"
                    );
                    return None;
                }
            };

            acme::spawn_renewal_task(
                account,
                challenge_tokens,
                tls.clone(),
                acme::RenewalConfig {
                    domains: domains.clone(),
                    acme_dir: cfg.acme_dir.clone(),
                    ..Default::default()
                },
                shutdown_rx,
            );

            Some(tls)
        }
        TlsMode::Manual => {
            let tls_config = tls::load_tls_config(
                cfg.tls_cert
                    .as_ref()
                    .expect("MAILRS_TLS_CERT must be set when TLS mode is Manual"),
                cfg.tls_key
                    .as_ref()
                    .expect("MAILRS_TLS_KEY must be set when TLS mode is Manual"),
            )
            .expect("failed to load TLS certificate and key files");
            Some(tls::TlsState::new(
                std::sync::Arc::try_unwrap(tls_config).unwrap_or_else(|arc| (*arc).clone()),
            ))
        }
        TlsMode::None => None,
    }
}
