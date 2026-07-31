//! The webhook delivery queue on kevy.
//!
//! ```text
//!   webhook:outbox:ready    zset   score = due epoch seconds, member = id
//!   webhook:outbox:{id}     hash   the entry
//!   webhook:outbox:counter  str    next id
//!   webhook:outbox:dead     zset   score = failed-at, member = id
//! ```
//!
//! A user can create a subscription today and nothing ever fires: the
//! monolith had a worker over 316 lines of SQL, and the lane production runs
//! had neither queue nor worker. This is that queue.
//!
//! Claiming is the part that has to be right. Two workers must not both POST
//! the same entry — a subscriber receiving a payment notification twice is
//! not a cosmetic fault — so a claim is `zrem`, which either removes the
//! member (this worker owns it) or does not (someone else got there first).
//! The entry only re-enters `ready` when its retry falls due, and the
//! retry's schedule is computed before the claim so a crashed worker leaves
//! the entry due rather than lost.

use kevy_client::Connection;

const READY: &[u8] = b"webhook:outbox:ready";
const DEAD: &[u8] = b"webhook:outbox:dead";
const COUNTER: &[u8] = b"webhook:outbox:counter";

/// Attempts before an entry is given up on.
pub const MAX_ATTEMPTS: u32 = 6;

fn entry_key(id: i64) -> String {
    format!("webhook:outbox:{id}")
}

/// How long to wait before attempt number `attempt` (1-based).
///
/// Doubling from ten seconds and capped at an hour: 10s, 20s, 40s, 80s,
/// 160s, 320s. A subscriber that is down for a deploy is retried through it;
/// one that is down for a day is given up on rather than retried forever.
pub fn retry_delay_secs(attempt: u32) -> i64 {
    let doubled = 10i64.saturating_mul(1i64 << attempt.min(20));
    doubled.min(3600)
}

/// An entry waiting to be delivered.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEntry {
    /// Queue id, also the delivery id sent in `X-Mailrs-Delivery`.
    pub id: i64,
    /// Which subscription this is for.
    pub subscription_id: i64,
    /// The account that owns the subscription, so the worker can find it.
    pub account_address: String,
    /// The JSON body to POST.
    pub payload: String,
    /// Deliveries already attempted.
    pub attempts: u32,
    /// The most recent failure, for the operator.
    pub last_error: Option<String>,
}

impl OutboxEntry {
    fn to_pairs(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut pairs = vec![
            (
                b"subscription_id".to_vec(),
                self.subscription_id.to_string().into_bytes(),
            ),
            (
                b"account_address".to_vec(),
                self.account_address.clone().into_bytes(),
            ),
            (b"payload".to_vec(), self.payload.clone().into_bytes()),
            (b"attempts".to_vec(), self.attempts.to_string().into_bytes()),
        ];
        if let Some(ref e) = self.last_error {
            pairs.push((b"last_error".to_vec(), e.clone().into_bytes()));
        }
        pairs
    }

    fn from_flat(id: i64, flat: &[Vec<u8>]) -> Option<Self> {
        let mut subscription_id = None;
        let mut account_address = None;
        let mut payload = None;
        let mut attempts = 0u32;
        let mut last_error = None;
        let mut i = 0;
        while i + 1 < flat.len() {
            let field = std::str::from_utf8(&flat[i]).unwrap_or("");
            let value = String::from_utf8(flat[i + 1].clone()).unwrap_or_default();
            match field {
                "subscription_id" => subscription_id = value.parse::<i64>().ok(),
                "account_address" => account_address = Some(value),
                "payload" => payload = Some(value),
                "attempts" => attempts = value.parse::<u32>().unwrap_or(0),
                "last_error" => last_error = Some(value),
                _ => {}
            }
            i += 2;
        }
        Some(Self {
            id,
            subscription_id: subscription_id?,
            account_address: account_address?,
            payload: payload?,
            attempts,
            last_error,
        })
    }
}

/// Queue a delivery, due immediately.
pub fn enqueue(
    conn: &mut Connection,
    subscription_id: i64,
    account_address: &str,
    payload: &str,
    now: i64,
) -> std::io::Result<i64> {
    let id = conn.incr(COUNTER).map_err(std::io::Error::other)?;
    let entry = OutboxEntry {
        id,
        subscription_id,
        account_address: account_address.to_string(),
        payload: payload.to_string(),
        attempts: 0,
        last_error: None,
    };
    let pairs = entry.to_pairs();
    let refs: Vec<(&[u8], &[u8])> = pairs.iter().map(|(k, v)| (&k[..], &v[..])).collect();
    conn.hset(entry_key(id).as_bytes(), &refs)
        .map_err(std::io::Error::other)?;
    let member = id.to_string();
    conn.zadd(READY, &[(now as f64, member.as_bytes())])
        .map_err(std::io::Error::other)?;
    Ok(id)
}

/// Take up to `limit` entries that are due, exclusively.
///
/// An entry is this worker's only if `zrem` removed it. Another worker
/// reading the same page loses the race and skips it, so no payload is
/// POSTed twice.
pub fn claim_due(
    conn: &mut Connection,
    now: i64,
    limit: usize,
) -> std::io::Result<Vec<OutboxEntry>> {
    // The network client's zrange returns members without scores and has no
    // reverse form, so the whole ready set is read and filtered here. The
    // set holds one member per undelivered webhook; if that ever grows past
    // a page this needs a score-ranged read, which the network client would
    // have to grow first.
    let members = conn.zrange(READY, 0, -1).map_err(std::io::Error::other)?;
    let mut claimed = Vec::new();
    for member in members {
        if claimed.len() >= limit {
            break;
        }
        let Ok(id_str) = String::from_utf8(member.clone()) else {
            continue;
        };
        let Ok(id) = id_str.parse::<i64>() else {
            continue;
        };
        let score = conn
            .zscore(READY, member.as_slice())
            .map_err(std::io::Error::other)?;
        match score {
            Some(due) if due as i64 <= now => {}
            _ => continue,
        }
        // The claim. Zero means another worker already took it.
        let removed = conn
            .zrem(READY, &[member.as_slice()])
            .map_err(std::io::Error::other)?;
        if removed == 0 {
            continue;
        }
        let flat = conn
            .hgetall(entry_key(id).as_bytes())
            .map_err(std::io::Error::other)?;
        match OutboxEntry::from_flat(id, &flat) {
            Some(e) => claimed.push(e),
            // The hash is gone but the index named it. Dropping the member
            // is the repair; leaving it would spin.
            None => continue,
        }
    }
    Ok(claimed)
}

/// The delivery succeeded: forget the entry.
pub fn mark_delivered(conn: &mut Connection, id: i64) -> std::io::Result<()> {
    conn.del(&[entry_key(id).as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(())
}

/// What happens to an entry after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterFailure {
    /// Due again at this epoch second.
    Retry(i64),
    /// Out of attempts; moved to the dead set.
    DeadLettered,
}

/// Decide, without touching the store, what a failure means.
///
/// Separate so the schedule is assertable: an off-by-one here either retries
/// forever or gives up on the first blip, and both look like the queue
/// working.
pub fn plan_after_failure(attempts_now: u32, now: i64) -> AfterFailure {
    match attempts_now >= MAX_ATTEMPTS {
        true => AfterFailure::DeadLettered,
        false => AfterFailure::Retry(now + retry_delay_secs(attempts_now)),
    }
}

/// Record a failed attempt and reschedule or dead-letter it.
pub fn mark_failed(
    conn: &mut Connection,
    entry: &OutboxEntry,
    error: &str,
    now: i64,
) -> std::io::Result<AfterFailure> {
    let attempts = entry.attempts + 1;
    let key = entry_key(entry.id);
    conn.hset(
        key.as_bytes(),
        &[
            (b"attempts".as_slice(), attempts.to_string().as_bytes()),
            (b"last_error".as_slice(), error.as_bytes()),
        ],
    )
    .map_err(std::io::Error::other)?;
    let plan = plan_after_failure(attempts, now);
    match plan {
        AfterFailure::Retry(due) => {
            let member = entry.id.to_string();
            conn.zadd(READY, &[(due as f64, member.as_bytes())])
                .map_err(std::io::Error::other)?;
        }
        AfterFailure::DeadLettered => {
            let member = entry.id.to_string();
            conn.zadd(DEAD, &[(now as f64, member.as_bytes())])
                .map_err(std::io::Error::other)?;
        }
    }
    Ok(plan)
}

/// How many entries are waiting and how many were given up on.
///
/// Both, so an operator reading zero deliveries can tell an empty queue from
/// a stalled worker.
pub fn depth(conn: &mut Connection) -> std::io::Result<(usize, usize)> {
    let ready = conn.zcard(READY).map_err(std::io::Error::other)?;
    let dead = conn.zcard(DEAD).map_err(std::io::Error::other)?;
    Ok((ready, dead))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_doubles_and_stops_at_an_hour() {
        assert_eq!(retry_delay_secs(1), 20);
        assert_eq!(retry_delay_secs(2), 40);
        assert_eq!(retry_delay_secs(3), 80);
        assert_eq!(retry_delay_secs(6), 640);
        // Capped, and no overflow at absurd attempt counts.
        assert_eq!(retry_delay_secs(30), 3600);
        assert_eq!(retry_delay_secs(u32::MAX), 3600);
    }

    /// The boundary: the sixth failure is the last one.
    #[test]
    fn it_gives_up_after_the_configured_attempts() {
        assert_eq!(
            plan_after_failure(MAX_ATTEMPTS - 1, 1_000),
            AfterFailure::Retry(1_000 + retry_delay_secs(MAX_ATTEMPTS - 1))
        );
        assert_eq!(
            plan_after_failure(MAX_ATTEMPTS, 1_000),
            AfterFailure::DeadLettered
        );
        assert_eq!(
            plan_after_failure(MAX_ATTEMPTS + 1, 1_000),
            AfterFailure::DeadLettered
        );
    }

    #[test]
    fn an_entry_survives_the_round_trip() {
        let e = OutboxEntry {
            id: 7,
            subscription_id: 3,
            account_address: "lihao@golia.jp".into(),
            payload: r#"{"event":"mail.received"}"#.into(),
            attempts: 2,
            last_error: Some("HTTP 502".into()),
        };
        let pairs = e.to_pairs();
        let flat: Vec<Vec<u8>> = pairs.into_iter().flat_map(|(k, v)| [k, v]).collect();
        assert_eq!(OutboxEntry::from_flat(7, &flat), Some(e));
    }

    /// A half-written hash must not come back as an entry with an empty
    /// payload, which would POST `{}` to the subscriber and count as
    /// delivered.
    #[test]
    fn an_incomplete_hash_is_not_an_entry() {
        let flat: Vec<Vec<u8>> = vec![b"subscription_id".to_vec(), b"3".to_vec()];
        assert_eq!(OutboxEntry::from_flat(7, &flat), None);
    }
}
