//! Decide the sectioned inbox's section for mail delivered before sections
//! existed: each message's facts are read again from its file and folded
//! into the reader's row the way delivery folds a new one.
//!
//! Old mail is never a first contact: whoever sent it already has a contact
//! record counting it, which is the only history delivery reads. Dry by
//! default, paged by the last thread finished, stopped by a hang-up.

use super::prelude::*;
use super::thread_cursor::{After, next, threads_after, users_from};
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
    /// Threads whose section this walk would set or change.
    changed: u64,
    /// Of `changed`, those that had none before.
    undecided: u64,
    sections: std::collections::BTreeMap<&'static str, u64>,
}

fn walk(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    hangup: &super::hangup::Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut c = Counts::default();
    let mut stopped_early = false;
    let after = After::from_query(q.after_user.as_deref(), q.after_thread.as_deref());
    let mut last: Option<After> = None;
    'walk: for user in &users_from(users.clone(), after.as_ref()) {
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in threads_after(state, user, after.as_ref()) {
            if c.threads >= q.limit {
                stopped_early = true;
                break 'walk;
            }
            if hangup.hung_up() {
                stopped_early = true;
                break 'walk;
            }
            c.threads += 1;
            if c.threads.is_multiple_of(PAUSE_EVERY) {
                std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
            }
            let category = state
                .mailbox
                .get_thread_for_user(user, &tid)?
                .map(|r| r.category)
                .unwrap_or_default();
            let mut latest = Latest::default();
            let mut arrivals = Vec::new();
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                c.messages += 1;
                let Some(raw) = state
                    .mailbox
                    .user_message_facts(user, &mid)
                    .ok()
                    .flatten()
                    .and_then(|copy| {
                        on_disk
                            .get(crate::maildir_scan::base_id(&copy.blob_ref))
                            .cloned()
                    })
                    .and_then(|path| std::fs::read(path).ok())
                else {
                    c.no_file += 1;
                    continue;
                };
                let head = &raw[..raw.len().min(16 * 1024)];
                let message = mailrs_rfc5322::Message::new(head);
                let from = message.header_str("From").unwrap_or_default().to_string();
                let to = message.header_str("To").unwrap_or_default().to_string();
                let at = mailrs_inbound::dates::received_epoch(&raw).unwrap_or(0);
                let own = mailrs_rfc5322::list_contains(&from, user);
                let facts = mailrs_intake::section_facts::facts(
                    None, user, &from, &to, head, &category, own,
                );
                latest.fold(at, own, facts);
                arrivals.push((at, own, facts, from));
            }
            if arrivals.is_empty() {
                continue;
            }
            let want = match mailrs_mailbox_kevy::scope::bucket_of(&category) {
                mailrs_mailbox_kevy::scope::Bucket::Junk => "",
                _ => section_of(&latest).name(),
            };
            let (have, _) = state.mailbox.section_fields(user, &tid)?;
            *c.sections.entry(want).or_default() += 1;
            if have != want {
                c.changed += 1;
                c.undecided += u64::from(have.is_empty());
                if !q.dry_run {
                    for (at, own, facts, from) in &arrivals {
                        crate::sections::record(state, user, &tid, *at, *own, *facts, from);
                    }
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
        "were_undecided": c.undecided,
        "by_section": c.sections,
        "next_after": next(&last),
        "done": !stopped_early,
    }))
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
