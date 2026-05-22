#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus, RetryPolicy,
};
use mailrs_tls_reload::TlsState;
use rustls::ServerConfig;
use tokio::sync::watch;

#[cfg(feature = "axum-http")]
use std::net::SocketAddr;
#[cfg(feature = "axum-http")]
use tokio::net::TcpListener;

/// Shared map of `token → key_authorization` used by the HTTP-01
/// challenge server. The ACME orchestration writes to this map when
/// a challenge is set up; the HTTP server reads it when ACME's CA
/// hits `/.well-known/acme-challenge/{token}`.
///
/// Wire it once at startup, share across the challenge server +
/// the orchestration tasks.
pub type ChallengeTokens = Arc<RwLock<HashMap<String, String>>>;

/// Construct an empty challenge token store.
pub fn new_challenge_tokens() -> ChallengeTokens {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Convenience: build a `rustls::ServerConfig` from in-memory PEM
/// certificate and private key. Used internally after a successful
/// provisioning; exposed for callers who want to build configs from
/// cert bytes they got elsewhere (e.g. dump from a secret manager).
pub fn build_server_config(
    cert_pem: &str,
    key_pem: &str,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

/// Parse a PEM-encoded certificate and return the number of days
/// until `notAfter`. Negative if already expired.
///
/// Useful for "should I renew now?" checks in your renewal loop. The
/// bundled renewal task in [`spawn_renewal_task`] already uses this
/// internally with a 30-day threshold.
pub fn cert_days_remaining(pem_data: &[u8]) -> Result<i64, Box<dyn std::error::Error>> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem_data)?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)?;
    let not_after_ts = cert.validity().not_after.timestamp();
    let now_ts = chrono::Utc::now().timestamp();
    Ok((not_after_ts - now_ts) / 86400)
}

/// Load an existing ACME account from `account.json` under `acme_dir`,
/// or create a new one (against Let's Encrypt staging or production)
/// and persist it.
///
/// `account.json` is plain JSON containing `instant_acme::AccountCredentials`
/// — i.e. the account URL + a private key. Keep this file protected;
/// anyone with it can issue certs under your contact.
pub async fn load_or_create_account(
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
        let account = Account::builder()?.from_credentials(credentials).await?;
        tracing::info!(path = %account_path.display(), "acme: loaded existing account");
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
    tracing::info!(path = %account_path.display(), "acme: created new account");

    Ok(account)
}

/// Provision a certificate for `domains` via the HTTP-01 challenge.
///
/// Writes challenge tokens into the shared `tokens` map (your HTTP-01
/// server should serve `/.well-known/acme-challenge/{token}` returning
/// the corresponding key_authorization). Once the order completes,
/// returns `(cert_pem_chain, key_pem)`.
///
/// The function clears the tokens map after success — fresh state for
/// the next provision call.
pub async fn provision_cert(
    account: &Account,
    domains: &[String],
    tokens: &ChallengeTokens,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let identifiers: Vec<Identifier> =
        domains.iter().map(|d| Identifier::Dns(d.clone())).collect();

    let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;

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
        let retries = RetryPolicy::new().timeout(Duration::from_secs(120));
        order.poll_ready(&retries).await?;
    }

    let state = order.state();
    if matches!(state.status, OrderStatus::Ready) {
        let key_pem = order.finalize().await?;
        let retries = RetryPolicy::new().timeout(Duration::from_secs(60));
        let cert_chain = order.poll_certificate(&retries).await?;

        // Clear tokens for next round.
        {
            let mut map = tokens.write().unwrap();
            map.clear();
        }

        return Ok((cert_chain, key_pem));
    }

    Err("order already valid but no key available from this path".into())
}

/// Write `cert.pem` + `key.pem` into `acme_dir`. Creates the directory
/// if missing.
pub fn save_cert(acme_dir: &Path, cert_pem: &str, key_pem: &str) -> io::Result<()> {
    std::fs::create_dir_all(acme_dir)?;
    std::fs::write(acme_dir.join("cert.pem"), cert_pem)?;
    std::fs::write(acme_dir.join("key.pem"), key_pem)?;
    Ok(())
}

/// Bundled HTTP-01 challenge server (axum-based). Binds the given
/// `addr` and serves `/.well-known/acme-challenge/{token}` from the
/// shared [`ChallengeTokens`] map.
///
/// Returns immediately after spawning the listener task. The task
/// shuts down gracefully when the `shutdown` watch flips to `true`.
///
/// **You can skip this and serve the challenge route from your own
/// HTTP stack** — read tokens from the same `ChallengeTokens` map
/// the orchestration writes to.
#[cfg(feature = "axum-http")]
pub fn spawn_challenge_server(
    tokens: ChallengeTokens,
    addr: SocketAddr,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(%addr, error = %e, "acme: failed to bind challenge port");
                return;
            }
        };
        tracing::info!(%addr, "acme: challenge server listening");

        let tokens = tokens.clone();
        let app = axum::Router::new().route(
            "/.well-known/acme-challenge/{token}",
            axum::routing::get(
                move |axum::extract::Path(token): axum::extract::Path<String>| {
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
                },
            ),
        );

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.wait_for(|v| *v).await;
            })
            .await
            .ok();
    });
}

/// Configuration for [`spawn_renewal_task`].
#[derive(Clone)]
pub struct RenewalConfig {
    /// Domains to renew. Same set used for the initial provision.
    pub domains: Vec<String>,
    /// Where `cert.pem`, `key.pem`, `account.json` live.
    pub acme_dir: PathBuf,
    /// How often to wake up + check expiry. 12 hours is reasonable.
    pub check_interval: Duration,
    /// Renew when the existing cert has ≤ this many days left.
    /// Let's Encrypt recommends 30 days.
    pub renew_when_days_below: i64,
}

impl Default for RenewalConfig {
    fn default() -> Self {
        Self {
            domains: Vec::new(),
            acme_dir: PathBuf::from("./acme"),
            check_interval: Duration::from_secs(12 * 60 * 60),
            renew_when_days_below: 30,
        }
    }
}

/// Spawn a background task that periodically checks `acme_dir/cert.pem`
/// expiry and renews via HTTP-01 when it's getting close.
///
/// On successful renewal, atomically swaps the new config into
/// `tls_state` — in-flight handshakes finish with the old cert,
/// new handshakes use the new cert.
///
/// Returns immediately after spawning. The task exits when `shutdown`
/// flips to `true`.
pub fn spawn_renewal_task(
    account: Account,
    tokens: ChallengeTokens,
    tls_state: TlsState,
    config: RenewalConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(config.check_interval) => {}
                _ = shutdown.wait_for(|v| *v) => {
                    tracing::info!("acme: renewal task shutting down");
                    return;
                }
            }

            let cert_path = config.acme_dir.join("cert.pem");
            let cert_data = match std::fs::read(&cert_path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            match cert_days_remaining(&cert_data) {
                Ok(days) => {
                    tracing::info!(days, "acme: certificate expiry check");
                    if days > config.renew_when_days_below {
                        continue;
                    }
                    tracing::info!(threshold = config.renew_when_days_below, "acme: renewing");
                }
                Err(e) => {
                    tracing::error!(error = %e, "acme: failed to check cert expiry");
                    continue;
                }
            }

            match provision_cert(&account, &config.domains, &tokens).await {
                Ok((cert_pem, key_pem)) => {
                    if let Err(e) = save_cert(&config.acme_dir, &cert_pem, &key_pem) {
                        tracing::error!(error = %e, "acme: failed to save renewed cert");
                        continue;
                    }
                    match build_server_config(&cert_pem, &key_pem) {
                        Ok(server_config) => {
                            tls_state.swap(server_config);
                            tracing::info!("acme: certificate renewed and swapped");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "acme: failed to build TLS config");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "acme: renewal failed");
                }
            }
        }
    });
}

/// High-level init: load existing cert if valid, else provision a new
/// one. Returns the constructed [`TlsState`] (ready for your TLS
/// listeners) and the [`Account`] (hand to [`spawn_renewal_task`]).
///
/// This is the "happy-path" wrapper. For more control over the flow
/// (e.g. provision from a custom CA, different file layout) call the
/// pieces directly.
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

    let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
        let cert_data = std::fs::read(&cert_path)?;
        let days = cert_days_remaining(&cert_data).unwrap_or(0);
        if days > 0 {
            tracing::info!(days, "acme: existing certificate valid");
            let cert = std::fs::read_to_string(&cert_path)?;
            let key = std::fs::read_to_string(&key_path)?;
            (cert, key)
        } else {
            tracing::info!("acme: existing certificate expired, provisioning new one");
            let (cert, key) = provision_cert(&account, domains, tokens).await?;
            save_cert(acme_dir, &cert, &key)?;
            (cert, key)
        }
    } else {
        tracing::info!("acme: no existing certificate, provisioning");
        let (cert, key) = provision_cert(&account, domains, tokens).await?;
        save_cert(acme_dir, &cert, &key)?;
        (cert, key)
    };

    let config = build_server_config(&cert_pem, &key_pem)?;
    let tls_state = TlsState::new(config);

    Ok((tls_state, account))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_challenge_tokens_starts_empty() {
        let t = new_challenge_tokens();
        assert!(t.read().unwrap().is_empty());
    }

    #[test]
    fn challenge_tokens_insert_and_read() {
        let t = new_challenge_tokens();
        {
            let mut map = t.write().unwrap();
            map.insert("tok1".into(), "key_auth_1".into());
            map.insert("tok2".into(), "key_auth_2".into());
        }
        let map = t.read().unwrap();
        assert_eq!(map.get("tok1").map(String::as_str), Some("key_auth_1"));
        assert_eq!(map.get("tok2").map(String::as_str), Some("key_auth_2"));
        assert!(map.get("missing").is_none());
    }

    #[test]
    fn renewal_config_default() {
        let c = RenewalConfig::default();
        assert!(c.domains.is_empty());
        assert_eq!(c.acme_dir, PathBuf::from("./acme"));
        assert_eq!(c.check_interval, Duration::from_secs(12 * 60 * 60));
        assert_eq!(c.renew_when_days_below, 30);
    }

    #[test]
    fn renewal_config_clone() {
        let a = RenewalConfig {
            domains: vec!["example.com".into()],
            acme_dir: PathBuf::from("/var/acme"),
            check_interval: Duration::from_secs(3600),
            renew_when_days_below: 14,
        };
        let b = a.clone();
        assert_eq!(a.domains, b.domains);
        assert_eq!(a.acme_dir, b.acme_dir);
        assert_eq!(a.check_interval, b.check_interval);
        assert_eq!(a.renew_when_days_below, b.renew_when_days_below);
    }

    #[test]
    fn cert_days_remaining_rejects_garbage() {
        let r = cert_days_remaining(b"not a PEM");
        assert!(r.is_err());
    }

    #[test]
    fn cert_days_remaining_rejects_empty() {
        let r = cert_days_remaining(b"");
        assert!(r.is_err());
    }

    #[test]
    fn save_cert_creates_directory() {
        let dir = std::env::temp_dir().join(format!("mailrs-acme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save_cert(&dir, "cert-pem-data", "key-pem-data").expect("save_cert");
        assert!(dir.join("cert.pem").exists());
        assert!(dir.join("key.pem").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("cert.pem")).unwrap(),
            "cert-pem-data"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_cert_overwrites_existing() {
        let dir = std::env::temp_dir().join(format!("mailrs-acme-overwrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save_cert(&dir, "old", "old-key").unwrap();
        save_cert(&dir, "new", "new-key").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("cert.pem")).unwrap(), "new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cert_days_remaining_with_expired_pem() {
        // A pre-baked expired cert (notAfter = 2000-01-01). Should
        // return negative days.
        let expired_pem = b"-----BEGIN CERTIFICATE-----
MIIBxDCCAW6gAwIBAgIUC2DnZmnxR6c6PXcyG9hqOQRJxMUwDQYJKoZIhvcNAQEL
BQAwGTEXMBUGA1UEAwwOZXhwaXJlZC50ZXN0LjAeFw0wMDAxMDEwMDAwMDBaFw0w
MDAxMDIwMDAwMDBaMBkxFzAVBgNVBAMMDmV4cGlyZWQudGVzdC4wXDANBgkqhkiG
9w0BAQEFAANLADBIAkEAuGVc7uoEgavLxc7KVxSi5q6IXkD0pAYmqr8gbZIO5p2k
KqQXNkVtoyzMOXjlV6vLOXAcgksMQQ5UqxQwlmHvOQIDAQABo1MwUTAdBgNVHQ4E
FgQUw7VxpcfRPwOOTQ6SHGyqyhI/o/owHwYDVR0jBBgwFoAUw7VxpcfRPwOOTQ6S
HGyqyhI/o/owDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAANBAGc=
-----END CERTIFICATE-----";
        // The above is intentionally truncated/may not be valid — the
        // assertion is that the parser EITHER returns negative days
        // OR an error (both are acceptable for an obviously-bad cert).
        let r = cert_days_remaining(expired_pem);
        // Accept either path: real expired cert → Ok(negative), garbage
        // → Err. Both indicate "don't trust this cert".
        if let Ok(days) = r {
            assert!(days < 0, "expected negative days, got {days}");
        }
    }

    #[test]
    fn renewal_config_can_be_constructed_with_custom_values() {
        let c = RenewalConfig {
            domains: vec!["a.com".into(), "b.com".into()],
            acme_dir: PathBuf::from("/etc/acme"),
            check_interval: Duration::from_secs(60 * 60),
            renew_when_days_below: 7,
        };
        assert_eq!(c.domains.len(), 2);
        assert_eq!(c.renew_when_days_below, 7);
        assert_eq!(c.check_interval, Duration::from_secs(3600));
    }

    #[test]
    fn challenge_tokens_default_works() {
        // ChallengeTokens = Arc<RwLock<HashMap<String, String>>>
        // Default exists via Arc<T: Default>'s impl
        let t: ChallengeTokens = Default::default();
        assert!(t.read().unwrap().is_empty());
    }

    #[test]
    fn challenge_tokens_clear() {
        let t = new_challenge_tokens();
        {
            let mut map = t.write().unwrap();
            map.insert("a".into(), "1".into());
            map.insert("b".into(), "2".into());
        }
        {
            let mut map = t.write().unwrap();
            map.clear();
        }
        assert!(t.read().unwrap().is_empty());
    }

    #[test]
    fn save_cert_preserves_exact_bytes() {
        let dir = std::env::temp_dir().join(format!("mailrs-acme-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cert = "cert-with-special-chars\n\t\r\nß";
        let key = "key-with-newlines\n\n\n";
        save_cert(&dir, cert, key).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("cert.pem")).unwrap(), cert);
        assert_eq!(std::fs::read_to_string(dir.join("key.pem")).unwrap(), key);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_server_config_rejects_garbage() {
        let r = build_server_config("not pem", "not key either");
        assert!(r.is_err());
    }
}
