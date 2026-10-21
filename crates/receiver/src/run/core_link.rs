//! What the receiver takes from outside its own process: TLS
//! certificates, the users file, and the core's credential, recipient
//! and local-domain answers.

use std::sync::Arc;

use mailrs_core::users::UserStore;

use crate::config::ReceiverConfig;
use crate::greylist_local::GreylistLocalHandle;
use crate::kevy_net::KevyNetClient;

/// manual TLS only (ACME stays core-side for phase 1); plain SMTP if unset.
/// The files are followed afterwards, so a renewed certificate reaches
/// 465, 587 and STARTTLS on 25 without a restart.
pub(super) fn tls_state(cfg: &ReceiverConfig) -> Option<mailrs_tls_reload::TlsState> {
    match (&cfg.tls_cert, &cfg.tls_key) {
        (Some(cert), Some(key)) => {
            let loaded = mailrs_tls_reload::load_tls_config(cert, key)
                .and_then(|c| Ok((c, mailrs_tls_reload::CertWatcher::new(cert, key)?)));
            match loaded {
                Ok((c, watcher)) => {
                    let state = mailrs_tls_reload::TlsState::new(Arc::unwrap_or_clone(c));
                    mailrs_tls_reload::spawn_watch(
                        state.clone(),
                        watcher,
                        std::time::Duration::from_secs(60),
                        "receiver",
                    );
                    Some(state)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to load TLS cert/key; starting without TLS");
                    None
                }
            }
        }
        _ => None,
    }
}

pub(super) fn users(cfg: &ReceiverConfig) -> Arc<UserStore> {
    Arc::new(match &cfg.users_file {
        Some(path) => UserStore::load(path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load users file; no submission auth");
            UserStore::empty()
        }),
        None => UserStore::empty(),
    })
}

pub(super) fn credential_store(cfg: &ReceiverConfig) -> Option<Arc<dyn crate::CredentialStore>> {
    match (&cfg.core_rpc_base, &cfg.core_api_secret) {
        (Some(base), Some(secret)) => Some(Arc::new(crate::CoreCredentialStore::new(
            base.clone(),
            secret.clone(),
        )) as Arc<dyn crate::CredentialStore>),
        _ => {
            tracing::warn!(
                "MAILRS_CORE_RPC_BASE / MAILRS_CORE_API_SECRET not both set: \
                 submission auth accepts users.toml entries only, and the local \
                 domains are MAILRS_LOCAL_DOMAINS alone"
            );
            None
        }
    }
}

pub(super) fn recipient_check(cfg: &ReceiverConfig) -> Option<crate::RecipientCheck> {
    match (&cfg.core_rpc_base, &cfg.core_api_secret) {
        (Some(base), Some(secret)) => {
            Some(crate::RecipientCheck::new(base.clone(), secret.clone()))
        }
        _ => None,
    }
}

pub(super) async fn runtime_settings(
    cfg: &ReceiverConfig,
    kevy_client: &Arc<KevyNetClient>,
    greylist_local: GreylistLocalHandle,
) -> crate::runtime_settings::RuntimeSettings {
    let runtime = crate::runtime_settings::RuntimeSettings {
        local_domains: crate::runtime_settings::LocalDomains::new(cfg.local_domains.clone()),
        max_message_size: crate::runtime_settings::MessageSizeLimit::new(Some(kevy_client.clone())),
        sender_lists: greylist_local,
    };
    if let (Some(base), Some(secret)) = (&cfg.core_rpc_base, &cfg.core_api_secret) {
        let core = mailrs_core_api::client::Client::new(base.clone(), secret.clone());
        crate::runtime_settings::refresh_and_log(&runtime.local_domains, &core).await;
        let every = std::time::Duration::from_secs(60);
        crate::runtime_settings::spawn_local_domains_refresh(
            runtime.local_domains.clone(),
            core,
            every,
        );
    }
    runtime
}
