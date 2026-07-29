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

// v2.5.3 §P8-B-C: sender BRPOPs the v2 pending-idx now, not the
// legacy list. Enqueue still LPUSHes both (via core-sidestate) —
// Phase 8.2 drops the legacy write path. Duplicate ids in
// pending-idx (from Phase 6.2/7 LPUSH-without-RPOP semantics) are
// filtered by the WATCH+CAS state=pending check in `pop_next`.
const PENDING_IDX_KEY: &[u8] = b"mailrs:outbound:pending-idx";
const FAILED_KEY: &[u8] = b"mailrs:outbound:failed";

#[derive(Clone)]
struct Cfg {
    kevy_url: String,
    /// EHLO name announced on outbound sessions. Must match the PTR of
    /// the sending IP — receivers check forward-confirmed reverse DNS.
    helo: String,
    /// Domain used in `MAILER-DAEMON@…` on DSNs we originate. Distinct
    /// from `helo`: this one has to survive DMARC at the far end, so it
    /// must be a domain with an aligned DKIM key, not the MTA hostname.
    /// See `bounce::compose_dsn`.
    dsn_from_domain: String,
    max_attempts: u32,
    poll_ms: u64,
    retry_min_secs: i64,
    /// DKIM signing enabled when `MAILRS_DKIM_DOMAIN`,
    /// `MAILRS_DKIM_SELECTOR`, and `MAILRS_DKIM_PRIVATE_KEY_PEM_FILE`
    /// are all set. Public MX (Gmail / Outlook / etc.) drop unsigned
    /// mail from mailrs-hosted domains into spam.
    dkim: Option<Arc<DkimSignConfig>>,
    /// Signing key for ARC seals on forwarded mail. Same key and
    /// selector as DKIM — ARC verifiers look the public key up under
    /// `<selector>._domainkey.<domain>`, exactly where DKIM's already
    /// is, so sealing needs no new DNS.
    arc_key: Option<Arc<mailrs_dkim::RsaSigningKey>>,
}

/// Parse the DKIM private key once for ARC sealing.
///
/// Separate from `DkimSignConfig`'s lazily-parsed copy because that one
/// is private to the signer. Returns `None` when no key is configured,
/// which simply means forwards go out unsealed.
fn load_arc_key() -> Option<Arc<mailrs_dkim::RsaSigningKey>> {
    let path = std::env::var("MAILRS_DKIM_PRIVATE_KEY").ok()?;
    let pem = std::fs::read_to_string(&path).ok()?;
    match mailrs_dkim::RsaSigningKey::from_pkcs8_pem(&pem) {
        Ok(k) => Some(Arc::new(k)),
        Err(e) => {
            tracing::warn!(error = %e, "ARC: key unparseable; forwards will not be sealed");
            None
        }
    }
}

impl Cfg {
    fn from_env() -> Self {
        Self {
            kevy_url: std::env::var("MAILRS_KEVY_URL")
                .expect("MAILRS_KEVY_URL required (kevy://host:port)"),
            helo: std::env::var("MAILRS_HELO_HOSTNAME")
                .unwrap_or_else(|_| "mail.golia.jp".to_string()),
            dsn_from_domain: mailrs_fastcore::bounce::dsn_identity().1,
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
            arc_key: load_arc_key(),
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
    // Per-domain overrides from MAILRS_DKIM_KEYS. Without these every
    // outbound message signs with the default `d=`, which only aligns
    // for the default domain — every other hosted domain fails DMARC
    // the moment SPF stops covering it (i.e. on any forward).
    let extra_keys = mailrs_outbound_queue::dkim_env::extra_keys_from_env();
    if extra_keys.is_empty() {
        tracing::info!("DKIM: single-domain mode (MAILRS_DKIM_KEYS unset)");
    } else {
        let mut domains: Vec<&str> = extra_keys.keys().map(String::as_str).collect();
        domains.sort_unstable();
        tracing::info!(
            count = extra_keys.len(),
            domains = %domains.join(","),
            "DKIM: per-domain signing keys loaded"
        );
    }
    Some(Arc::new(DkimSignConfig {
        selector,
        domain,
        private_key_pem: pem,
        parsed_key: Arc::new(std::sync::OnceLock::new()),
        extra_keys,
    }))
}

/// Put a permanently-failed recipient on the suppression list.
///
/// Only genuine 5xx replies count. `Outcome::Permanent` also covers our
/// own refusals — malformed recipients, signing failures, and the
/// suppression check itself — and none of those are evidence about the
/// remote mailbox. See [`is_remote_hard_bounce`] for how the two are
/// told apart.
///
/// Best-effort: a message that already failed must not fail louder
/// because the side-state write did not land.
fn record_suppression(cfg: &Cfg, recipient: &str, reason: &str) {
    use mailrs_core_sidestate::families::suppression;

    if !is_remote_hard_bounce(reason) {
        return;
    }
    let Ok(mut conn) = kevy(&cfg.kevy_url) else {
        tracing::warn!(%recipient, "suppression: no kevy connection");
        return;
    };
    match suppression::add(
        &mut conn,
        recipient,
        suppression::Source::HardBounce,
        reason,
        now_secs(),
    ) {
        Ok(()) => tracing::info!(
            %recipient,
            "suppressed after hard bounce (expires in 90 days)"
        ),
        Err(e) => tracing::warn!(error = %e, %recipient, "suppression: add failed"),
    }
}

/// Record "this user has sent to this address" on the shared contact
/// hash. Best-effort: a delivery that already succeeded must never be
/// failed or retried because a derived counter could not be bumped.
fn record_sent_relationship(cfg: &Cfg, sender: &str, recipient: &str) {
    let from = sender.trim().to_lowercase();
    let to = recipient.trim().to_lowercase();
    if from.is_empty() || to.is_empty() {
        return;
    }
    let Ok(mut conn) = kevy(&cfg.kevy_url) else {
        tracing::warn!(%from, %to, "sent-relationship: no kevy connection");
        return;
    };
    if let Err(e) = mailrs_core_sidestate::families::contacts::record_sent_to(&mut conn, &from, &to)
    {
        tracing::warn!(error = %e, %from, %to, "sent-relationship: hincrby failed");
    }
}

fn kevy(url: &str) -> std::io::Result<kevy_client::Connection> {
    kevy_client::Connection::connect(url).map_err(std::io::Error::other)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Blocks up to `wait` waiting for the next id to arrive in pending;
/// returns `Ok(None)` only when the timer expires with the queue still
/// empty. v2.2 §2 (2026-07-08) — replaces the earlier
/// `rpop + sleep(poll_ms)` polling loop with kevy-client 1.14's
/// wrapped `BRPOP`; the queue-empty case releases the blocking thread
/// promptly on wake-up, and the queue-arrival case fires the moment
/// the producer's `LPUSH` lands (no ~poll_ms/2 average wake latency).
async fn pop_next(cfg: Cfg, wait: Duration) -> std::io::Result<Option<String>> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        // v2.5.3 §P8-B-C: BRPOP the v2 pending-idx. pending-idx may
        // contain duplicate ids left over from Phase 6.2/7 LPUSH-
        // without-RPOP semantics — the WATCH+CAS `state=pending` guard
        // below filters those: only the entry that finds state==pending
        // wins the CAS, others fall through and loop for the next id.
        //
        // BRPOP with the `wait` timer, then a short inner loop that
        // spends at most (poll_ms * 5) tolerating dup skip until the
        // outer main loop's brpop_wait window is more accurate. In
        // practice the pending-idx dup fraction shrinks fast once
        // Phase 8 lands, since we RPOP one entry per claim now.
        let Some((_key, id_bytes)) = c
            .brpop(&[PENDING_IDX_KEY], Some(wait))
            .map_err(std::io::Error::other)?
        else {
            return Ok(None);
        };
        let id = String::from_utf8_lossy(&id_bytes).to_string();
        if !try_claim(&mut c, &id)? {
            // Dup or already-terminal id — nothing to process. Return
            // Some so the outer loop calls process_one(id=this-id)?
            // No: process_one would call load_envelope which returns
            // None and short-circuits gracefully. Simpler to return
            // Some(id) and let process_one detect envelope absence.
            //
            // Actually cleaner: recurse-once by returning Ok(None) so
            // the main loop performs its own idle-back-off; a fresh
            // BRPOP will hit the next entry. Return None.
            return Ok(None);
        }
        Ok(Some(id))
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// v2.5.3 §P8-B-C: WATCH+HGET state → MULTI HSET state=inflight CAS
/// claim. Returns true if we won the CAS (state was pending, we now
/// own it), false if state was non-pending (dup entry or already-
/// terminal) or the WATCH aborted (another sender beat us).
fn try_claim(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<bool> {
    let job_k = format!("mailrs:outbound:job:{id}");
    c.watch(&[job_k.as_bytes()])
        .map_err(std::io::Error::other)?;
    let state = c
        .hget(job_k.as_bytes(), b"state")
        .map_err(std::io::Error::other)?
        .unwrap_or_default();
    if state != b"pending" {
        c.unwatch().map_err(std::io::Error::other)?;
        return Ok(false);
    }
    let now = now_secs_str();
    let mut tx = c.multi().map_err(std::io::Error::other)?;
    tx.queue(&[
        b"HSET",
        job_k.as_bytes(),
        b"state",
        b"inflight",
        b"claimed_at",
        now.as_bytes(),
    ])
    .map_err(std::io::Error::other)?;
    tx.queue(&[b"HINCRBY", job_k.as_bytes(), b"attempts", b"1"])
        .map_err(std::io::Error::other)?;
    match tx.exec_watched().map_err(std::io::Error::other)? {
        Some(_) => Ok(true),
        None => Ok(false), // another sender's WATCH won
    }
}

/// v2.5.2 §P8-B-B `recover_stale` (RFC §2.6). Walks every
/// `mailrs:outbound:job:*` hash, finds ones where `state==inflight` and
/// `now - claimed_at > STALE_SECS` (default 5 min) — those are ids a
/// prior sender BRPOPed and marked inflight but crashed before reaching
/// a terminal state. Re-enqueues each stale id back onto the legacy
/// pending list (so the next BRPOP picks it up) and resets state=pending
/// on the job hash. Idempotent + WATCH-guarded — a second sender doing
/// recover_stale in parallel will lose the CAS and drop through.
async fn recover_stale(cfg: Cfg) -> std::io::Result<usize> {
    spawn_blocking(move || {
        const STALE_SECS: i64 = 300;
        const SCAN_BATCH: usize = 200;
        let mut c = kevy(&cfg.kevy_url)?;
        let mut cursor = 0u64;
        let mut recovered = 0usize;
        loop {
            let (next, keys) = c
                .scan(cursor, Some(b"mailrs:outbound:job:*"), Some(SCAN_BATCH))
                .map_err(std::io::Error::other)?;
            for key in keys {
                let Ok(key_str) = std::str::from_utf8(&key) else {
                    continue;
                };
                let Some(id) = key_str.strip_prefix("mailrs:outbound:job:") else {
                    continue;
                };
                let id_owned = id.to_string();
                let state = c.hget(&key, b"state").map_err(std::io::Error::other)?;
                let claimed_at_bytes =
                    c.hget(&key, b"claimed_at").map_err(std::io::Error::other)?;
                let (Some(state), Some(claimed_at_bytes)) = (state, claimed_at_bytes) else {
                    continue;
                };
                if state != b"inflight" {
                    continue;
                }
                let Some(claimed_at) = std::str::from_utf8(&claimed_at_bytes)
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                else {
                    continue;
                };
                if now_secs() - claimed_at < STALE_SECS {
                    continue;
                }
                // Optimistic CAS: re-check state + claimed_at, then flip.
                c.watch(&[&key]).map_err(std::io::Error::other)?;
                let state_now = c
                    .hget(&key, b"state")
                    .map_err(std::io::Error::other)?
                    .unwrap_or_default();
                let claimed_now = c
                    .hget(&key, b"claimed_at")
                    .map_err(std::io::Error::other)?
                    .unwrap_or_default();
                if state_now != b"inflight" || claimed_now != claimed_at_bytes {
                    c.unwatch().map_err(std::io::Error::other)?;
                    continue;
                }
                let now = now_secs_str();
                let mut tx = c.multi().map_err(std::io::Error::other)?;
                tx.queue(&[
                    b"HSET",
                    &key,
                    b"state",
                    b"pending",
                    b"updated_at",
                    now.as_bytes(),
                ])
                .map_err(std::io::Error::other)?;
                tx.queue(&[b"HDEL", &key, b"claimed_at"])
                    .map_err(std::io::Error::other)?;
                // v2.5.3 §P8-B-C: only LPUSH the v2 pending-idx.
                // sender BRPOPs pending-idx now, not the legacy list.
                tx.queue(&[b"LPUSH", PENDING_IDX_KEY, id_owned.as_bytes()])
                    .map_err(std::io::Error::other)?;
                if tx.exec_watched().map_err(std::io::Error::other)?.is_some() {
                    recovered += 1;
                    tracing::info!(id = %id_owned, "recover_stale: reset inflight -> pending");
                }
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
        Ok(recovered)
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
        let members = c
            .zrange(SCHEDULED_KEY, 0, 99)
            .map_err(std::io::Error::other)?;
        for m in members {
            let score = c
                .zscore(SCHEDULED_KEY, &m)
                .map_err(std::io::Error::other)?
                .unwrap_or(f64::MAX);
            if score > now {
                break; // rest are future
            }
            // due: pending first, then remove from scheduled — a crash
            // between the two re-promotes harmlessly (idempotent).
            // v2.5.3 §P8-B-C: promote to v2 pending-idx directly.
            c.lpush(PENDING_IDX_KEY, &[m.as_slice()])
                .map_err(std::io::Error::other)?;
            c.zrem(SCHEDULED_KEY, &[m.as_slice()])
                .map_err(std::io::Error::other)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Fetch the envelope for `id`. Returns `Ok(None)` if blob missing.
///
/// v2.5.3 §P8-B-C: reads the v2 job hash's `blob` field (enqueue's
/// dual-write mirrors the same JSON there since Phase 6.2). Legacy
/// `mailrs:outbound:{id}` hash is no longer touched by sender.
async fn load_envelope(cfg: Cfg, id: String) -> std::io::Result<Option<serde_json::Value>> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let key = format!("mailrs:outbound:job:{id}");
        let blob = c
            .hget(key.as_bytes(), b"blob")
            .map_err(std::io::Error::other)?;
        let Some(bytes) = blob else { return Ok(None) };
        let v: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::other(format!("blob json: {e}")))?;
        Ok(Some(v))
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// Delete the blob for `id` (successful delivery or terminal failure).
///
/// v2.5.1 §P8-B-A dual-write completion (roadmap addendum discovered
/// via crash-test harness 2026-07-12): sender-side terminal
/// transitions must mirror on the v2 job hash so Phase 7 read
/// cutover (which reads state from `mailrs:outbound:job:{id}`) sees
/// a consistent view. `dual_write_terminal` sets state=delivered,
/// LPUSHes done-idx, and EXPIREs the job hash to 24 h.
async fn drop_blob(cfg: Cfg, id: String) -> std::io::Result<()> {
    let id_c = id.clone();
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        // v2.5.3 §P8-B-C: legacy `mailrs:outbound:{id}` DEL removed.
        // Enqueue still writes the legacy hash (Phase 8.2 will drop
        // that too), but sender no longer reads or deletes it.
        dual_write_terminal(&mut c, &id_c, b"delivered")?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

fn now_secs_str() -> String {
    now_secs().to_string()
}

/// v2.5.1 §P8-B-A helper: sender-side terminal transition on the v2
/// job hash. `state` = b"delivered" | b"failed" | b"bounced". Fires
/// after the legacy key has been mutated so any partial-write crash
/// leaves the new hash provably behind the old one — Phase 8 legacy
/// drop won't run until this always fires in lock-step (Phase 7
/// harness gate).
fn dual_write_terminal(
    c: &mut kevy_client::Connection,
    id: &str,
    state: &[u8],
) -> std::io::Result<()> {
    let job_k = format!("mailrs:outbound:job:{id}");
    let now = now_secs_str();
    let _ = c.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            state,
            b"updated_at",
            now.as_bytes(),
        ]);
        p.cmd(&[b"HDEL", job_k.as_bytes(), b"claimed_at"]);
        p.cmd(&[b"LPUSH", b"mailrs:outbound:done-idx", id.as_bytes()]);
        p.cmd(&[b"EXPIRE", job_k.as_bytes(), b"86400"]);
    });
    Ok(())
}

/// v2.5.1 §P8-B-A helper: sender-side retry (state=pending).
fn dual_write_pending(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<()> {
    let job_k = format!("mailrs:outbound:job:{id}");
    let now = now_secs_str();
    let _ = c.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            b"pending",
            b"updated_at",
            now.as_bytes(),
        ]);
        p.cmd(&[b"HDEL", job_k.as_bytes(), b"claimed_at"]);
        p.cmd(&[b"LPUSH", b"mailrs:outbound:pending-idx", id.as_bytes()]);
    });
    Ok(())
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
        // These two — the FAILED set + per-id `failed:{id}` audit hash
        // — are operator-inspection surfaces (webapi's failed queue
        // view), not part of the v2 job FSM. They stay independent of
        // the P8-B-C cutover.
        c.sadd(FAILED_KEY, &[id_c.as_bytes()])
            .map_err(std::io::Error::other)?;
        let audit_key = format!("mailrs:outbound:failed:{id_c}");
        c.hset(
            audit_key.as_bytes(),
            &[
                (b"failed_at" as &[u8], now_secs().to_string().as_bytes()),
                (b"reason", reason_c.as_bytes()),
            ],
        )
        .map_err(std::io::Error::other)?;
        // v2.5.3 §P8-B-C: legacy `mailrs:outbound:{id}` blob DEL removed.
        // `keep_blob` parameter is now moot; caller passes it for
        // backward-source-compat but the value has no effect.
        let _ = keep_blob;
        dual_write_terminal(&mut c, &id_c, b"failed")?;
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
        &cfg.dsn_from_domain,
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
        )
        .map_err(std::io::Error::other)?;
        c.lpush(mailrs_fastcore::bounce::BOUNCE_PENDING, &[id.as_bytes()])
            .map_err(std::io::Error::other)?;
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
///
/// v2.5.1 §P8-B-A dual-write completion: on retry, the v2 job hash
/// state resets to pending and the pending-idx list gets a matching
/// LPUSH so Phase 7 read cutover sees the same retry semantics.
async fn requeue(cfg: Cfg, id: String, envelope: serde_json::Value) -> std::io::Result<()> {
    let id_c = id.clone();
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        // v2.5.3 §P8-B-C: envelope blob now written to the v2 job hash,
        // not the legacy `mailrs:outbound:{id}` hash. `dual_write_pending`
        // handles state=pending + HDEL claimed_at + LPUSH pending-idx.
        let job_k = format!("mailrs:outbound:job:{id_c}");
        let payload = envelope.to_string();
        c.hset(job_k.as_bytes(), &[(b"blob" as &[u8], payload.as_bytes())])
            .map_err(std::io::Error::other)?;
        dual_write_pending(&mut c, &id_c)?;
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

/// Report one recipient's outcome onto the Send row (RFC
/// 20260730-send-status S2).
///
/// Called once, before the outcome is matched for queue handling, so
/// every arm is covered by construction. The match here is exhaustive:
/// a new `Outcome` variant fails to compile until it decides what the
/// user should see, which is the point — the previous design let the
/// view learn about a send from a periodic sweep, and a variant that
/// forgot to report would put us back there.
///
/// The remote's code and text are stored verbatim. When a send fails,
/// the receiving server's own words are the only useful thing on the
/// screen.
fn report_send_outcome(
    cfg: &Cfg,
    send_id: Option<&str>,
    user: &str,
    recipient: &str,
    outcome: &Outcome,
    attempts: u32,
) {
    use mailrs_core_sidestate::families::send as sendfam;
    let Some(send_id) = send_id else {
        // Mail nobody composed — tls-rpt, a bounce DSN, a sieve
        // redirect. There is no Send row to report against, and
        // inventing one would put mail in the user's Send list that they
        // never wrote.
        return;
    };
    let state = match outcome {
        Outcome::Delivered => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: true,
            pending: false,
            code: 250,
            message: String::new(),
        },
        Outcome::Permanent(reason) => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: false,
            pending: false,
            code: extract_code(reason),
            message: reason.clone(),
        },
        Outcome::Transient(reason) => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: false,
            // Retries left keeps the whole send `sending`; only the
            // final attempt is a verdict. Publishing "failed" while a
            // retry is pending invites resending mail that is about to
            // arrive.
            pending: attempts < cfg.max_attempts,
            code: extract_code(reason),
            message: reason.clone(),
        },
    };
    let url = cfg.kevy_url.clone();
    let user = user.to_string();
    let send_id = send_id.to_string();
    if let Err(e) = (|| -> std::io::Result<()> {
        let mut c = kevy(&url)?;
        sendfam::update_recipient(&mut c, &user, &send_id, &state)?;
        Ok(())
    })() {
        tracing::warn!(err = %e, %send_id, %recipient, "send row outcome write failed");
    }
}

/// Pull a three-digit SMTP code out of a reason string, or 0.
///
/// The reason is the remote's text as we received it; the code is
/// duplicated into its own field so a UI can colour by class without
/// parsing prose, while the prose stays intact.
fn extract_code(reason: &str) -> u16 {
    for token in reason.split_whitespace() {
        let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
        if digits.len() == 3
            && let Ok(code) = digits.parse::<u16>()
            && (200..=599).contains(&code)
        {
            return code;
        }
    }
    0
}

/// Whether a failure reason carries a remote 5xx rejection.
///
/// `outbound_queue::is_hard_bounce` tests whether the string *starts*
/// with a 5, which suits a bare SMTP reply. Our reasons are assembled
/// as `"{mx} {code} {text}"`, so the code never leads and that check
/// silently returned false for every real rejection — the suppression
/// list stayed empty through a live 550. Scan the tokens instead.
///
/// Looking for a standalone three-digit 5xx also excludes the failures
/// we generate ourselves — "invalid recipient", "dkim sign", the
/// suppression check — none of which say anything about the remote
/// mailbox. Enhanced codes like `5.1.1` are not three digits, so they
/// cannot trigger it either.
fn is_remote_hard_bounce(reason: &str) -> bool {
    reason.split_whitespace().any(|token| {
        token.len() == 3 && token.starts_with('5') && token.bytes().all(|b| b.is_ascii_digit())
    })
}

/// ARC-seal `message` when this delivery is a forward, else `None`.
///
/// Returns `None` on every non-applicable or failing path — an unsealed
/// forward still goes out. See `arc_seal`'s module docs.
fn arc_seal_target(
    cfg: &Cfg,
    sender: &str,
    original_sender: &str,
    message: &[u8],
) -> Option<Vec<u8>> {
    if !is_forward(sender, original_sender) {
        return None;
    }
    let key = cfg.arc_key.as_ref()?;
    let dkim = cfg.dkim.as_ref()?;
    mailrs_fastcore::arc_seal::seal_forwarded(message, key, &dkim.domain, &dkim.selector)
}

/// Whether this delivery is a forward rather than an original send.
///
/// The envelope sender is rewritten (SRS) when we forward on someone
/// else's behalf, so it stops matching the original. Equal values — and
/// the null sender on both sides, which is what system mail uses — mean
/// this is our own message.
fn is_forward(sender: &str, original_sender: &str) -> bool {
    let s = sender.trim().trim_matches(['<', '>']);
    let o = original_sender.trim().trim_matches(['<', '>']);
    !o.is_empty() && !s.is_empty() && !s.eq_ignore_ascii_case(o)
}

/// Pull the `d=` tag out of the DKIM-Signature header that signing just
/// prepended.
///
/// Diagnostics only — a `None` never gates anything. Scans a bounded
/// prefix because the signature is prepended, so the header is always
/// at the very start; that also keeps the tag split from wandering into
/// the body, where semicolons are just bytes.
fn signed_d_tag(signed: &[u8]) -> Option<String> {
    let head = &signed[..signed.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let sig = text.strip_prefix("DKIM-Signature:")?;
    // Header ends at the first bare CRLF (a folded continuation line
    // starts with whitespace, so it is not a terminator).
    let header_end = sig
        .match_indices("\r\n")
        .find(|(i, _)| !sig[i + 2..].starts_with([' ', '\t']))
        .map(|(i, _)| i)
        .unwrap_or(sig.len());
    for tag in sig[..header_end].split(';') {
        if let Some(v) = tag.trim().strip_prefix("d=") {
            return Some(v.trim().to_string());
        }
    }
    None
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
async fn try_deliver(
    cfg: &Cfg,
    sender: &str,
    recipient_raw: &str,
    message: &[u8],
    original_sender: &str,
) -> Outcome {
    let recipient = extract_addr_spec(recipient_raw);
    let sender = extract_addr_spec(sender);

    // ARC-seal first, so the DKIM signature covers a message that
    // already carries its ARC set.
    //
    // A forward is where the envelope sender differs from the original
    // sender — the SRS rewrite in enqueue_redirect produces exactly
    // that. Ordinary sends have the two equal, and system mail has both
    // as the null sender, so neither gets sealed.
    let sealed_msg;
    let message: &[u8] = match arc_seal_target(cfg, sender, original_sender, message) {
        Some(bytes) => {
            sealed_msg = bytes;
            &sealed_msg
        }
        None => message,
    };

    // DKIM sign if a signing key is configured. Fatal signing errors
    // are permanent (won't heal on retry): message data is malformed
    // or the private key is broken.
    let signed;
    let payload: &[u8] = match cfg.dkim.as_ref() {
        Some(dkim) => match dkim.sign(message) {
            Ok(bytes) => {
                signed = bytes;
                // Log the d= we actually signed with. Alignment failures
                // are invisible from this side — the message leaves
                // fine and only fails at the far end, on a forward,
                // weeks later. Recording the tag makes the question
                // "did this domain sign as itself?" answerable from
                // logs instead of from a round trip through a third
                // party's inbox.
                if let Some(d) = signed_d_tag(&signed) {
                    tracing::info!(dkim_d = %d, %sender, "signed");
                }
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

    // Suppression check. Repeatedly delivering to an address that hard-
    // bounced or complained is what costs sending reputation, so this
    // runs before any DNS or connection work. The reason string
    // deliberately does not start with a 5xx code — see
    // record_suppression, which would otherwise re-suppress on the way
    // back out.
    if let Ok(mut conn) = kevy(&cfg.kevy_url)
        && mailrs_core_sidestate::families::suppression::is_suppressed(&mut conn, recipient)
    {
        tracing::info!(%recipient, "suppressed recipient — not delivering");
        return Outcome::Permanent(format!("recipient on suppression list: {recipient}"));
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
    let original_sender = envelope
        .get("original_sender")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let outcome = try_deliver(&cfg, &sender, &recipient, &message_bytes, &original_sender).await;
    // Once, before the queue handling below, so every arm is covered by
    // construction rather than by remembering.
    report_send_outcome(
        &cfg,
        envelope.get("send_id").and_then(|v| v.as_str()),
        &sender,
        &recipient,
        &outcome,
        attempts_prev + 1,
    );
    match outcome {
        Outcome::Delivered => {
            // Relationship fact: the user has now sent to this address.
            // This is the only writer of `sent_count`, and without it
            // `is_mutual` / `has_sent_to` — the strongest inbound
            // importance signals — can never become true
            // (RFC 20260721-self-hosted-importance-ranking).
            record_sent_relationship(&cfg, &sender, &recipient);
            if let Err(e) = drop_blob(cfg, id.clone()).await {
                tracing::error!(%id, err = %e, "drop_blob after success failed");
            } else {
                tracing::info!(%id, "delivered + blob dropped");
            }
        }
        Outcome::Permanent(reason) => {
            tracing::warn!(%id, reason = %reason, "permanent — moving to failed");
            record_suppression(&cfg, &recipient, &reason);
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

    // v2.5.2 §P8-B-B boot hook: reset any inflight job hash that a prior
    // sender crashed on. Best-effort — a network kevy blip here shouldn't
    // block the boot loop, it just means the recovery happens on the
    // periodic sweep instead.
    match recover_stale((*cfg).clone()).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(recovered = n, "recover_stale at boot"),
        Err(e) => tracing::warn!(err = %e, "recover_stale at boot failed"),
    }

    // BRPOP timer bound. Short enough to promote_due at cadence, long
    // enough that idle brpop dominates wall-clock over the wake-up +
    // reissue overhead. 5 s makes a due-sweep miss at most 5 s from
    // wall-clock — the sender loop's scheduled-sweep resolution.
    // v2.2 §2 (2026-07-08): supersedes MAILRS_SENDER_POLL_MS as the
    // idle-cadence knob (poll_ms retained only for error backoff).
    let brpop_wait = Duration::from_secs(5);
    let mut consecutive_errors: u32 = 0;
    let mut last_stale_sweep = now_secs();
    loop {
        // v2.5.2 §P8-B-B: periodic recover_stale (every 60 s). Cheap
        // when the queue is small; guards against sender crashes that
        // happen between boots and leave orphan inflight jobs.
        if now_secs() - last_stale_sweep >= 60 {
            if let Err(e) = recover_stale((*cfg).clone()).await {
                tracing::warn!(err = %e, "periodic recover_stale failed");
            }
            last_stale_sweep = now_secs();
        }
        // promote any scheduled sends whose time has arrived (G13)
        if let Err(e) = promote_due((*cfg).clone()).await {
            tracing::warn!(err = %e, "scheduled due-sweep failed");
        }
        match pop_next((*cfg).clone(), brpop_wait).await {
            Ok(Some(id)) => {
                consecutive_errors = 0;
                process_one((*cfg).clone(), id).await;
            }
            Ok(None) => {
                // BRPOP timer expired with queue empty. Loop back
                // immediately — no sleep needed; the next brpop() will
                // itself block for up to `brpop_wait`.
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
    use super::{extract_addr_spec, extract_code, is_forward, is_remote_hard_bounce, signed_d_tag};

    /// The code is pulled out of the remote's own sentence, which is
    /// rarely code-first. Same reason `is_remote_hard_bounce` needed a
    /// token scan rather than `starts_with('5')`.
    #[test]
    fn a_code_is_found_anywhere_in_the_remotes_sentence() {
        assert_eq!(
            extract_code("gmail-smtp-in.l.google.com 550 5.1.1 does not exist."),
            550
        );
        assert_eq!(extract_code("451 4.7.1 greylisted, try later"), 451);
        // 5.1.1 is an enhanced status code, not a reply code, and must
        // not be mistaken for one.
        assert_eq!(extract_code("mx.example.com said: 5.1.1 unknown"), 0);
        assert_eq!(extract_code("connection timed out"), 0);
    }

    #[test]
    fn detects_a_5xx_inside_a_real_reason_string() {
        // Verbatim from a live rejection — the code is the second token,
        // which is why a starts_with('5') check found nothing.
        assert!(is_remote_hard_bounce(
            "gmail-smtp-in.l.google.com 550 5.1.1 The email account that you tried to reach does not exist."
        ));
        assert!(is_remote_hard_bounce(
            "mx.example.com 552 message too large"
        ));
    }

    #[test]
    fn ignores_transient_and_self_inflicted_failures() {
        assert!(!is_remote_hard_bounce("mx.example.com 450 try again later"));
        assert!(!is_remote_hard_bounce("invalid recipient: not-an-address"));
        assert!(!is_remote_hard_bounce("dkim sign: key unreadable"));
        assert!(!is_remote_hard_bounce(
            "recipient on suppression list: a@b.c"
        ));
        assert!(!is_remote_hard_bounce("no MX for example.com"));
    }

    #[test]
    fn an_enhanced_code_alone_does_not_count() {
        // 5.1.1 is not a three-digit token, so a reason carrying only
        // an enhanced code must not be read as a rejection.
        assert!(!is_remote_hard_bounce("failed with status 5.1.1"));
    }

    #[test]
    fn a_rewritten_envelope_sender_marks_a_forward() {
        assert!(is_forward(
            "SRS0=abc=xy=other.com=user@golia.jp",
            "user@other.com"
        ));
    }

    #[test]
    fn an_ordinary_send_is_not_a_forward() {
        assert!(!is_forward("a@golia.jp", "a@golia.jp"));
        assert!(!is_forward("A@Golia.JP", "a@golia.jp"));
        assert!(!is_forward("<a@golia.jp>", "a@golia.jp"));
    }

    #[test]
    fn system_mail_is_not_a_forward() {
        // DSNs enqueue with the null sender on both sides.
        assert!(!is_forward("<>", "<>"));
        assert!(!is_forward("", ""));
        assert!(!is_forward("a@golia.jp", ""));
    }

    #[test]
    fn reads_the_d_tag_from_a_signature() {
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256; d=bitreits.com; s=mail;\r\n\
                    \tbh=abc; b=xyz\r\nFrom: a@b.c\r\n\r\nbody";

        assert_eq!(signed_d_tag(msg).as_deref(), Some("bitreits.com"));
    }

    #[test]
    fn reads_the_d_tag_across_a_folded_header() {
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256;\r\n\
                    \ts=mail; d=doracawl.com; bh=abc;\r\n\tb=xyz\r\nFrom: a@b.c\r\n\r\nbody";

        assert_eq!(signed_d_tag(msg).as_deref(), Some("doracawl.com"));
    }

    #[test]
    fn stops_at_the_end_of_the_signature_header() {
        // A `d=` later in the message must not be picked up.
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256; s=mail; b=xyz\r\n\
                    From: a@b.c\r\n\r\nbody with d=not-a-domain; in it";

        assert_eq!(signed_d_tag(msg), None);
    }

    #[test]
    fn unsigned_message_has_no_tag() {
        assert_eq!(signed_d_tag(b"From: a@b.c\r\n\r\nbody"), None);
        assert_eq!(signed_d_tag(b""), None);
    }

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
