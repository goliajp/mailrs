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
        }
    }
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

    let timeouts = TimeoutConfig::default();
    let mut last_err = String::from("no MX host attempted");

    for mx in &mx_records {
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
        if let Err(e) = conn.ehlo(&cfg.helo).await {
            last_err = format!("ehlo {}: {e}", mx.exchange);
            tracing::warn!(err = %last_err, "ehlo failed, next MX");
            continue;
        }

        // Opportunistic STARTTLS: on Rejected we stay plaintext (best
        // effort); on HandshakeFailed we treat as transient because the
        // TCP is dead.
        let conn = match conn.try_starttls(&mx.exchange).await {
            mailrs_smtp_client::StarttlsResult::Success(c) => {
                let mut c = c;
                if let Err(e) = c.ehlo(&cfg.helo).await {
                    last_err = format!("ehlo-after-starttls {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "post-tls ehlo failed, next MX");
                    continue;
                }
                c
            }
            mailrs_smtp_client::StarttlsResult::Rejected {
                conn,
                code,
                message: msg,
            } => {
                tracing::info!(code, %msg, "STARTTLS rejected, continuing plaintext");
                conn
            }
            mailrs_smtp_client::StarttlsResult::HandshakeFailed { source, .. } => {
                last_err = format!("starttls {}: {source}", mx.exchange);
                tracing::warn!(err = %last_err, "starttls handshake failed, next MX");
                continue;
            }
        };

        let mut conn = conn;
        let resp = match conn.deliver(sender, &[recipient], message).await {
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
    let message_data = envelope
        .get("message_data")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let attempts_prev = envelope
        .get("attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let last_attempt_at = envelope
        .get("last_attempt_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if sender.is_empty() || recipient.is_empty() || message_data.is_empty() {
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
    match try_deliver(&cfg, &sender, &recipient, message_data.as_bytes()).await {
        Outcome::Delivered => {
            if let Err(e) = drop_blob(cfg, id.clone()).await {
                tracing::error!(%id, err = %e, "drop_blob after success failed");
            } else {
                tracing::info!(%id, "delivered + blob dropped");
            }
        }
        Outcome::Permanent(reason) => {
            tracing::warn!(%id, reason = %reason, "permanent — moving to failed");
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
