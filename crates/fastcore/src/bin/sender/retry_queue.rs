//! Jobs that are not due yet: future-dated sends on `scheduled-idx`, and
//! jobs waiting between attempts on `retry-idx`. Both are zsets scored by
//! the time the job becomes due, released onto `pending-idx` by one sweep.

use mailrs_core_sidestate::families::outbound::{RETRY_IDX, job_key};

use super::config::*;
use super::*;

/// Move every due member of both zsets onto `pending-idx`.
pub(super) async fn promote_due(cfg: Cfg) -> std::io::Result<()> {
    spawn_blocking(move || promote_due_on(&mut kevy(&cfg.kevy_url)?, now_secs()))
        .await
        .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

fn promote_due_on(c: &mut kevy_client::Connection, now: i64) -> std::io::Result<()> {
    for key in [SCHEDULED_KEY, RETRY_IDX] {
        // ascending by score; batch of 100 due items per tick is plenty
        for m in c.zrange(key, 0, 99)? {
            let score = c.zscore(key, &m)?.unwrap_or(f64::MAX);
            if score > now as f64 {
                break; // rest are future
            }
            // pending first, then remove from the zset — a crash between
            // the two re-promotes harmlessly (the claim CAS lets only one
            // copy through)
            c.lpush(PENDING_IDX_KEY, &[m.as_slice()])?;
            c.zrem(key, &[m.as_slice()])?;
        }
    }
    Ok(())
}

/// Park job `id` with its updated envelope until `due_at` (unix seconds).
///
/// The job stays `pending` so a stray copy of its id still on
/// `pending-idx` is claimable; that copy reaches the retry floor and is
/// parked again, which consumes it instead of putting it back.
pub(super) async fn schedule_retry(
    cfg: Cfg,
    id: String,
    envelope: serde_json::Value,
    due_at: i64,
) -> std::io::Result<()> {
    spawn_blocking(move || schedule_retry_on(&mut kevy(&cfg.kevy_url)?, &id, &envelope, due_at))
        .await
        .map_err(|e| std::io::Error::other(format!("join: {e}")))?
}

fn schedule_retry_on(
    c: &mut kevy_client::Connection,
    id: &str,
    envelope: &serde_json::Value,
    due_at: i64,
) -> std::io::Result<()> {
    let job_k = job_key(id);
    let payload = envelope.to_string();
    let now = now_secs_str();
    c.hset(
        job_k.as_bytes(),
        &[
            (b"blob" as &[u8], payload.as_bytes()),
            (b"state", b"pending"),
            (b"updated_at", now.as_bytes()),
        ],
    )?;
    c.hdel(job_k.as_bytes(), &[b"claimed_at"])?;
    c.zadd(RETRY_IDX, &[(due_at as f64, id.as_bytes())])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure this replaces: every early pick-up pushed the id back
    /// onto `pending-idx`, and the list grew to 59,211 entries for 18
    /// jobs. Parking the same job again and again must leave one entry.
    #[test]
    fn parking_a_job_repeatedly_keeps_one_entry_and_leaves_the_queue_alone() {
        let mut c = kevy("mem://retry-queue-park").unwrap();
        for _ in 0..5 {
            let env = serde_json::json!({"id": "7", "attempts": 3});
            schedule_retry_on(&mut c, "7", &env, now_secs() + 600).unwrap();
        }
        assert_eq!(c.zrange(RETRY_IDX, 0, -1).unwrap(), vec![b"7".to_vec()]);
        assert_eq!(c.llen(PENDING_IDX_KEY).unwrap(), 0);
        assert_eq!(
            c.hget(job_key(7).as_bytes(), b"state").unwrap().as_deref(),
            Some(&b"pending"[..])
        );
    }

    #[test]
    fn only_a_due_retry_is_released_onto_the_queue() {
        let mut c = kevy("mem://retry-queue-promote").unwrap();
        let env = serde_json::json!({});
        let now = now_secs();
        schedule_retry_on(&mut c, "1", &env, now - 5).unwrap();
        schedule_retry_on(&mut c, "2", &env, now + 600).unwrap();
        promote_due_on(&mut c, now).unwrap();
        assert_eq!(
            c.lrange(PENDING_IDX_KEY, 0, -1).unwrap(),
            vec![b"1".to_vec()]
        );
        assert_eq!(c.zrange(RETRY_IDX, 0, -1).unwrap(), vec![b"2".to_vec()]);
    }
}
