//! Put stored message dates back to what ingest computes from the file.
//!
//! Between 3cb56102 and 71986e7d (2026-07-03) the parser that wrote message
//! dates read the sender's local time and ignored the zone, so a `+0900`
//! message was stored nine hours late and a `-0700` one seven hours early.
//! Messages stored with no date at all (0) are a second population. Both
//! are recomputed from the file with the rule ingest uses today
//! (`stored_date`: the earlier of `Date:` and the arrival in the file
//! name), and every conversation holding one has each reader's row time
//! and the shared `latest_date` recounted (`redate_messages`).
//!
//! Only differences it can explain are corrected (`classify`). A stored
//! date nine hours before a `-0000` header has no known writer and is
//! counted under `skipped_minus_zero`; any other difference is counted
//! under `skipped_unexplained`, by shape (`unexplained_by_shape`; on
//! production almost all are rows stored at their arrival, seconds after
//! their `Date:`). Neither is written.
//!
//! What a reader sees: the web list and conversation, and JMAP
//! `receivedAt`, read the stored date. IMAP does not: its INTERNALDATE is
//! the delivery second in the maildir file name and its ENVELOPE date is
//! the file's own header, and no file is renamed here, so IMAP clients see
//! nothing change.
//!
//! `skip` / `limit` walk conversations (the union of every account's,
//! sorted); `next_skip` is where the next call starts. Dry run unless
//! `dry_run=false`. Only what differs is written, so the run after a real
//! one reports nothing to correct. The list positions in the report
//! (`rows_move`, `rows_move_in_newest_200`) are computed per call against
//! the rows as they stand, so a batched dry run counts each batch as if
//! the earlier ones had not run.

mod classify;
mod positions;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::PathBuf;

use super::prelude::*;
use classify::{FromFile, Verdict, classify};
use mailrs_mailbox_kevy::Owned;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeDatesQuery {
    /// Only conversations this account holds. Absent means every account.
    user: Option<String>,
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn yes() -> bool {
    true
}

fn default_limit() -> usize {
    1000
}

/// Per account. A message several accounts hold counts for each of them.
#[derive(Default, serde::Serialize)]
struct Tally {
    /// Messages whose shown time changes.
    messages_time_changes: u64,
    /// Of those: stored as 0, and stored with the zone ignored.
    zero_date: u64,
    zone_ignored: u64,
    skipped_minus_zero: u64,
    skipped_unexplained: u64,
    unexplained_by_shape: BTreeMap<String, u64>,
    /// No copy of the message has a file to read.
    no_file: u64,
    /// Rows whose time changes.
    rows_time_changes: u64,
    /// Of those, rows that change position in their folder.
    rows_move: u64,
    /// Of those, rows in the newest 200 of their folder before or after.
    rows_move_in_newest_200: u64,
    /// Rows whose stored time already differed from their messages before
    /// any date here changed.
    rows_off_before: u64,
}

const NEWEST: usize = 200;
const HEAD: u64 = 16 * 1024;

type Files = HashMap<(String, String), PathBuf>;

/// `POST /v1/admin/maintenance:recompute-message-dates[?user=&dry_run=false&skip=&limit=]`
pub(crate) async fn recompute_message_dates_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeDatesQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let tids = match conversations(&state, q.user.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(err = %e, "recompute-message-dates: accounts unread");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut walk = Walk::default();
    let batch: Vec<&String> = tids.iter().skip(q.skip).take(q.limit).collect();
    for tid in &batch {
        walk.thread(&state, tid);
    }
    walk.positions(&state);
    let mut written = 0u64;
    // what the readers of the written conversations own, read around each
    // write: a redate that changed any of it is counted, not hidden
    let mut owned_before = Owned::default();
    let mut owned_after = Owned::default();
    let mut owned_changed = 0u64;
    if !q.dry_run {
        for (tid, fix) in &walk.pending {
            match write(&state, tid, fix) {
                Ok((before, after)) => {
                    written += 1;
                    owned_before.add(before);
                    owned_after.add(after);
                    if before != after {
                        owned_changed += 1;
                    }
                }
                Err(e) => {
                    walk.errors += 1;
                    tracing::warn!(err = %e, %tid, "recompute-message-dates: write failed");
                }
            }
        }
    }
    let total = |f: fn(&Tally) -> u64| walk.by_user.values().map(f).sum::<u64>();
    tracing::info!(
        dry_run = q.dry_run,
        threads = batch.len(),
        to_correct = walk.pending.len(),
        written,
        "recompute message dates"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "skip": q.skip,
            "limit": q.limit,
            "threads_walked": batch.len(),
            "messages_walked": walk.messages,
            "agree": walk.agree,
            "messages_to_correct": walk.to_correct,
            "threads_to_correct": walk.pending.len(),
            "threads_written": written,
            "owned_before": owned_before,
            "owned_after": owned_after,
            "threads_owned_changed": owned_changed,
            "shared_latest_date_changes": walk.shared_changes,
            "messages_time_changes": total(|t| t.messages_time_changes),
            "rows_time_changes": total(|t| t.rows_time_changes),
            "rows_move": total(|t| t.rows_move),
            "rows_move_in_newest_200": total(|t| t.rows_move_in_newest_200),
            "skipped_minus_zero": walk.skipped_minus_zero,
            "skipped_unexplained": walk.skipped_unexplained,
            "unexplained_by_shape": walk.unexplained_by_shape,
            "zone_ignored_by_hours": walk.zone_ignored_by_hours,
            "no_blob": walk.no_blob,
            "errors": walk.errors,
            "by_user": walk.by_user,
            "samples": walk.samples,
            "done": q.skip + batch.len() >= tids.len(),
            "next_skip": q.skip + batch.len(),
        }),
        motion.finish(&state),
    ))
    .into_response()
}

fn write(
    state: &FastcoreState,
    tid: &str,
    fix: &BTreeMap<String, i64>,
) -> std::io::Result<(Owned, Owned)> {
    let before = state.mailbox.owned_in(tid)?;
    state.mailbox.redate_messages(tid, fix, false)?;
    Ok((before, state.mailbox.owned_in(tid)?))
}

/// Every conversation any account (or `user`) holds, sorted.
fn conversations(state: &FastcoreState, user: Option<&str>) -> std::io::Result<Vec<String>> {
    let users = match user {
        Some(u) => vec![u.to_string()],
        None => state.mailbox.list_account_addresses()?,
    };
    let mut all = BTreeSet::new();
    for u in &users {
        all.extend(state.mailbox.all_thread_ids_for_user(u)?);
    }
    Ok(all.into_iter().collect())
}

#[derive(Default)]
struct Walk {
    files: HashMap<String, Files>,
    by_user: BTreeMap<String, Tally>,
    /// Conversations to correct, with the new date of each message.
    pending: Vec<(String, BTreeMap<String, i64>)>,
    /// Per account, the rows whose time changes: thread id → new time.
    row_changes: BTreeMap<String, BTreeMap<String, i64>>,
    messages: u64,
    agree: u64,
    to_correct: u64,
    shared_changes: u64,
    skipped_minus_zero: u64,
    skipped_unexplained: u64,
    unexplained_by_shape: BTreeMap<String, u64>,
    /// Corrected zone-ignored messages by how many hours they were off.
    zone_ignored_by_hours: BTreeMap<String, u64>,
    no_blob: u64,
    errors: u64,
    samples: Vec<serde_json::Value>,
}

impl Walk {
    fn thread(&mut self, state: &Arc<FastcoreState>, tid: &str) {
        let owners = match state.mailbox.thread_message_owners(tid) {
            Ok(o) => o,
            Err(e) => {
                self.errors += 1;
                tracing::warn!(err = %e, %tid, "recompute-message-dates: owners unread");
                return;
            }
        };
        let mut holders: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (user, mid) in owners {
            holders.entry(mid).or_default().push(user);
        }
        let mut fix = BTreeMap::new();
        for (mid, users) in &holders {
            if let Some(want) = self.message(state, tid, mid, users) {
                fix.insert(mid.clone(), want);
            }
        }
        if fix.is_empty() {
            return;
        }
        let redate = match state.mailbox.redate_messages(tid, &fix, true) {
            Ok(r) => r,
            Err(e) => {
                self.errors += 1;
                tracing::warn!(err = %e, %tid, "recompute-message-dates: recount failed");
                return;
            }
        };
        for r in &redate.rows {
            let tally = self.by_user.entry(r.user.clone()).or_default();
            if r.recount_before != r.from {
                tally.rows_off_before += 1;
            }
            if r.from != r.to {
                tally.rows_time_changes += 1;
                self.row_changes
                    .entry(r.user.clone())
                    .or_default()
                    .insert(tid.to_string(), r.to);
            }
        }
        if redate.shared_from != redate.shared_to {
            self.shared_changes += 1;
        }
        self.pending.push((tid.to_string(), fix));
    }

    /// The corrected date of `mid`, or `None` when it stays as it is.
    fn message(
        &mut self,
        state: &Arc<FastcoreState>,
        tid: &str,
        mid: &str,
        users: &[String],
    ) -> Option<i64> {
        self.messages += 1;
        let Some(stored) = stored_date_of(state, mid) else {
            self.no_blob += 1;
            return None;
        };
        // the earliest any copy's file gives, so a message delivered to
        // several accounts takes the first arrival
        let file = users
            .iter()
            .filter_map(|u| self.read_file(state, u, mid))
            .min_by_key(|f| f.want);
        let Some(file) = file else {
            for u in users {
                self.by_user.entry(u.clone()).or_default().no_file += 1;
            }
            return None;
        };
        let verdict = classify(stored, &file);
        match verdict {
            Verdict::Agree => self.agree += 1,
            Verdict::MinusZeroNineHoursEarly => self.skipped_minus_zero += 1,
            Verdict::Unexplained => {
                self.skipped_unexplained += 1;
                *self
                    .unexplained_by_shape
                    .entry(classify::shape(stored, &file))
                    .or_default() += 1
            }
            Verdict::ZoneIgnored => {
                let hours = (stored - file.want) / 3600;
                *self
                    .zone_ignored_by_hours
                    .entry(format!("{hours:+}h"))
                    .or_default() += 1
            }
            Verdict::Zero => {}
        }
        if verdict.corrected() {
            self.to_correct += 1;
        }
        for u in users {
            let t = self.by_user.entry(u.clone()).or_default();
            match verdict {
                Verdict::Agree => {}
                Verdict::Zero => t.zero_date += 1,
                Verdict::ZoneIgnored => t.zone_ignored += 1,
                Verdict::MinusZeroNineHoursEarly => t.skipped_minus_zero += 1,
                Verdict::Unexplained => {
                    t.skipped_unexplained += 1;
                    *t.unexplained_by_shape
                        .entry(classify::shape(stored, &file))
                        .or_default() += 1;
                }
            }
            if verdict.corrected() {
                t.messages_time_changes += 1;
            }
        }
        if verdict != Verdict::Agree && self.samples.len() < 20 {
            self.samples.push(serde_json::json!({
                "thread_id": tid, "message_id": mid, "verdict": format!("{verdict:?}"),
                "stored": stored, "want": file.want, "header": file.header,
                "zone": format!("{:?}", file.zone), "arrival": file.arrival,
            }));
        }
        verdict.corrected().then_some(file.want)
    }

    fn read_file(&mut self, state: &Arc<FastcoreState>, user: &str, mid: &str) -> Option<FromFile> {
        let facts = state.mailbox.user_message_facts(user, mid).ok()??;
        let (folder, leaf) = match facts.blob_ref.rsplit_once('/') {
            Some((f, l)) if f.starts_with('.') => (f.to_string(), l),
            _ => (String::new(), facts.blob_ref.as_str()),
        };
        let base = leaf.split(':').next().unwrap_or(leaf).to_string();
        let files = self
            .files
            .entry(user.to_string())
            .or_insert_with(|| super::future_dates::files_of(state, user));
        let path = files.get(&(folder, base))?;
        let mut head = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(HEAD)
            .read_to_end(&mut head)
            .ok()?;
        let header = crate::headers::extract_headers(&head).4;
        Some(FromFile {
            header,
            zone: mailrs_inbound::dates::date_header_zone(&head),
            arrival: crate::headers::maildir_filename_epoch(leaf),
            want: crate::headers::stored_date(header, &facts.blob_ref),
        })
    }

    /// Rank the changed rows in each reader's folders, before any write.
    fn positions(&mut self, state: &Arc<FastcoreState>) {
        for (user, changes) in &self.row_changes {
            let ranks = match positions::ranks(&state.mailbox, user, changes) {
                Ok(r) => r,
                Err(e) => {
                    self.errors += 1;
                    tracing::warn!(err = %e, %user, "recompute-message-dates: rows unread");
                    continue;
                }
            };
            let tally = self.by_user.entry(user.clone()).or_default();
            for rank in ranks.values().filter(|r| r.before != r.after) {
                tally.rows_move += 1;
                if rank.before < NEWEST || rank.after < NEWEST {
                    tally.rows_move_in_newest_200 += 1;
                }
            }
        }
    }
}

fn stored_date_of(state: &FastcoreState, mid: &str) -> Option<i64> {
    let blob = state.mailbox.get_message(mid).ok()??;
    let v: serde_json::Value = serde_json::from_slice(&blob).ok()?;
    Some(v["internal_date"].as_i64().unwrap_or(0))
}
