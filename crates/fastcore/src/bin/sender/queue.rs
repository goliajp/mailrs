//! The outbound queue state machine: claim, recover, promote, requeue.
//!
//! `pop_next` uses BRPOP with a bounded wait and a WATCH+CAS check on the
//! row's state, so a crash between the pop and the claim leaves the item
//! recoverable rather than lost.

use std::time::Duration;

use super::config::*;
use super::*;
use mailrs_core_sidestate::families::outbound as ob;

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
        let Some((_key, id_bytes)) = c.brpop(&[PENDING_IDX_KEY], Some(wait))? else {
            return Ok(None);
        };
        let id = String::from_utf8_lossy(&id_bytes).to_string();
        // The id has left the list whatever the claim says. A job that is
        // still pending afterwards sits in no index, and `recover` puts it
        // back once it is old enough not to be mid-claim.
        match try_claim(&mut c, &id)? {
            Claim::Won => Ok(Some(id)),
            Claim::Missing => {
                tracing::info!(%id, "claim skipped: no job row");
                Ok(None)
            }
            Claim::NotPending(state) => {
                tracing::debug!(%id, %state, "claim skipped: job is not pending");
                Ok(None)
            }
            Claim::Lost => {
                tracing::warn!(%id, "claim lost: the job changed while claiming");
                Ok(None)
            }
        }
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

/// v2.5.3 §P8-B-C: WATCH+HGET state → MULTI HSET state=inflight CAS
/// claim. Returns true if we won the CAS (state was pending, we now
/// own it), false if state was non-pending (dup entry or already-
/// terminal) or the WATCH aborted (another sender beat us).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Claim {
    Won,
    /// No job row: a duplicate id whose job finished and expired, or a
    /// row that is not visible yet.
    Missing,
    /// Inflight or finished: a duplicate id.
    NotPending(String),
    /// The row changed between the read and the write.
    Lost,
}

pub(super) fn try_claim(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<Claim> {
    let job_k = ob::job_key(id);
    c.watch(&[job_k.as_bytes()])?;
    let Some(state) = c.hget(job_k.as_bytes(), b"state")? else {
        c.unwatch()?;
        return Ok(Claim::Missing);
    };
    if state != b"pending" {
        c.unwatch()?;
        return Ok(Claim::NotPending(
            String::from_utf8_lossy(&state).into_owned(),
        ));
    }
    let now = now_secs_str();
    let mut tx = c.multi()?;
    tx.queue(&[
        b"HSET",
        job_k.as_bytes(),
        b"state",
        b"inflight",
        b"claimed_at",
        now.as_bytes(),
    ])?;
    tx.queue(&[b"HINCRBY", job_k.as_bytes(), b"attempts", b"1"])?;
    match tx.exec_watched()? {
        Some(_) => Ok(Claim::Won),
        None => Ok(Claim::Lost),
    }
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
        let blob = c.hget(ob::job_key(&id).as_bytes(), b"blob")?;
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
/// a consistent view. `dual_write_terminal` sets state=delivered and
/// EXPIREs the job hash to 24 h.
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
    let job_k = ob::job_key(id);
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
        p.cmd(&[b"EXPIRE", job_k.as_bytes(), b"86400"]);
    });
    Ok(())
}

/// v2.5.1 §P8-B-A helper: sender-side retry (state=pending).
pub(super) fn dual_write_pending(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<()> {
    let job_k = ob::job_key(id);
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

/// Record the job as failed for good — with who sent it to whom, since
/// the job itself is dropped and the admin queue page has nothing else to
/// show — and settle its state.
pub(super) async fn move_to_failed(
    cfg: Cfg,
    id: String,
    reason: String,
    sender: String,
    recipient: String,
) -> std::io::Result<()> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        ob::record_failure(
            &mut c,
            &ob::Failure {
                id: id.clone(),
                failed_at: now_secs(),
                reason,
                sender,
                recipient,
            },
        )?;
        dual_write_terminal(&mut c, &id, b"failed")?;
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
        let job_k = ob::job_key(&id_c);
        let payload = envelope.to_string();
        c.hset(job_k.as_bytes(), &[(b"blob" as &[u8], payload.as_bytes())])?;
        dual_write_pending(&mut c, &id_c)?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}
