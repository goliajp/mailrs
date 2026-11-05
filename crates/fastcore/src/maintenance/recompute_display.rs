//! Put each reader's own subject and preview back on their membership row,
//! and count what the rows and the search index showed them of other
//! people's mail.
//!
//! The row's `subject` and `latest_preview` used to be copied from the
//! shared thread hash, which holds whichever member's mail arrived last,
//! and conversation search read a text field on that hash built from every
//! member's subject, senders and preview. Stopping the copy and moving the
//! search onto the rows fixes new writes; the rows already written stay
//! wrong until this runs (`recount_display`).
//!
//! Only conversations another account also holds, unless `all=true`.
//!
//! Besides what it would change, the report counts, per account:
//! - `missing_some`: pairs where another member holds a message this reader
//!   does not, the only place a leak can come from;
//! - `shows_foreign_subject` / `shows_foreign_preview`: the stored value is
//!   that of a message in the conversation the reader does not hold, and of
//!   none they do;
//! - `findable_by_foreign_words`: the shared hash's subject, senders and
//!   preview, which the retired search index matched, contain a word that
//!   the reader's own subject, senders and preview do not. Words are split
//!   on anything that is not a letter or digit, which is close to the
//!   index's tokenizer for latin text and coarser for CJK. The shared hash
//!   is not changed by this route, so the count stays after a real run; it
//!   measures what the retired index exposed, and the index is dropped at
//!   boot;
//! - why the reader lacks the others' messages (`gap_*`, per missing
//!   message): a reply that did not include them, a message that did list
//!   them, or something else; and how the reader came into the
//!   conversation (`joined_*`, per pair): with mail not addressed to them
//!   (Bcc, a list or an alias), with a forward, with their own reply, or
//!   addressed.
//!
//! `skip` / `limit` walk (user, conversation) pairs in a fixed order;
//! `next_skip` is where the next call starts. Dry run unless
//! `dry_run=false`. A row is written only when it differs, so the run after
//! a real one reports `would_change: 0`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeDisplayQuery {
    user: Option<String>,
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    all: bool,
}

fn yes() -> bool {
    true
}

fn default_limit() -> usize {
    2000
}

#[derive(Default, serde::Serialize)]
struct UserTally {
    walked: u64,
    shared: u64,
    missing_some: u64,
    agree: u64,
    change: u64,
    /// Of `change`, what differs.
    subject_differs: u64,
    preview_filled: u64,
    preview_blanked: u64,
    preview_replaced: u64,
    shows_foreign_subject: u64,
    shows_foreign_preview: u64,
    findable_by_foreign_words: u64,
    gap_reply_without_them: u64,
    gap_listed_them: u64,
    gap_other: u64,
    joined_not_addressed: u64,
    joined_forward: u64,
    joined_own_reply: u64,
    joined_addressed: u64,
    kept_single_owner: u64,
    no_messages: u64,
}

#[derive(serde::Deserialize, Default)]
struct Wire {
    #[serde(default)]
    sender: String,
    #[serde(default)]
    recipients: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    in_reply_to: String,
    #[serde(default)]
    internal_date: i64,
}

fn wire(state: &FastcoreState, mid: &str) -> Wire {
    state
        .mailbox
        .get_message(mid)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect()
}

/// How `user` came into the conversation: their earliest message in it.
fn joined(user: &str, first: &Wire, t: &mut UserTally) {
    let subject = first.subject.trim_start().to_ascii_lowercase();
    if mailrs_rfc5322::list_contains(&first.sender, user) {
        t.joined_own_reply += 1;
    } else if subject.starts_with("fwd:") || subject.starts_with("fw:") {
        t.joined_forward += 1;
    } else if mailrs_rfc5322::list_contains(&first.recipients, user) {
        t.joined_addressed += 1;
    } else {
        t.joined_not_addressed += 1;
    }
}

/// `POST /v1/admin/maintenance:recompute-thread-display[?user=&dry_run=false&skip=&limit=&all=true]`
pub(crate) async fn recompute_thread_display_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeDisplayQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let users: Vec<String> = match &q.user {
        Some(u) => vec![u.clone()],
        None => match state.mailbox.list_account_addresses() {
            Ok(mut u) => {
                u.sort();
                u
            }
            Err(e) => {
                tracing::error!(err = %e, "list_account_addresses failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };
    let mut position = 0usize;
    let mut walked = 0u64;
    let mut changed = 0u64;
    let mut errors = 0u64;
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut by_user: BTreeMap<String, UserTally> = BTreeMap::new();
    // conversations with a member missing another's message, across readers
    let mut uneven: HashSet<String> = HashSet::new();
    let end = q.skip.saturating_add(q.limit);

    'users: for user in &users {
        let mut tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        tids.sort();
        for tid in &tids {
            if position >= end {
                break 'users;
            }
            position += 1;
            if position <= q.skip {
                continue;
            }
            walked += 1;
            let tally = by_user.entry(user.clone()).or_default();
            tally.walked += 1;
            let owners = match state.mailbox.thread_message_owners(tid) {
                Ok(o) => o,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "display recompute: owners unread");
                    continue;
                }
            };
            let shared = owners.iter().any(|(o, _)| o != user);
            if shared {
                tally.shared += 1;
            } else if !q.all {
                tally.kept_single_owner += 1;
                continue;
            }
            let held: HashSet<String> = state
                .mailbox
                .user_thread_message_ids(user, tid)
                .unwrap_or_default()
                .into_iter()
                .collect();
            if held.is_empty() {
                tally.no_messages += 1;
                continue;
            }
            let missing: BTreeSet<&str> = owners
                .iter()
                .filter(|(_, m)| !held.contains(m))
                .map(|(_, m)| m.as_str())
                .collect();
            if !missing.is_empty() {
                tally.missing_some += 1;
                uneven.insert(tid.clone());
                for mid in &missing {
                    let w = wire(&state, mid);
                    if mailrs_rfc5322::list_contains(&w.recipients, user) {
                        tally.gap_listed_them += 1;
                    } else if !w.in_reply_to.is_empty() {
                        tally.gap_reply_without_them += 1;
                    } else {
                        tally.gap_other += 1;
                    }
                }
                let first = held
                    .iter()
                    .map(|m| wire(&state, m))
                    .min_by_key(|w| w.internal_date)
                    .unwrap_or_default();
                joined(user, &first, tally);
            }
            let r = match state.mailbox.recount_display(user, tid, q.dry_run) {
                Ok(r) => r,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "display recompute failed");
                    continue;
                }
            };
            // Against what the reader's own messages say, not the row: a
            // dry run leaves the row as it is.
            if !missing.is_empty()
                && let (Ok(Some(hash)), Ok(senders)) = (
                    state.mailbox.get_thread(tid),
                    state.mailbox.participants_from_messages(user, tid),
                )
            {
                let theirs = words(&format!(
                    "{} {} {}",
                    hash.subject, hash.senders_csv, hash.latest_preview
                ));
                let own = words(&format!("{} {} {}", r.subject, senders, r.preview));
                if !theirs.is_subset(&own) {
                    tally.findable_by_foreign_words += 1;
                }
            }
            if r.foreign_subject {
                tally.shows_foreign_subject += 1;
            }
            if r.foreign_preview {
                tally.shows_foreign_preview += 1;
            }
            if !r.differs {
                tally.agree += 1;
                continue;
            }
            tally.change += 1;
            tally.subject_differs += u64::from(r.subject_differs);
            tally.preview_filled += u64::from(r.preview_filled);
            tally.preview_blanked += u64::from(r.preview_blanked);
            tally.preview_replaced += u64::from(r.preview_replaced);
            if !q.dry_run {
                changed += 1;
            }
            if samples.len() < 20 {
                samples.push(serde_json::json!({
                    "user": user, "thread_id": tid, "shared": shared,
                    "foreign_subject": r.foreign_subject, "foreign_preview": r.foreign_preview,
                }));
            }
        }
    }

    let sum = |f: fn(&UserTally) -> u64| -> u64 { by_user.values().map(f).sum() };
    let would_change = sum(|t| t.change);
    let done = walked < q.limit as u64;
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        would_change,
        changed,
        "thread display recompute"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "all": q.all,
            "skip": q.skip,
            "limit": q.limit,
            "walked": walked,
            "would_change": would_change,
            "shows_foreign_subject": sum(|t| t.shows_foreign_subject),
            "shows_foreign_preview": sum(|t| t.shows_foreign_preview),
            "findable_by_foreign_words": sum(|t| t.findable_by_foreign_words),
            "missing_some": sum(|t| t.missing_some),
            "conversations_uneven_in_batch": uneven.len(),
            "changed": changed,
            "errors": errors,
            "by_user": by_user,
            "samples": samples,
            "done": done,
            "next_skip": q.skip + walked as usize,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
