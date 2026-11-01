//! What a reader's rules for one sender do to that sender's
//! conversations: move them all to another bucket, put them back, and let
//! go of the ones a retention rule no longer keeps.
//!
//! The decisions are here, once; each core lists and changes its own rows
//! through [`SenderStore`].

use std::io;

use mailrs_core_sidestate::families::sender_rules::{self, Disposal, Moved, SenderRule};

pub use mailrs_core_sidestate::families::sender_rules::BUCKETS;

/// One of a sender's conversations, as the decisions read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderThread {
    pub thread_id: String,
    /// `inbox`, `notifications`, `promotions` or `junk`.
    pub bucket: String,
    pub latest_date: i64,
    pub archived: bool,
    pub trashed: bool,
    pub held: bool,
    pub starred: bool,
    pub pinned: bool,
}

impl SenderThread {
    /// In the ordinary lists, where a sender's rules apply: not archived,
    /// in the Trash, held, or in Junk.
    fn live(&self) -> bool {
        !self.archived && !self.trashed && !self.held && BUCKETS.contains(&self.bucket.as_str())
    }
}

/// A core's rows, as these decisions change them.
pub trait SenderStore {
    /// `user`'s conversations whose newest message from someone else came
    /// from `sender`.
    fn sender_threads(&self, user: &str, sender: &str) -> io::Result<Vec<SenderThread>>;
    /// Put the conversation in `bucket`, without training anything.
    fn set_bucket(&self, user: &str, thread_id: &str, bucket: &str) -> io::Result<()>;
    fn dispose(&self, user: &str, thread_id: &str, how: Disposal, now: i64) -> io::Result<()>;
}

/// The conversations moving to `to`: the live ones in another bucket.
pub fn plan_reclassify(threads: &[SenderThread], to: &str) -> Vec<Moved> {
    threads
        .iter()
        .filter(|t| t.live() && t.bucket != to)
        .map(|t| Moved {
            thread_id: t.thread_id.clone(),
            from_bucket: t.bucket.clone(),
        })
        .collect()
}

/// The conversations a retention rule lets go of: past the newest
/// `keep_latest`, or older than `older_than_days`. Starred and pinned
/// ones are kept whatever the rule says, and do not count towards it.
pub fn plan_retention(threads: &[SenderThread], rule: &SenderRule, now: i64) -> Vec<String> {
    let mut kept: Vec<&SenderThread> = threads
        .iter()
        .filter(|t| t.live() && !t.starred && !t.pinned)
        .collect();
    kept.sort_by_key(|t| std::cmp::Reverse(t.latest_date));
    let cutoff = rule.older_than_days.map(|d| now - i64::from(d) * 86_400);
    kept.iter()
        .enumerate()
        .filter(|(i, t)| {
            let past_count = rule.keep_latest.is_some_and(|n| *i >= n as usize);
            let past_age = cutoff.is_some_and(|c| t.latest_date < c);
            past_count || past_age
        })
        .map(|(_, t)| t.thread_id.clone())
        .collect()
}

/// Move every live conversation of `sender` to `to`; what moved, for the
/// correction fact an undo reads.
pub fn reclassify<S: SenderStore + ?Sized>(
    store: &S,
    user: &str,
    sender: &str,
    to: &str,
) -> io::Result<Vec<Moved>> {
    let moved = plan_reclassify(&store.sender_threads(user, sender)?, to);
    for m in &moved {
        store.set_bucket(user, &m.thread_id, to)?;
    }
    Ok(moved)
}

/// Put back what a correction moved, where it is still where it was moved
/// to; how many went back.
pub fn restore<S: SenderStore + ?Sized>(
    store: &S,
    user: &str,
    sender: &str,
    to: &str,
    moved: &[Moved],
) -> io::Result<u32> {
    let now: Vec<SenderThread> = store.sender_threads(user, sender)?;
    let mut back = 0;
    for m in moved {
        let still = now
            .iter()
            .any(|t| t.thread_id == m.thread_id && t.bucket == to && !t.trashed);
        if still {
            store.set_bucket(user, &m.thread_id, &m.from_bucket)?;
            back += 1;
        }
    }
    Ok(back)
}

/// Apply `rule`'s retention to `sender`'s conversations; how many went.
pub fn retain<S: SenderStore + ?Sized>(
    store: &S,
    user: &str,
    sender: &str,
    rule: &SenderRule,
    now: i64,
) -> io::Result<u32> {
    if !rule.retains() {
        return Ok(0);
    }
    let doomed = plan_retention(&store.sender_threads(user, sender)?, rule, now);
    for tid in &doomed {
        store.dispose(user, tid, rule.disposal, now)?;
    }
    Ok(doomed.len() as u32)
}

/// After a delivery: a sender with "keep the newest N" may now have one
/// too many. Age is the sweep's; it does not change with a delivery.
pub fn after_delivery<S: SenderStore + ?Sized>(
    store: &S,
    net: Option<&mut kevy_client::Connection>,
    user: &str,
    from: &str,
    now: i64,
) {
    let Some(conn) = net else {
        return;
    };
    let Ok(Some(rule)) = sender_rules::get(conn, user, from) else {
        return;
    };
    if rule.keep_latest.is_none() {
        return;
    }
    let only_count = SenderRule {
        older_than_days: None,
        ..rule
    };
    let sender = sender_rules::normalize(from);
    if let Err(e) = retain(store, user, &sender, &only_count, now) {
        tracing::warn!(error = %e, %user, "sender retention after delivery failed");
    }
}

/// One pass over every rule that keeps by age; how many conversations
/// went. Nobody with such a rule is one read of an empty set.
pub fn sweep<S: SenderStore + ?Sized>(
    store: &S,
    conn: &mut kevy_client::Connection,
    now: i64,
) -> io::Result<u32> {
    let mut gone = 0;
    for user in sender_rules::retaining_users(conn)? {
        for (sender, rule) in sender_rules::list(conn, &user)? {
            if rule.older_than_days.is_some() {
                gone += retain(store, &user, &sender, &rule, now)?;
            }
        }
    }
    Ok(gone)
}

#[cfg(test)]
#[path = "sender_ops_tests.rs"]
mod tests;
