//! ACME HTTP-01 renewal task — spawned from fastcore's `run()`.
//!
//! Fastcore owns the cert lifecycle in the split topology. Receiver +
//! webapi read certs from disk and manage their own TLS state; fastcore
//! only writes new cert files and lets those consumers pick them up on
//! their own reload cycle.
//!
//! Layout under `MAILRS_ACME_DIR` (default `/data/acme`):
//! - `account.json` — pkcs8 private key + LE account URL, persisted so
//!   the same account is reused across restarts (rate limits apply).
//! - `cert.pem` / `key.pem` — most recent issued cert + private key.
//!
//! Boot flow:
//! 1. Load or create the LE account.
//! 2. Bind the challenge server on `MAILRS_ACME_CHALLENGE_ADDR`
//!    (default `0.0.0.0:80`).
//! 3. If cert.pem is missing OR within the renewal threshold, provision
//!    immediately. Otherwise skip.
//! 4. Enter the periodic renewal loop (12 h tick, 30-day threshold).
//!
//! Env:
//! - `MAILRS_ACME_EMAIL`         — required to enable. Leave unset to skip.
//! - `MAILRS_ACME_DOMAINS`       — comma-separated. Required when EMAIL set.
//! - `MAILRS_ACME_DIR`           — persistence dir. Default `/data/acme`.
//! - `MAILRS_ACME_STAGING`       — "true" hits LE staging. Default `false`.
//! - `MAILRS_ACME_CHALLENGE_ADDR`— bind for challenge HTTP. Default `0.0.0.0:80`.
//! - `MAILRS_ACME_RENEW_DAYS`    — renewal threshold. Default `30`.
//! - `MAILRS_ACME_CHECK_SECS`    — poll cadence. Default `43200` (12 h).

use std::path::{Path, PathBuf};
use std::time::Duration;

use mailrs_acme::{
    ChallengeTokens, cert_days_remaining, load_or_create_account, provision_cert, save_cert,
    spawn_challenge_server,
};
use tokio::sync::watch;

/// Entry point called from fastcore's `run()`. All failures downgrade
/// to warn — fastcore keeps serving even when ACME is off or broken.
pub async fn spawn() {
    let Some(email) = std::env::var("MAILRS_ACME_EMAIL")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        tracing::debug!("MAILRS_ACME_EMAIL unset — ACME disabled");
        return;
    };
    let Some(domains_raw) = std::env::var("MAILRS_ACME_DOMAINS")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        tracing::warn!("MAILRS_ACME_EMAIL is set but MAILRS_ACME_DOMAINS is empty; ACME disabled");
        return;
    };
    let domains: Vec<String> = domains_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if domains.is_empty() {
        tracing::warn!("MAILRS_ACME_DOMAINS parsed to empty; ACME disabled");
        return;
    }
    let acme_dir = PathBuf::from(
        std::env::var("MAILRS_ACME_DIR").unwrap_or_else(|_| "/data/acme".to_string()),
    );
    let staging = std::env::var("MAILRS_ACME_STAGING")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let challenge_addr: std::net::SocketAddr = std::env::var("MAILRS_ACME_CHALLENGE_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| ([0, 0, 0, 0], 80).into());
    let renew_days: i64 = std::env::var("MAILRS_ACME_RENEW_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let check_secs: u64 = std::env::var("MAILRS_ACME_CHECK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12 * 60 * 60);

    if let Err(e) = std::fs::create_dir_all(&acme_dir) {
        tracing::error!(error = %e, dir = %acme_dir.display(), "acme: mkdir acme_dir failed");
        return;
    }

    let tokens: ChallengeTokens = mailrs_acme::new_challenge_tokens();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    // Keep the sender alive for the lifetime of the process — dropping
    // it would flip the receiver to true and stop the challenge server.
    Box::leak(Box::new(shutdown_tx));

    spawn_challenge_server(tokens.clone(), challenge_addr, shutdown_rx.clone());

    let account = match load_or_create_account(&email, staging, &acme_dir).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "acme: load_or_create_account failed");
            return;
        }
    };
    tracing::info!(
        %email, staging, domains = ?domains,
        dir = %acme_dir.display(), challenge = %challenge_addr,
        "acme: account ready"
    );

    // First-run bootstrap — provision immediately if cert.pem missing
    // OR already close to expiry. Best-effort; failures fall through to
    // the renewal loop's retry cadence.
    let (bootstrap_account, bootstrap_domains) = (account.clone(), domains.clone());
    let bootstrap_dir = acme_dir.clone();
    let bootstrap_tokens = tokens.clone();
    tokio::spawn(async move {
        renew_pass(
            &bootstrap_account,
            &bootstrap_domains,
            &bootstrap_dir,
            &bootstrap_tokens,
            renew_days,
        )
        .await;

        // Long-running periodic renewal — same task so `account` isn't
        // re-parsed. Sleeps between passes.
        loop {
            tokio::time::sleep(Duration::from_secs(check_secs)).await;
            renew_pass(
                &bootstrap_account,
                &bootstrap_domains,
                &bootstrap_dir,
                &bootstrap_tokens,
                renew_days,
            )
            .await;
        }
    });
    tracing::info!(check_secs, renew_days, "acme: renewal loop spawned");
}

/// One expiry check + optional renew. Extracted so `spawn`'s spawned
/// task can call it both at boot and in the sleep loop without keeping
/// two copies of the ~15-line body.
async fn renew_pass(
    account: &instant_acme::Account,
    domains: &[String],
    acme_dir: &Path,
    tokens: &ChallengeTokens,
    renew_days: i64,
) {
    let cert_path = acme_dir.join("cert.pem");
    let should_renew = match std::fs::read(&cert_path) {
        Ok(bytes) => match cert_days_remaining(&bytes) {
            Ok(days) => {
                tracing::info!(days, "acme: cert expiry check");
                days <= renew_days
            }
            Err(e) => {
                tracing::warn!(error = %e, "acme: cert_days_remaining failed; will attempt renewal");
                true
            }
        },
        Err(_) => {
            tracing::info!("acme: no cert.pem found; provisioning");
            true
        }
    };
    if !should_renew {
        return;
    }
    match provision_cert(account, domains, tokens).await {
        Ok((cert_pem, key_pem)) => match save_cert(acme_dir, &cert_pem, &key_pem) {
            Ok(()) => {
                tracing::info!(dir = %acme_dir.display(), "acme: cert renewed + saved");
            }
            Err(e) => {
                tracing::error!(error = %e, "acme: save_cert failed");
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "acme: provision_cert failed; retry next tick");
        }
    }
}
