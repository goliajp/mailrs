//! Fastcore-native outbound SMTP sender.
//!
//! Drains `mailrs:outbound:pending` (RPOP → FIFO oldest first), resolves
//! MX for the recipient domain, connects on port 25, opportunistically
//! STARTTLS, and delivers the raw `message_data` blob via `MAIL FROM /
//! RCPT TO / DATA`. On 2xx it DELs the blob; on transient errors it
//! bumps the `attempts` counter and RPUSHes back to the tail with a
//! per-attempt sleep floor; on 5xx or attempts-exhausted it moves the
//! id into `mailrs:outbound:failed` for operator inspection.
//!
//! No spg. No monolith. Reads/writes only network kevy at
//! `MAILRS_KEVY_URL`.
//!
//! Env:
//!   MAILRS_KEVY_URL              — required, kevy://host:port
//!   MAILRS_HELO_HOSTNAME         — default "mail.golia.jp"
//!   MAILRS_SENDER_MAX_ATTEMPTS   — default 10
//!   MAILRS_SENDER_POLL_MS        — default 500 (idle sleep)
//!   MAILRS_SENDER_RETRY_MIN_SECS — default 60 (per-item minimum retry delay)

use std::sync::Arc;
use std::time::Duration;

use mailrs_outbound_queue::dkim_sign::DkimSignConfig;
use mailrs_smtp_client::{SmtpConnection, TimeoutConfig, TokioResolver, resolve_mx};
use tokio::task::spawn_blocking;

const PENDING_KEY: &[u8] = b"mailrs:outbound:pending";
const FAILED_KEY: &[u8] = b"mailrs:outbound:failed";

#[derive(Clone)]
struct Cfg {
    kevy_url: String,
    helo: String,
    max_attempts: u32,
    poll_ms: u64,
    retry_min_secs: i64,
    /// DKIM signing enabled when `MAILRS_DKIM_DOMAIN`,
    /// `MAILRS_DKIM_SELECTOR`, and `MAILRS_DKIM_PRIVATE_KEY_PEM_FILE`
    /// are all set. Public MX (Gmail / Outlook / etc.) drop unsigned
    /// mail from mailrs-hosted domains into spam.
    dkim: Option<Arc<DkimSignConfig>>,
}

impl Cfg {
    fn from_env() -> Self {
        Self {
            kevy_url: std::env::var("MAILRS_KEVY_URL")
                .expect("MAILRS_KEVY_URL required (kevy://host:port)"),
            helo: std::env::var("MAILRS_HELO_HOSTNAME")
                .unwrap_or_else(|_| "mail.golia.jp".to_string()),
            max_attempts: std::env::var("MAILRS_SENDER_MAX_ATTEMPTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            poll_ms: std::env::var("MAILRS_SENDER_POLL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),
            retry_min_secs: std::env::var("MAILRS_SENDER_RETRY_MIN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
            dkim: load_dkim_from_env(),
        }
    }
}

fn load_dkim_from_env() -> Option<Arc<DkimSignConfig>> {
    let domain = std::env::var("MAILRS_DKIM_DOMAIN").ok()?;
    let selector = std::env::var("MAILRS_DKIM_SELECTOR").ok()?;
    // Accept either the monolith's env-var convention
    // (MAILRS_DKIM_PRIVATE_KEY = file path) or inline PEM
    // (MAILRS_DKIM_PRIVATE_KEY_PEM). The file path takes precedence.
    let pem = if let Ok(path) = std::env::var("MAILRS_DKIM_PRIVATE_KEY") {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(%path, err = %e, "MAILRS_DKIM_PRIVATE_KEY unreadable");
                return None;
            }
        }
    } else if let Ok(path) = std::env::var("MAILRS_DKIM_PRIVATE_KEY_PEM_FILE") {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(%path, err = %e, "MAILRS_DKIM_PRIVATE_KEY_PEM_FILE unreadable");
                return None;
            }
        }
    } else if let Ok(pem) = std::env::var("MAILRS_DKIM_PRIVATE_KEY_PEM") {
        pem
    } else {
        return None;
    };
    Some(Arc::new(DkimSignConfig {
        selector,
        domain,
        private_key_pem: pem,
        parsed_key: Arc::new(std::sync::OnceLock::new()),
        extra_keys: std::collections::HashMap::new(),
    }))
}

fn kevy(url: &str) -> std::io::Result<kevy_client::Connection> {
    kevy_client::Connection::open(url)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Attempt to pop one id from pending. Returns `Ok(None)` on empty.
async fn pop_next(cfg: Cfg) -> std::io::Result<Option<String>> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let popped = c.rpop(PENDING_KEY, 1)?;
        Ok(popped
            .into_iter()
            .next()
            .map(|v| String::from_utf8_lossy(&v).to_string()))
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Move scheduled outbound whose send time has arrived into pending.
/// The scheduled zset is score-ordered by send-at epoch, so we walk
/// from the front and stop at the first future item.
const SCHEDULED_KEY: &[u8] = b"mailrs:outbound:scheduled";
async fn promote_due(cfg: Cfg) -> std::io::Result<()> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let now = now_secs() as f64;
        // ascending by score; batch of 100 due items per tick is plenty
        let members = c.zrange(SCHEDULED_KEY, 0, 99)?;
        for m in members {
            let score = c.zscore(SCHEDULED_KEY, &m)?.unwrap_or(f64::MAX);
            if score > now {
                break; // rest are future
            }
            // due: pending first, then remove from scheduled — a crash
            // between the two re-promotes harmlessly (idempotent)
            c.lpush(PENDING_KEY, &[m.as_slice()])?;
            c.zrem(SCHEDULED_KEY, &[m.as_slice()])?;
        }
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Fetch the envelope for `id`. Returns `Ok(None)` if blob missing.
async fn load_envelope(cfg: Cfg, id: String) -> std::io::Result<Option<serde_json::Value>> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let key = format!("mailrs:outbound:{id}");
        let blob = c.hget(key.as_bytes(), b"blob")?;
        let Some(bytes) = blob else { return Ok(None) };
        let v: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::other(format!("blob json: {e}")))?;
        Ok(Some(v))
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Delete the blob for `id` (successful delivery or terminal failure).
async fn drop_blob(cfg: Cfg, id: String) -> std::io::Result<()> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let key = format!("mailrs:outbound:{id}");
        c.del(&[key.as_bytes()])?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Move the id into `mailrs:outbound:failed` (SET) and drop the blob.
/// Blob is retained for operator inspection only when `keep_blob=true`.
async fn move_to_failed(
    cfg: Cfg,
    id: String,
    reason: String,
    keep_blob: bool,
) -> std::io::Result<()> {
    let id_c = id.clone();
    let reason_c = reason.clone();
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        c.sadd(FAILED_KEY, &[id_c.as_bytes()])?;
        let audit_key = format!("mailrs:outbound:failed:{id_c}");
        c.hset(
            audit_key.as_bytes(),
            &[
                (b"failed_at" as &[u8], now_secs().to_string().as_bytes()),
                (b"reason", reason_c.as_bytes()),
            ],
        )?;
        if !keep_blob {
            let blob_key = format!("mailrs:outbound:{id_c}");
            c.del(&[blob_key.as_bytes()])?;
        }
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Compose an RFC 3464 DSN for a permanently failed delivery and push
/// it onto the bounce hand-off queue fastcore drains (G9). Null /
/// daemon senders are suppressed — a bounce never bounces.
async fn enqueue_bounce_dsn(
    cfg: &Cfg,
    sender: &str,
    recipient: &str,
    reason: &str,
    message: &[u8],
) {
    use base64::Engine as _;
    if mailrs_fastcore::bounce::suppress_bounce(sender) {
        tracing::info!(%recipient, "bounce suppressed (null/daemon sender)");
        return;
    }
    let dsn = mailrs_fastcore::bounce::compose_dsn(
        &cfg.helo,
        sender.trim_matches(|c| c == '<' || c == '>'),
        recipient,
        "5.0.0",
        reason,
        message,
    );
    let cfg = cfg.clone();
    let sender = sender.trim_matches(|c| c == '<' || c == '>').to_string();
    let res = spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let id = format!("{}-{}", now_secs(), std::process::id());
        let key = format!("mailrs:bounce:{id}");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&dsn);
        c.hset(
            key.as_bytes(),
            &[
                (b"recipient" as &[u8], sender.as_bytes()),
                (b"blob", b64.as_bytes()),
            ],
        )?;
        c.lpush(mailrs_fastcore::bounce::BOUNCE_PENDING, &[id.as_bytes()])?;
        Ok::<(), std::io::Error>(())
    })
    .await;
    match res {
        Ok(Ok(())) => {}
        other => tracing::warn!(?other, "bounce enqueue failed"),
    }
}

/// Persist updated envelope (new attempts / last_error) and RPUSH back
/// to the pending tail for a retry.
async fn requeue(cfg: Cfg, id: String, envelope: serde_json::Value) -> std::io::Result<()> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let key = format!("mailrs:outbound:{id}");
        let payload = envelope.to_string();
        c.hset(key.as_bytes(), &[(b"blob" as &[u8], payload.as_bytes())])?;
        c.rpush(PENDING_KEY, &[id.as_bytes()])?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Delivery outcome per RCPT.
enum Outcome {
    /// 2xx on DATA final response.
    Delivered,
    /// 4xx anywhere in the exchange, or a network error — retry.
    Transient(String),
    /// 5xx anywhere in the exchange — do not retry.
    Permanent(String),
}

/// Extract the addr-spec from an RFC 5322 mailbox token.
///
/// Accepts both `addr@domain` (bare `addr-spec`) and
/// `Display Name <addr@domain>` (bare `name-addr`) forms. Trims
/// surrounding whitespace. Never panics — returns the original trimmed
/// input on parse failure so the caller can surface a
/// `Permanent(invalid recipient …)` error one level up.
fn extract_addr_spec(raw: &str) -> &str {
    let t = raw.trim();
    if let Some(start) = t.rfind('<')
        && let Some(end) = t.rfind('>')
        && end > start
    {
        return t[start + 1..end].trim();
    }
    t
}

/// Attempt SMTP delivery via the recipient's MX hosts, in priority
/// order. Returns the first non-transient outcome; on all-transient
/// exhaustion returns `Outcome::Transient` with the last error.
async fn try_deliver(cfg: &Cfg, sender: &str, recipient_raw: &str, message: &[u8]) -> Outcome {
    let recipient = extract_addr_spec(recipient_raw);
    let sender = extract_addr_spec(sender);

    // DKIM sign if a signing key is configured. Fatal signing errors
    // are permanent (won't heal on retry): message data is malformed
    // or the private key is broken.
    let signed;
    let payload: &[u8] = match cfg.dkim.as_ref() {
        Some(dkim) => match dkim.sign(message) {
            Ok(bytes) => {
                signed = bytes;
                &signed
            }
            Err(e) => {
                return Outcome::Permanent(format!("dkim sign: {e}"));
            }
        },
        None => message,
    };
    let Some(domain) = recipient.split('@').nth(1) else {
        return Outcome::Permanent(format!("invalid recipient: {recipient_raw}"));
    };
    if domain.is_empty() || domain.contains(char::is_whitespace) {
        return Outcome::Permanent(format!("invalid recipient: {recipient_raw}"));
    }

    let resolver = match TokioResolver::builder_tokio() {
        Ok(b) => match b.build() {
            Ok(r) => r,
            Err(e) => return Outcome::Transient(format!("resolver build: {e}")),
        },
        Err(e) => return Outcome::Transient(format!("resolver builder: {e}")),
    };

    let mx_records = match resolve_mx(&resolver, domain).await {
        Ok(v) => v,
        Err(e) => return Outcome::Transient(format!("mx lookup: {e}")),
    };
    if mx_records.is_empty() {
        return Outcome::Transient(format!("no MX for {domain}"));
    }

    // MTA-STS policy (G8): enforce mode forbids plaintext downgrade and
    // restricts delivery to the policy's mx: set. testing/none/absent =
    // opportunistic (unchanged). Fail-open on any discovery error.
    let sts_policy = mailrs_fastcore::sender_sts::fetch_policy(&cfg.kevy_url, domain).await;
    let sts_enforce = sts_policy
        .as_ref()
        .map(mailrs_fastcore::sender_sts::is_enforce)
        .unwrap_or(false);

    let timeouts = TimeoutConfig::default();
    let mut last_err = String::from("no MX host attempted");

    for mx in &mx_records {
        // enforce: skip MX not covered by the policy's mx: patterns
        if let Some(policy) = &sts_policy
            && sts_enforce
            && mailrs_fastcore::sender_sts::mx_decision(policy, &mx.exchange)
                == mailrs_mta_sts::Decision::Deny
        {
            last_err = format!("mta-sts enforce: {} not in policy mx:", mx.exchange);
            tracing::warn!(err = %last_err, "MX excluded by STS policy, next MX");
            mailrs_fastcore::tlsrpt::record(
                &cfg.kevy_url,
                &mailrs_fastcore::tlsrpt::TlsEvent {
                    domain: domain.to_string(),
                    mx: mx.exchange.to_string(),
                    success: false,
                    failure_type: Some("mx-mismatch".into()),
                    detail: Some(last_err.clone()),
                },
            );
            continue;
        }
        tracing::info!(mx = %mx.exchange, priority = mx.priority, %recipient, "attempt");
        let conn = match SmtpConnection::connect_with_timeout(&mx.exchange, 25, &timeouts).await {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("connect {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "connect failed, next MX");
                continue;
            }
        };

        // First EHLO (plain).
        let mut conn = conn;
        let mut tls_used = false;
        if let Err(e) = conn.ehlo(&cfg.helo).await {
            last_err = format!("ehlo {}: {e}", mx.exchange);
            tracing::warn!(err = %last_err, "ehlo failed, next MX");
            continue;
        }

        // Opportunistic STARTTLS with plaintext downgrade on failure.
        //
        // - Success: upgrade + re-EHLO
        // - Rejected (server refused STARTTLS): stay on the same plaintext conn
        // - HandshakeFailed (peer cert expired / SNI mismatch / etc.):
        //   the TCP session is dead, so open a fresh plaintext session
        //   and continue there. This matches how Gmail/O365/Postfix
        //   handle opportunistic-TLS failures — SPF/DKIM/DMARC (not TLS)
        //   are the real integrity guarantees for interpersonal mail.
        // DANE (RFC 7672 / G8.2): if the MX publishes DNSSEC-anchored
        // TLSA records, TLS is mandatory and the cert is verified
        // against them — a missing/failed handshake must NOT downgrade.
        let tlsa = mailrs_smtp_client::resolve_tlsa(&resolver, &mx.exchange).await;
        let dane_active = !tlsa.is_empty();
        let starttls = if dane_active {
            let cfg = mailrs_smtp_client::dane_tls_config(tlsa);
            conn.try_starttls_with_config(&mx.exchange, cfg).await
        } else {
            conn.try_starttls(&mx.exchange).await
        };
        let conn = match starttls {
            mailrs_smtp_client::StarttlsResult::Success(c) => {
                let mut c = c;
                if let Err(e) = c.ehlo(&cfg.helo).await {
                    last_err = format!("ehlo-after-starttls {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "post-tls ehlo failed, next MX");
                    continue;
                }
                tls_used = true;
                c
            }
            mailrs_smtp_client::StarttlsResult::Rejected {
                conn,
                code,
                message: msg,
            } => {
                if sts_enforce || dane_active {
                    last_err = format!("mta-sts enforce: {} refused STARTTLS", mx.exchange);
                    tracing::warn!(err = %last_err, "STARTTLS refused under STS enforce, next MX");
                    mailrs_fastcore::tlsrpt::record(
                        &cfg.kevy_url,
                        &mailrs_fastcore::tlsrpt::TlsEvent {
                            domain: domain.to_string(),
                            mx: mx.exchange.to_string(),
                            success: false,
                            failure_type: Some("starttls-not-supported".into()),
                            detail: Some(last_err.clone()),
                        },
                    );
                    let mut c = conn;
                    let _ = c.quit().await;
                    continue;
                }
                tracing::info!(code, %msg, "STARTTLS rejected, continuing plaintext");
                conn
            }
            mailrs_smtp_client::StarttlsResult::HandshakeFailed { source, .. } => {
                if sts_enforce || dane_active {
                    last_err = format!("mta-sts enforce: {} TLS handshake failed", mx.exchange);
                    tracing::warn!(err = %last_err, "TLS handshake failed under STS enforce, next MX");
                    continue;
                }
                tracing::warn!(
                    err = %source,
                    mx = %mx.exchange,
                    "STARTTLS handshake failed, downgrading to plaintext"
                );
                let mut plain = match SmtpConnection::connect_with_timeout(
                    &mx.exchange,
                    25,
                    &timeouts,
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        last_err = format!("plaintext reconnect {}: {e}", mx.exchange);
                        tracing::warn!(err = %last_err, "reconnect after TLS failure failed, next MX");
                        continue;
                    }
                };
                if let Err(e) = plain.ehlo(&cfg.helo).await {
                    last_err = format!("plaintext ehlo {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "plaintext ehlo after TLS failure failed, next MX");
                    continue;
                }
                plain
            }
        };

        let mut conn = conn;
        let resp = match conn.deliver(sender, &[recipient], payload).await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("deliver {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "deliver io error, next MX");
                let _ = conn.quit().await;
                continue;
            }
        };

        let _ = conn.quit().await;

        if resp.is_positive() {
            tracing::info!(mx = %mx.exchange, code = resp.code, msg = %resp.message(), "delivered");
            // TLS-RPT success event (G8.3): tls_used tracks whether the
            // final connection upgraded — set at STARTTLS resolution.
            mailrs_fastcore::tlsrpt::record(
                &cfg.kevy_url,
                &mailrs_fastcore::tlsrpt::TlsEvent {
                    domain: domain.to_string(),
                    mx: mx.exchange.to_string(),
                    success: tls_used,
                    failure_type: (!tls_used).then(|| "starttls-not-supported".to_string()),
                    detail: None,
                },
            );
            return Outcome::Delivered;
        }
        if resp.is_permanent_error() {
            let msg = format!("{} {} {}", mx.exchange, resp.code, resp.message());
            tracing::warn!(err = %msg, "permanent rejection");
            return Outcome::Permanent(msg);
        }
        // Transient (4xx): try next MX before giving up on this attempt.
        last_err = format!("{} {} {}", mx.exchange, resp.code, resp.message());
        tracing::warn!(err = %last_err, "transient rejection, next MX");
    }

    Outcome::Transient(last_err)
}

/// Process one dequeued id. Never panics — logs everything.
async fn process_one(cfg: Cfg, id: String) {
    let envelope = match load_envelope(cfg.clone(), id.clone()).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            tracing::warn!(%id, "blob missing, dropped from pending");
            return;
        }
        Err(e) => {
            tracing::error!(%id, err = %e, "load envelope failed, requeue-and-hope");
            // requeue as-is with a synthetic envelope containing just id
            let filler = serde_json::json!({"id": id, "attempts": 1, "last_error": e.to_string()});
            if let Err(e2) = requeue(cfg.clone(), id.clone(), filler).await {
                tracing::error!(%id, err = %e2, "requeue after load failure also failed");
            }
            return;
        }
    };

    let sender = envelope
        .get("sender")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let recipient = envelope
        .get("recipient")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Read the raw RFC 5322 bytes. Prefer message_data_b64 so 8-bit
    // MIME (binary attachments, non-UTF-8 encodings) survives the
    // JSON round-trip; fall back to the legacy plaintext field for
    // backwards compatibility with in-flight items enqueued before
    // the base64 switch.
    let message_bytes: Vec<u8> =
        if let Some(b64) = envelope.get("message_data_b64").and_then(|v| v.as_str()) {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap_or_default()
        } else {
            envelope
                .get("message_data")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .as_bytes()
                .to_vec()
        };
    let attempts_prev = envelope
        .get("attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let last_attempt_at = envelope
        .get("last_attempt_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if sender.is_empty() || recipient.is_empty() || message_bytes.is_empty() {
        tracing::error!(%id, "envelope malformed (missing sender/recipient/message_data)");
        let _ = move_to_failed(cfg, id, "malformed envelope".into(), true).await;
        return;
    }

    // Enforce a soft per-item retry floor. If we picked this up too
    // soon, put it back and let another item run first.
    let now = now_secs();
    if attempts_prev > 0 && (now - last_attempt_at) < cfg.retry_min_secs {
        tracing::debug!(%id, attempts_prev, "retry floor not reached, requeuing");
        let _ = requeue(cfg.clone(), id, envelope).await;
        // Sleep briefly so the loop doesn't spin on a single retry-floor item.
        tokio::time::sleep(Duration::from_millis(cfg.poll_ms.max(500))).await;
        return;
    }

    tracing::info!(%id, %sender, %recipient, attempt = attempts_prev + 1, "delivering");
    match try_deliver(&cfg, &sender, &recipient, &message_bytes).await {
        Outcome::Delivered => {
            if let Err(e) = drop_blob(cfg, id.clone()).await {
                tracing::error!(%id, err = %e, "drop_blob after success failed");
            } else {
                tracing::info!(%id, "delivered + blob dropped");
            }
        }
        Outcome::Permanent(reason) => {
            tracing::warn!(%id, reason = %reason, "permanent — moving to failed");
            mailrs_fastcore::live_sync::audit_system("mail.send_failed", &recipient, &reason);
            enqueue_bounce_dsn(&cfg, &sender, &recipient, &reason, &message_bytes).await;
            if let Err(e) = move_to_failed(cfg, id.clone(), reason, true).await {
                tracing::error!(%id, err = %e, "move_to_failed after permanent failed");
            }
        }
        Outcome::Transient(reason) => {
            let attempts = attempts_prev + 1;
            if attempts >= cfg.max_attempts {
                tracing::warn!(
                    %id,
                    attempts,
                    reason = %reason,
                    "max attempts reached — moving to failed"
                );
                enqueue_bounce_dsn(&cfg, &sender, &recipient, &reason, &message_bytes).await;
                let _ = move_to_failed(cfg, id, reason, true).await;
                return;
            }
            let mut env = envelope;
            env["attempts"] = serde_json::Value::from(attempts);
            env["last_error"] = serde_json::Value::from(reason.clone());
            env["last_attempt_at"] = serde_json::Value::from(now_secs());
            tracing::info!(%id, attempts, %reason, "transient — requeue tail");
            if let Err(e) = requeue(cfg, id, env).await {
                tracing::error!(err = %e, "requeue after transient failed");
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Install the process-wide rustls crypto provider before any TLS
    // config is built (STARTTLS in try_deliver). Without this rustls
    // 0.23 panics on first use — same fix mailrs-receiver / mailrs-server
    // apply. .ok() because a second install is a no-op error we can
    // safely ignore in a single-binary process.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let cfg = Arc::new(Cfg::from_env());
    tracing::info!(
        helo = %cfg.helo,
        max_attempts = cfg.max_attempts,
        poll_ms = cfg.poll_ms,
        retry_min_secs = cfg.retry_min_secs,
        "mailrs-fastcore-sender starting"
    );

    // Fail fast on kevy connect so misconfig surfaces at boot.
    if let Err(e) = kevy(&cfg.kevy_url) {
        tracing::error!(err = %e, "kevy connect failed at boot — exiting");
        std::process::exit(2);
    }

    let mut consecutive_errors: u32 = 0;
    loop {
        // promote any scheduled sends whose time has arrived (G13)
        if let Err(e) = promote_due((*cfg).clone()).await {
            tracing::warn!(err = %e, "scheduled due-sweep failed");
        }
        match pop_next((*cfg).clone()).await {
            Ok(Some(id)) => {
                consecutive_errors = 0;
                process_one((*cfg).clone(), id).await;
            }
            Ok(None) => {
                tokio::time::sleep(Duration::from_millis(cfg.poll_ms)).await;
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                let back_ms = (cfg.poll_ms * (1 << consecutive_errors.min(6))).min(30_000);
                tracing::error!(err = %e, back_ms, "pop_next error — backing off");
                tokio::time::sleep(Duration::from_millis(back_ms)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_addr_spec;

    #[test]
    fn bare_addr_spec_passes_through() {
        assert_eq!(
            extract_addr_spec("nagata@nagatax.tokyo.jp"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn name_addr_extracts_inside_brackets() {
        assert_eq!(
            extract_addr_spec("Masato Nagata <nagata@nagatax.tokyo.jp>"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn quoted_display_name_supported() {
        assert_eq!(
            extract_addr_spec("\"Nagata, M.\" <nagata@nagatax.tokyo.jp>"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn trims_outer_whitespace() {
        assert_eq!(extract_addr_spec("  a@b.c  "), "a@b.c");
        assert_eq!(extract_addr_spec("  A <a@b.c>  "), "a@b.c");
    }
}
