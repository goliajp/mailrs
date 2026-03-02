use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus, RetryPolicy,
};
use rustls::ServerConfig;
use tokio::net::TcpListener;

use crate::tls::TlsState;

pub type ChallengeTokens = Arc<RwLock<HashMap<String, String>>>;

/// load existing ACME account or create a new one
async fn load_or_create_account(
    email: &str,
    staging: bool,
    acme_dir: &Path,
) -> Result<Account, Box<dyn std::error::Error>> {
    let account_path = acme_dir.join("account.json");

    let url = if staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    if account_path.exists() {
        let data = std::fs::read_to_string(&account_path)?;
        let credentials: instant_acme::AccountCredentials = serde_json::from_str(&data)?;
        let account = Account::builder()?
            .from_credentials(credentials)
            .await?;
        eprintln!("acme: loaded existing account from {}", account_path.display());
        return Ok(account);
    }

    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[&format!("mailto:{email}")],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            url.to_string(),
            None,
        )
        .await?;

    std::fs::create_dir_all(acme_dir)?;
    std::fs::write(&account_path, serde_json::to_string_pretty(&credentials)?)?;
    eprintln!("acme: created new account, saved to {}", account_path.display());

    Ok(account)
}

/// provision a certificate for the given domains
async fn provision_cert(
    account: &Account,
    domains: &[String],
    tokens: &ChallengeTokens,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let identifiers: Vec<Identifier> = domains
        .iter()
        .map(|d| Identifier::Dns(d.clone()))
        .collect();

    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await?;

    let state = order.state();
    if matches!(state.status, OrderStatus::Pending) {
        let mut authz_stream = order.authorizations();
        while let Some(result) = authz_stream.next().await {
            let mut authz = result?;
            match authz.status {
                AuthorizationStatus::Pending => {}
                AuthorizationStatus::Valid => continue,
                _ => return Err(format!("unexpected authz status: {:?}", authz.status).into()),
            }

            let mut challenge = authz
                .challenge(ChallengeType::Http01)
                .ok_or("no HTTP-01 challenge found")?;

            let key_auth = challenge.key_authorization();
            {
                let mut map = tokens.write().unwrap();
                map.insert(challenge.token.clone(), key_auth.as_str().to_string());
            }

            challenge.set_ready().await?;
        }

        // poll until ready
        let retries = RetryPolicy::new()
            .timeout(Duration::from_secs(120));
        order.poll_ready(&retries).await?;
    }

    let state = order.state();
    if matches!(state.status, OrderStatus::Ready) {
        // finalize generates a CSR internally (rcgen feature) and returns private key PEM
        let key_pem = order.finalize().await?;

        // poll until certificate is issued
        let retries = RetryPolicy::new()
            .timeout(Duration::from_secs(60));
        let cert_chain = order.poll_certificate(&retries).await?;

        // clear tokens
        {
            let mut map = tokens.write().unwrap();
            map.clear();
        }

        return Ok((cert_chain, key_pem));
    }

    // order already valid — shouldn't happen in normal flow
    Err("order already valid but no key available from this path".into())
}

/// parse a PEM certificate and return days until expiration
pub fn cert_days_remaining(pem_data: &[u8]) -> Result<i64, Box<dyn std::error::Error>> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem_data)?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)?;
    let not_after_ts = cert.validity().not_after.timestamp();
    let now_ts = chrono::Utc::now().timestamp();
    Ok((not_after_ts - now_ts) / 86400)
}

/// build rustls ServerConfig from PEM cert + key
fn build_server_config(
    cert_pem: &str,
    key_pem: &str,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let certs = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
        .ok_or("no private key found in PEM")?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

/// spawn HTTP-01 challenge server on port 80
pub fn spawn_challenge_server(
    tokens: ChallengeTokens,
    shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let listener = match TcpListener::bind("0.0.0.0:80").await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("acme: failed to bind port 80: {e}");
                return;
            }
        };
        eprintln!("acme: challenge server on port 80");

        let tokens = tokens.clone();
        let app = axum::Router::new().route(
            "/.well-known/acme-challenge/{token}",
            axum::routing::get(move |axum::extract::Path(token): axum::extract::Path<String>| {
                let tokens = tokens.clone();
                async move {
                    let map = tokens.read().unwrap();
                    match map.get(&token) {
                        Some(key_auth) => (
                            axum::http::StatusCode::OK,
                            [(axum::http::header::CONTENT_TYPE, "text/plain")],
                            key_auth.clone(),
                        ),
                        None => (
                            axum::http::StatusCode::NOT_FOUND,
                            [(axum::http::header::CONTENT_TYPE, "text/plain")],
                            "not found".to_string(),
                        ),
                    }
                }
            }),
        );

        let mut shutdown = shutdown;
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.wait_for(|v| *v).await;
            })
            .await
            .ok();
    });
}

/// spawn background task that checks certificate expiry every 12h
pub fn spawn_renewal_task(
    account: Account,
    domains: Vec<String>,
    tokens: ChallengeTokens,
    acme_dir: PathBuf,
    tls_state: TlsState,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(12 * 60 * 60);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shutdown.wait_for(|v| *v) => {
                    eprintln!("acme: renewal task shutting down");
                    return;
                }
            }

            let cert_path = acme_dir.join("cert.pem");
            let cert_data = match std::fs::read(&cert_path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            match cert_days_remaining(&cert_data) {
                Ok(days) => {
                    eprintln!("acme: certificate expires in {days} days");
                    if days > 30 {
                        continue;
                    }
                    eprintln!("acme: renewing certificate (≤30 days remaining)");
                }
                Err(e) => {
                    eprintln!("acme: failed to check cert expiry: {e}");
                    continue;
                }
            }

            match provision_cert(&account, &domains, &tokens).await {
                Ok((cert_pem, key_pem)) => {
                    if let Err(e) = save_cert(&acme_dir, &cert_pem, &key_pem) {
                        eprintln!("acme: failed to save renewed cert: {e}");
                        continue;
                    }
                    match build_server_config(&cert_pem, &key_pem) {
                        Ok(config) => {
                            tls_state.swap(config);
                            eprintln!("acme: certificate renewed and swapped");
                        }
                        Err(e) => eprintln!("acme: failed to build TLS config: {e}"),
                    }
                }
                Err(e) => eprintln!("acme: renewal failed: {e}"),
            }
        }
    });
}

fn save_cert(acme_dir: &Path, cert_pem: &str, key_pem: &str) -> io::Result<()> {
    std::fs::create_dir_all(acme_dir)?;
    std::fs::write(acme_dir.join("cert.pem"), cert_pem)?;
    std::fs::write(acme_dir.join("key.pem"), key_pem)?;
    Ok(())
}

/// initialize ACME: load existing cert or provision new one, return TlsState
pub async fn init(
    email: &str,
    domains: &[String],
    acme_dir: &Path,
    staging: bool,
    tokens: &ChallengeTokens,
) -> Result<(TlsState, Account), Box<dyn std::error::Error>> {
    let account = load_or_create_account(email, staging, acme_dir).await?;

    let cert_path = acme_dir.join("cert.pem");
    let key_path = acme_dir.join("key.pem");

    // try loading existing cert
    let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
        let cert_data = std::fs::read(&cert_path)?;
        let days = cert_days_remaining(&cert_data).unwrap_or(0);
        if days > 0 {
            eprintln!("acme: existing certificate valid for {days} days");
            let cert = std::fs::read_to_string(&cert_path)?;
            let key = std::fs::read_to_string(&key_path)?;
            (cert, key)
        } else {
            eprintln!("acme: existing certificate expired, provisioning new one");
            let (cert, key) = provision_cert(&account, domains, tokens).await?;
            save_cert(acme_dir, &cert, &key)?;
            (cert, key)
        }
    } else {
        eprintln!("acme: no existing certificate, provisioning");
        let (cert, key) = provision_cert(&account, domains, tokens).await?;
        save_cert(acme_dir, &cert, &key)?;
        (cert, key)
    };

    let config = build_server_config(&cert_pem, &key_pem)?;
    let tls_state = TlsState::new(config);

    Ok((tls_state, account))
}
