//! Putting lost jobs back on the queue: once at boot and every minute.
//!
//! Two ways a job can be left where nothing will pick it up:
//! - claimed (`inflight`) by a sender that died before finishing it;
//! - still `pending` but in no index: its id was popped and the claim did
//!   not take, or the id never reached `pending-idx`.
//!
//! Each is put back only after `STALE_SECS` without a change, so a job a
//! live sender is claiming or sending is left alone. A second sender
//! doing the same sweep loses the WATCH and drops through; an id pushed
//! twice is claimed once, as the claim checks the state.
//!
//! A job left claimed by a dead sender [`INTERRUPTED_LIMIT`] times is the
//! job that stops it (a message its scan or signing cannot get through),
//! and is set aside instead of being put back every five minutes.

use super::config::*;
use super::*;
use mailrs_core_sidestate::families::outbound as ob;

const STALE_SECS: i64 = 300;

pub(super) async fn recover_stale(cfg: Cfg) -> std::io::Result<usize> {
    spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        recover_on(&mut c, now_secs())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

pub(super) fn recover_on(c: &mut kevy_client::Connection, now: i64) -> std::io::Result<usize> {
    const SCAN_BATCH: usize = 200;
    let mut cursor = 0u64;
    let mut recovered = 0usize;
    loop {
        let (next, keys) = c.scan(cursor, Some(ob::JOB_PATTERN), Some(SCAN_BATCH))?;
        for key in keys {
            let Some(id) = std::str::from_utf8(&key)
                .ok()
                .and_then(|k| k.strip_prefix(ob::JOB_PREFIX))
                .map(str::to_string)
            else {
                continue;
            };
            let recovered_here = match c.hget(&key, b"state")?.as_deref() {
                Some(b"inflight") => reset_stale_inflight(c, &key, &id, now)?,
                Some(b"pending") => requeue_unindexed(c, &key, &id, now)?,
                _ => false,
            };
            if recovered_here {
                recovered += 1;
            }
        }
        if next == 0 {
            break;
        }
        cursor = next;
    }
    Ok(recovered)
}

fn field_secs(
    c: &mut kevy_client::Connection,
    key: &[u8],
    field: &[u8],
) -> std::io::Result<Option<i64>> {
    Ok(c.hget(key, field)?
        .and_then(|v| std::str::from_utf8(&v).ok().and_then(|s| s.parse().ok())))
}

fn reset_stale_inflight(
    c: &mut kevy_client::Connection,
    key: &[u8],
    id: &str,
    now: i64,
) -> std::io::Result<bool> {
    let Some(claimed_at_bytes) = c.hget(key, b"claimed_at")? else {
        return Ok(false);
    };
    let Some(claimed_at) = std::str::from_utf8(&claimed_at_bytes)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
    else {
        return Ok(false);
    };
    if now - claimed_at < STALE_SECS {
        return Ok(false);
    }
    c.watch(&[key])?;
    let state_now = c.hget(key, b"state")?.unwrap_or_default();
    let claimed_now = c.hget(key, b"claimed_at")?.unwrap_or_default();
    if state_now != b"inflight" || claimed_now != claimed_at_bytes {
        c.unwatch()?;
        return Ok(false);
    }
    let interrupted = field_secs(c, key, b"interrupted")?.unwrap_or(0) + 1;
    let now_s = now.to_string();
    let interrupted_s = interrupted.to_string();
    let give_up = interrupted >= i64::from(INTERRUPTED_LIMIT);
    let mut tx = c.multi()?;
    let state: &[u8] = match give_up {
        true => SET_ASIDE_STATE,
        false => b"pending",
    };
    tx.queue(&[
        b"HSET",
        key,
        b"state",
        state,
        b"updated_at",
        now_s.as_bytes(),
        b"interrupted",
        interrupted_s.as_bytes(),
    ])?;
    tx.queue(&[b"HDEL", key, b"claimed_at"])?;
    match give_up {
        true => tx.queue(&[b"ZADD", ob::SET_ASIDE_IDX, now_s.as_bytes(), id.as_bytes()])?,
        false => tx.queue(&[b"LPUSH", PENDING_IDX_KEY, id.as_bytes()])?,
    }
    let done = tx.exec_watched()?.is_some();
    match (done, give_up) {
        (true, true) => report_set_aside(c, key, id, interrupted, now)?,
        (true, false) => tracing::info!(%id, interrupted, "recover: reset inflight -> pending"),
        (false, _) => {}
    }
    Ok(done)
}

/// Claims of one job that a dead sender left behind before the job is set
/// aside. A deploy or an unrelated crash can leave an innocent job claimed
/// once; the same job three times running is the job.
pub(super) const INTERRUPTED_LIMIT: u32 = 3;

/// The state of a job set aside after [`INTERRUPTED_LIMIT`] dead claims.
/// Kept, with its message, for a person; nothing claims or expires it. To
/// send it again: `HSET <job> state pending` and `LPUSH` its id onto the
/// pending index.
pub(super) const SET_ASIDE_STATE: &[u8] = b"set_aside";

/// The log line and the failed-jobs record the admin queue page reads.
/// No DSN: composing one quotes the message, and reading the message is
/// what stopped the sender.
fn report_set_aside(
    c: &mut kevy_client::Connection,
    key: &[u8],
    id: &str,
    interrupted: i64,
    now: i64,
) -> std::io::Result<()> {
    let blob: serde_json::Value = c
        .hget(key, b"blob")?
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let field = |name: &str| blob[name].as_str().unwrap_or_default().to_string();
    let reason = format!("set aside: the sender stopped while sending it {interrupted} times");
    tracing::error!(event = "outbound_job_set_aside", %id, interrupted,
        sender = %field("sender"), recipient = %field("recipient"), "{reason}");
    ob::record_failure(
        c,
        &ob::Failure {
            id: id.to_string(),
            failed_at: now,
            reason,
            sender: field("sender"),
            recipient: field("recipient"),
        },
    )
}

/// A pending job is waiting in exactly one place: `pending-idx`, or
/// `retry-idx` / `scheduled-idx` until it is due.
fn requeue_unindexed(
    c: &mut kevy_client::Connection,
    key: &[u8],
    id: &str,
    now: i64,
) -> std::io::Result<bool> {
    let changed = match field_secs(c, key, b"updated_at")? {
        Some(t) => Some(t),
        None => field_secs(c, key, b"created_at")?,
    };
    let Some(changed) = changed else {
        return Ok(false);
    };
    if now - changed < STALE_SECS {
        return Ok(false);
    }
    if c.zscore(ob::RETRY_IDX, id.as_bytes())?.is_some()
        || c.zscore(ob::SCHEDULED_IDX, id.as_bytes())?.is_some()
        || c.lrange(PENDING_IDX_KEY, 0, -1)?
            .iter()
            .any(|m| m == id.as_bytes())
    {
        return Ok(false);
    }
    c.watch(&[key])?;
    if c.hget(key, b"state")?.as_deref() != Some(b"pending") {
        c.unwatch()?;
        return Ok(false);
    }
    let now_s = now.to_string();
    let mut tx = c.multi()?;
    tx.queue(&[b"HSET", key, b"updated_at", now_s.as_bytes()])?;
    tx.queue(&[b"LPUSH", PENDING_IDX_KEY, id.as_bytes()])?;
    let done = tx.exec_watched()?.is_some();
    if done {
        tracing::warn!(%id, "recover: pending job was in no index, queued again");
    }
    Ok(done)
}

/// WATCH / MULTI, which the in-process store lacks, so against a
/// kevy-server.
#[cfg(test)]
mod tests {
    use super::*;

    async fn server() -> (
        testcontainers::ContainerAsync<testcontainers::GenericImage>,
        kevy_client::Connection,
    ) {
        let (container, url) = mailrs_test_docker::kevy::kevy_server().await;
        let c = kevy(&url).unwrap();
        (container, c)
    }

    fn job(c: &mut kevy_client::Connection, id: &str, fields: &[(&[u8], &str)]) {
        let pairs: Vec<(&[u8], &[u8])> = fields.iter().map(|(k, v)| (*k, v.as_bytes())).collect();
        c.hset(ob::job_key(id).as_bytes(), &pairs).unwrap();
    }

    fn pending_ids(c: &mut kevy_client::Connection) -> Vec<String> {
        let mut ids: Vec<String> = c
            .lrange(PENDING_IDX_KEY, 0, -1)
            .unwrap()
            .into_iter()
            .map(|m| String::from_utf8(m).unwrap())
            .collect();
        ids.sort();
        ids
    }

    /// Job 352 on 2026-10-24: popped, never claimed, left pending in no
    /// index for two days until a kevy-server restart replayed its push.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pending_job_in_no_index_is_queued_again_once_it_is_old() {
        let (_kevy, mut c) = server().await;
        let now = 10_000;
        job(
            &mut c,
            "1",
            &[
                (b"state", "pending"),
                (b"created_at", "1000"),
                (b"updated_at", "1000"),
            ],
        );
        // just enqueued and being claimed: left alone
        job(
            &mut c,
            "2",
            &[(b"state", "pending"), (b"updated_at", "9900")],
        );
        // waiting where it should be
        job(
            &mut c,
            "3",
            &[(b"state", "pending"), (b"updated_at", "1000")],
        );
        c.lpush(PENDING_IDX_KEY, &[b"3"]).unwrap();
        job(
            &mut c,
            "4",
            &[(b"state", "pending"), (b"updated_at", "1000")],
        );
        c.zadd(ob::RETRY_IDX, &[(20_000.0, b"4" as &[u8])]).unwrap();
        job(
            &mut c,
            "5",
            &[(b"state", "pending"), (b"updated_at", "1000")],
        );
        c.zadd(ob::SCHEDULED_IDX, &[(20_000.0, b"5" as &[u8])])
            .unwrap();
        // finished: not a candidate
        job(
            &mut c,
            "6",
            &[(b"state", "delivered"), (b"updated_at", "1000")],
        );

        assert_eq!(recover_on(&mut c, now).unwrap(), 1);
        assert_eq!(pending_ids(&mut c), vec!["1", "3"]);
        // a second sweep finds it waiting and does nothing
        assert_eq!(recover_on(&mut c, now).unwrap(), 0);
        assert_eq!(pending_ids(&mut c), vec!["1", "3"]);
        // and it can be claimed
        assert_eq!(try_claim(&mut c, "1").unwrap(), Claim::Won);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_job_a_dead_sender_claimed_is_reset_after_the_stale_window() {
        let (_kevy, mut c) = server().await;
        job(
            &mut c,
            "7",
            &[(b"state", "inflight"), (b"claimed_at", "1000")],
        );
        job(
            &mut c,
            "8",
            &[(b"state", "inflight"), (b"claimed_at", "9900")],
        );
        assert_eq!(recover_on(&mut c, 10_000).unwrap(), 1);
        assert_eq!(pending_ids(&mut c), vec!["7"]);
        assert_eq!(
            c.hget(ob::job_key("7").as_bytes(), b"state")
                .unwrap()
                .as_deref(),
            Some(&b"pending"[..])
        );
    }

    /// A sender that dies on the same job every time it claims it: put back
    /// twice, then set aside and reported, never queued a fourth time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_job_a_sender_died_on_three_times_is_set_aside() {
        let (_kevy, mut c) = server().await;
        let blob = r#"{"sender":"a@x.test","recipient":"b@y.test"}"#;
        job(
            &mut c,
            "9",
            &[(b"state", "pending"), (b"blob", blob), (b"updated_at", "0")],
        );
        let mut now = 10_000;
        for round in 1..=3 {
            // claimed, and the sender died holding it
            assert_eq!(try_claim(&mut c, "9").unwrap(), Claim::Won, "round {round}");
            c.hset(
                ob::job_key("9").as_bytes(),
                &[(b"claimed_at" as &[u8], now.to_string().as_bytes())],
            )
            .unwrap();
            // popped by the sender that died
            let _ = c.rpop(PENDING_IDX_KEY, 100);
            now += 1_000;
            assert_eq!(recover_on(&mut c, now).unwrap(), 1, "round {round}");
        }
        let state = c.hget(ob::job_key("9").as_bytes(), b"state").unwrap();
        assert_eq!(state.as_deref(), Some(&b"set_aside"[..]));
        assert!(
            pending_ids(&mut c).is_empty(),
            "set aside but queued again: {:?}",
            pending_ids(&mut c)
        );
        assert_eq!(c.zcard(ob::SET_ASIDE_IDX).unwrap(), 1);
        let failed = ob::failures(&mut c).unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].recipient, "b@y.test");
        assert!(failed[0].reason.contains("3 times"), "{}", failed[0].reason);
        // later sweeps leave it alone
        assert_eq!(recover_on(&mut c, now + 10_000).unwrap(), 0);
        assert!(pending_ids(&mut c).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_claim_says_why_it_did_not_take() {
        let (_kevy, mut c) = server().await;
        assert_eq!(try_claim(&mut c, "40").unwrap(), Claim::Missing);
        job(&mut c, "41", &[(b"state", "delivered")]);
        assert_eq!(
            try_claim(&mut c, "41").unwrap(),
            Claim::NotPending("delivered".into())
        );
        job(&mut c, "42", &[(b"state", "pending")]);
        assert_eq!(try_claim(&mut c, "42").unwrap(), Claim::Won);
        assert_eq!(
            try_claim(&mut c, "42").unwrap(),
            Claim::NotPending("inflight".into())
        );
    }
}
