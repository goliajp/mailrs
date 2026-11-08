//! Decide the sectioned inbox's section for mail delivered before sections
//! existed, and again for rows recorded before the facts Reply now reads
//! (`invite`, `mutual`): each message's facts are read again from its file
//! and folded into the reader's row the way delivery folds a new one.
//!
//! Old mail is never a first contact: whoever sent it already has a contact
//! record counting it, which is the only history delivery reads. Whether
//! the reader and a sender write to each other is read from today's counts,
//! not the counts when the mail arrived. Dry by default, paged by the last
//! thread finished, stopped by a hang-up. A row refolded has both facts, so
//! a second walk refolds nothing.

use super::prelude::*;
use super::thread_cursor::{After, next, threads_after, users_from};
use crate::sections::{ReadArrival, as_arrivals, read_arrival};
use mailrs_mailbox_kevy::section::{Latest, section_of};

const PAUSE_EVERY: u64 = 100;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without writing. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    after_user: Option<String>,
    after_thread: Option<String>,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn yes() -> bool {
    true
}
fn default_limit() -> u64 {
    2000
}
fn default_pause_ms() -> u64 {
    50
}

/// `POST /v1/admin/maintenance:backfill-sections?dry_run=false`
pub(crate) async fn backfill_sections_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    match super::hangup::run_blocking(move |h| walk(&state, &q, h)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "backfill-sections: listing accounts failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "backfill-sections task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Default)]
struct Counts {
    threads: u64,
    messages: u64,
    no_file: u64,
    /// Threads without a section that this walk would decide.
    changed: u64,
    /// Threads already decided on the current facts: left alone.
    decided: u64,
    sections: std::collections::BTreeMap<&'static str, u64>,
    /// Decided threads whose row lacks `invite` and `mutual`.
    outdated: u64,
    /// Of those, refolded (0 on a dry run).
    refreshed: u64,
    /// Of those, none of whose messages could be read: left as they were.
    unreadable: u64,
    /// `before>after` for each refold that changed the section.
    moved: std::collections::BTreeMap<String, u64>,
    /// What the refolds did to each section's count.
    change: std::collections::BTreeMap<String, i64>,
}

fn walk(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    hangup: &super::hangup::Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut net = state.net_conn();
    let mut c = Counts::default();
    let mut stopped_early = false;
    let after = After::from_query(q.after_user.as_deref(), q.after_thread.as_deref());
    let mut last: Option<After> = None;
    'walk: for user in &users_from(users.clone(), after.as_ref()) {
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in threads_after(state, user, after.as_ref()) {
            if c.threads >= q.limit || hangup.hung_up() {
                stopped_early = true;
                break 'walk;
            }
            c.threads += 1;
            if c.threads.is_multiple_of(PAUSE_EVERY) {
                std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
            }
            // a section delivery decided stands: it read the facts as they
            // were then (first contact above all), which a backfill can only
            // approximate, so going over it would undo the better record;
            // unless the row predates facts the section now reads
            let (have, _) = state.mailbox.section_fields(user, &tid)?;
            let outdated = !have.is_empty() && state.mailbox.section_facts_outdated(user, &tid)?;
            if !have.is_empty() && !outdated {
                c.decided += 1;
            } else {
                let read = read_thread(state, user, &tid, &on_disk, &mut net, &mut c)?;
                match outdated {
                    true => refresh(state, q, user, &tid, &read, &mut c)?,
                    false if read.is_empty() => continue,
                    false => decide(state, q, user, &tid, &read, &mut c)?,
                }
            }
            last = Some(After {
                user: user.clone(),
                thread: tid.clone(),
            });
        }
    }
    tracing::info!(
        threads = c.threads,
        changed = c.changed,
        refreshed = c.refreshed,
        dry_run = q.dry_run,
        "backfill-sections"
    );
    Ok(serde_json::json!({
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "threads_walked": c.threads,
        "messages_walked": c.messages,
        "no_file": c.no_file,
        "changed": c.changed,
        "already_decided": c.decided,
        "by_section": c.sections,
        "outdated_facts": c.outdated,
        "refreshed": c.refreshed,
        "refresh_unreadable": c.unreadable,
        "moved": c.moved,
        "section_change": c.change,
        "next_after": next(&last),
        "done": !stopped_early,
    }))
}

/// Every message of the thread whose file is on disk, read as delivery
/// reads it.
fn read_thread(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    on_disk: &std::collections::HashMap<String, std::path::PathBuf>,
    net: &mut Option<kevy_client::Connection>,
    c: &mut Counts,
) -> std::io::Result<Vec<ReadArrival>> {
    let category = state
        .mailbox
        .get_thread_for_user(user, tid)?
        .map(|r| r.category)
        .unwrap_or_default();
    let mut read = Vec::new();
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
    {
        c.messages += 1;
        let copy = state.mailbox.user_message_facts(user, &mid).ok().flatten();
        let blob_ref = copy.map(|c| c.blob_ref).unwrap_or_default();
        let Some(raw) = on_disk
            .get(crate::maildir_scan::base_id(&blob_ref))
            .cloned()
            .and_then(|path| std::fs::read(path).ok())
        else {
            c.no_file += 1;
            continue;
        };
        read.push(read_arrival(user, &blob_ref, &raw, &category, net.as_mut()));
    }
    Ok(read)
}

/// A thread with no section yet: fold its messages forward, as delivery
/// would have.
fn decide(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    user: &str,
    tid: &str,
    read: &[ReadArrival],
    c: &mut Counts,
) -> std::io::Result<()> {
    let mut latest = Latest::default();
    for r in read {
        latest.fold(r.at, r.own, r.facts);
    }
    let category = state
        .mailbox
        .get_thread_for_user(user, tid)?
        .map(|r| r.category)
        .unwrap_or_default();
    let want = match mailrs_mailbox_kevy::scope::bucket_of(&category) {
        mailrs_mailbox_kevy::scope::Bucket::Junk => "",
        _ => section_of(&latest).name(),
    };
    *c.sections.entry(want).or_default() += 1;
    if !want.is_empty() {
        c.changed += 1;
        if !q.dry_run {
            for r in read {
                crate::sections::record(state, user, tid, r.at, r.own, r.facts, &r.from_addr);
            }
        }
    }
    Ok(())
}

/// A decided thread whose row predates `invite` and `mutual`: decide it
/// again from all its messages.
fn refresh(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    user: &str,
    tid: &str,
    read: &[ReadArrival],
    c: &mut Counts,
) -> std::io::Result<()> {
    c.outdated += 1;
    let arrivals = as_arrivals(user, tid, read);
    let Some(r) = state
        .mailbox
        .refold_section_report(user, tid, &arrivals, !q.dry_run)?
    else {
        c.unreadable += 1;
        return Ok(());
    };
    if !q.dry_run {
        c.refreshed += 1;
    }
    if r.before != r.after {
        let name = |s: &str| match s {
            "" => "none".to_string(),
            s => s.to_string(),
        };
        let (before, after) = (name(&r.before), name(&r.after));
        *c.moved.entry(format!("{before}>{after}")).or_default() += 1;
        *c.change.entry(before).or_default() -= 1;
        *c.change.entry(after).or_default() += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::hangup::{Hangup, seed};
    use super::*;

    /// No files to read: every message is counted as missing and nothing is
    /// called decided — a walk that read nothing must say so.
    #[test]
    fn a_walk_without_files_says_it_read_none() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 3);
        let q = BackfillQuery {
            dry_run: false,
            after_user: None,
            after_thread: None,
            limit: 100,
            pause_ms: 0,
        };
        let out = walk(&state, &q, &Hangup::never()).unwrap();
        assert_eq!(out["threads_walked"], 3, "{out}");
        assert_eq!(out["no_file"], 3, "{out}");
        assert_eq!(out["changed"], 0, "{out}");
        assert_eq!(out["done"], true, "{out}");
    }

    /// What delivery decided stands: the walk skips it, says so, and finds
    /// nothing to change, so a second walk has the zero a converged
    /// backfill must be able to show.
    #[test]
    fn a_section_delivery_decided_is_left_alone() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 2);
        let facts = mailrs_mailbox_kevy::section::MessageFacts {
            direct: true,
            from_person: true,
            first_contact: true,
            ..Default::default()
        };
        crate::sections::record(&state, "u@x", "t0", 100, false, facts, "a@x");
        let before = state.mailbox.section_fields("u@x", "t0").unwrap();
        assert!(!before.0.is_empty(), "delivery decided it");
        let q = BackfillQuery {
            dry_run: false,
            after_user: None,
            after_thread: None,
            limit: 100,
            pause_ms: 0,
        };
        let out = walk(&state, &q, &Hangup::never()).unwrap();
        assert_eq!(out["already_decided"], 1, "{out}");
        assert_eq!(out["threads_walked"], 2, "{out}");
        assert_eq!(out["changed"], 0, "{out}");
        assert_eq!(state.mailbox.section_fields("u@x", "t0").unwrap(), before);
    }

    #[test]
    fn a_walk_stops_soon_after_the_caller_hangs_up() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 5);
        let q = BackfillQuery {
            dry_run: true,
            after_user: None,
            after_thread: None,
            limit: 100,
            pause_ms: 0,
        };
        let cut = walk(&state, &q, &Hangup::after(2)).unwrap();
        assert_eq!(cut["done"], false, "{cut}");
    }
}
