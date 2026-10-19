//! Queue mail whose attachments were never read, or were read by an older
//! extractor (before OCR).
//!
//! Walks every user's stored messages, reads each file, and queues the
//! copies that carry a PDF or an image and have no outcome from this
//! extractor generation; the worker in `attachment_text` does the
//! reading. Bounded and paused like every sweep
//! here since the outage on 2026-08-26, and dry by default.

use super::prelude::*;

/// Messages between pauses.
const PAUSE_EVERY: u64 = 100;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without queueing. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: u64,
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

/// `POST /v1/admin/maintenance:backfill-attachment-text?dry_run=false`
pub(crate) async fn backfill_attachment_text_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    // reading and parsing every stored file takes minutes: off the runtime
    match tokio::task::spawn_blocking(move || walk(&state, &q)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "backfill-attachment-text: listing accounts failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "backfill-attachment-text: walk panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn walk(state: &Arc<FastcoreState>, q: &BackfillQuery) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut files_read = 0u64;
    let mut no_file = 0u64;
    let mut with_readable = 0u64;
    let mut queued = 0u64;
    let mut already_queued = 0u64;
    let mut already_done = 0u64;
    // Done by an older extractor; queued again.
    let mut outdated = 0u64;
    let version = crate::attachment_text::version_here();
    // What earlier extraction made of the messages this walk found done.
    let mut done_failed = 0u64;
    let mut done_too_large = 0u64;
    let mut done_truncated = 0u64;
    let mut stopped_early = false;

    'walk: for user in &users {
        // one directory scan per mailbox: finding a flagged file by its id
        // lists cur/ each time, which made a full walk take half an hour
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                seen += 1;
                if seen <= q.skip {
                    continue;
                }
                if walked >= q.limit {
                    stopped_early = true;
                    break 'walk;
                }
                walked += 1;
                if walked.is_multiple_of(PAUSE_EVERY) {
                    std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
                }
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
                    no_file += 1;
                    continue;
                };
                files_read += 1;
                if !crate::attachment_text::may_carry_readable(&raw)
                    || crate::attachment_text::readable_parts(&mailrs_mime::parse(&raw))
                        .next()
                        .is_none()
                {
                    continue;
                }
                with_readable += 1;
                let done = state.mailbox.attachment_text_state(&mid).ok().flatten();
                if let Some(done) = &done
                    && done.version >= version
                {
                    already_done += 1;
                    done_failed += u64::from(done.failed);
                    done_too_large += u64::from(done.too_large);
                    done_truncated += u64::from(done.truncated);
                    continue;
                }
                if state
                    .mailbox
                    .attachment_text_queued(user, &mid)
                    .unwrap_or(false)
                {
                    already_queued += 1;
                    continue;
                }
                outdated += u64::from(done.is_some());
                if q.dry_run {
                    continue;
                }
                match state.mailbox.queue_attachment_text(user, &mid) {
                    Ok(()) => queued += 1,
                    Err(e) => tracing::warn!(error = %e, %user, %mid, "queueing pdf text failed"),
                }
            }
        }
    }

    tracing::info!(
        walked,
        files_read,
        with_readable,
        queued,
        already_done,
        outdated,
        dry_run = q.dry_run,
        "backfill-attachment-text"
    );
    Ok(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "messages_walked": walked,
        // Read off disk. Far below `messages_walked` means the maildir is
        // not where this process looks, and every count after it is low.
        "files_read": files_read,
        "no_file": no_file,
        "with_readable": with_readable,
        // Zero on a dry run, which says what it would queue as
        // `with_readable - already_done - already_queued`.
        "queued": queued,
        // Of the queued (or, on a dry run, the to-be-queued), how many
        // had an outcome from an older extractor.
        "outdated": outdated,
        "version": version,
        "already_queued": already_queued,
        "already_done": already_done,
        "already_done_failed": done_failed,
        "already_done_too_large": done_too_large,
        "already_done_truncated": done_truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No accounts answers with zeros that say so, rather than reading as
    /// "walked everything and found nothing".
    #[tokio::test]
    async fn a_backfill_over_no_mail_says_it_walked_nothing() {
        let state = crate::tests::fresh_state();
        let q = BackfillQuery {
            dry_run: false,
            skip: 0,
            limit: 10,
            pause_ms: 0,
        };
        let res = backfill_attachment_text_route(State(state), Query(q)).await;
        let out: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), 1 << 16)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(out["accounts"], 0, "{out}");
        assert_eq!(out["messages_walked"], 0, "{out}");
        assert_eq!(out["queued"], 0, "{out}");
        assert_eq!(out["done"], true, "{out}");
    }
}
