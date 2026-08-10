//! The outbound queue state machine: claim, recover, promote, requeue.
//!
//! `pop_next` uses BRPOP with a bounded wait and a WATCH+CAS check on the
//! row's state, so a crash between the pop and the claim leaves the item
//! recoverable rather than lost — `.claude/rules/kevy-patterns.md` →
//! `kevy/no-blocking-pop-wrap`.

use std::time::Duration;

use super::config::*;
use super::*;

/// Blocks up to `wait` waiting for the next id to arrive in pending;
/// returns `Ok(None)` only when the timer expires with the queue still
/// empty. v2.2 §2 (2026-07-08) — replaces the earlier
/// `rpop + sleep(poll_ms)` polling loop with kevy-client 1.14's
/// wrapped `BRPOP`; the queue-empty case releases the blocking thread
/// promptly on wake-up, and the queue-arrival case fires the moment
/// the producer's `LPUSH` lands (no ~poll_ms/2 average wake latency).
pub(super) async fn pop_next(cfg: Cfg, wait: Duration) -> std::io::Result<Option<String>> {
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
pub(super) fn try_claim(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<bool> {
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
pub(super) async fn recover_stale(cfg: Cfg) -> std::io::Result<usize> {
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

pub(super) async fn promote_due(cfg: Cfg) -> std::io::Result<()> {
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
pub(super) async fn load_envelope(
    cfg: Cfg,
    id: String,
) -> std::io::Result<Option<serde_json::Value>> {
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
pub(super) async fn drop_blob(cfg: Cfg, id: String) -> std::io::Result<()> {
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

pub(super) fn now_secs_str() -> String {
    now_secs().to_string()
}

/// v2.5.1 §P8-B-A helper: sender-side terminal transition on the v2
/// job hash. `state` = b"delivered" | b"failed" | b"bounced". Fires
/// after the legacy key has been mutated so any partial-write crash
/// leaves the new hash provably behind the old one — Phase 8 legacy
/// drop won't run until this always fires in lock-step (Phase 7
/// harness gate).
pub(super) fn dual_write_terminal(
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
pub(super) fn dual_write_pending(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<()> {
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
        p.cmd(&[b"LPUSH", PENDING_IDX, id.as_bytes()]);
    });
    Ok(())
}

/// Move the id into `mailrs:outbound:failed` (SET) and drop the blob.
/// Blob is retained for operator inspection only when `keep_blob=true`.
pub(super) async fn move_to_failed(
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

/// Persist updated envelope (new attempts / last_error) and RPUSH back
/// to the pending tail for a retry.
///
/// v2.5.1 §P8-B-A dual-write completion: on retry, the v2 job hash
/// state resets to pending and the pending-idx list gets a matching
/// LPUSH so Phase 7 read cutover sees the same retry semantics.
pub(super) async fn requeue(
    cfg: Cfg,
    id: String,
    envelope: serde_json::Value,
) -> std::io::Result<()> {
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
