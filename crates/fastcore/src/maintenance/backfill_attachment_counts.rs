//! Count the attachments of mail stored before the count was kept, so
//! `has:attachment` finds it.
//!
//! Walks every user's stored messages; a message already counted is
//! skipped without reading its file, so a second full run reads nothing.
//! Bounded, paused and dry by default like every sweep here.

use super::prelude::*;

/// Messages between pauses.
const PAUSE_EVERY: u64 = 100;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without writing. **Default true.**
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

/// `POST /v1/admin/maintenance:backfill-attachment-counts?dry_run=false`
pub(crate) async fn backfill_attachment_counts_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    // reading and parsing stored files takes minutes: off the runtime
    match tokio::task::spawn_blocking(move || walk(&state, &q)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "backfill-attachment-counts: listing accounts failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "backfill-attachment-counts: walk panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn walk(state: &Arc<FastcoreState>, q: &BackfillQuery) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut already = 0u64;
    let mut files_read = 0u64;
    let mut no_file = 0u64;
    let mut with_attachments = 0u64;
    let mut written = 0u64;
    let mut stopped_early = false;

    'walk: for user in &users {
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
                if state
                    .mailbox
                    .message_attachments_counted(&mid)
                    .unwrap_or(false)
                {
                    already += 1;
                    continue;
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
                let count = mailrs_mime::parse(&raw).shown_attachment_count() as u32;
                with_attachments += u64::from(count > 0);
                if q.dry_run {
                    continue;
                }
                match state.mailbox.set_message_attachments(&mid, count) {
                    Ok(true) => written += 1,
                    Ok(false) => already += 1,
                    Err(e) => {
                        tracing::warn!(error = %e, %user, %mid, "recording attachments failed")
                    }
                }
            }
        }
    }

    tracing::info!(
        walked,
        already,
        files_read,
        no_file,
        with_attachments,
        written,
        dry_run = q.dry_run,
        "backfill-attachment-counts"
    );
    Ok(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "messages_walked": walked,
        // counted before this run, by delivery or an earlier run
        "already_counted": already,
        // read off disk; far below the uncounted means the maildir is not
        // where this process looks
        "files_read": files_read,
        "no_file": no_file,
        "with_attachments": with_attachments,
        // zero on a dry run
        "written": written,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(mid: &str, attachments: Option<u32>) -> Vec<u8> {
        let mut v = serde_json::json!({
            "uid": 1, "sender": "a@x", "recipients": "b@x", "subject": "s",
            "date": 1, "internal_date": 1, "size": 1, "flags": 0,
            "message_id": mid, "in_reply_to": "", "thread_id": "t", "modseq": 0,
        });
        if let Some(n) = attachments {
            v["attachments"] = n.into();
        }
        serde_json::to_vec(&v).unwrap()
    }

    /// Counting writes once: a message already counted, by delivery or an
    /// earlier run, keeps its count.
    #[test]
    fn a_count_is_written_once() {
        let state = crate::tests::fresh_state();
        let mb = &state.mailbox;
        let facts = mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: "f1",
            uid: 1,
            flags: 0,
            modseq: 0,
        };
        mb.upsert_user_message("u@x", "t", "m-old", 1, &wire("m-old", None), &facts)
            .unwrap();
        mb.upsert_user_message("u@x", "t", "m-new", 1, &wire("m-new", Some(2)), &facts)
            .unwrap();
        assert!(!mb.message_attachments_counted("m-old").unwrap());
        assert!(mb.message_attachments_counted("m-new").unwrap());
        assert!(mb.set_message_attachments("m-old", 3).unwrap());
        assert!(!mb.set_message_attachments("m-old", 9).unwrap());
        assert!(!mb.set_message_attachments("m-new", 9).unwrap());
        let read = |mid: &str| {
            let v: serde_json::Value =
                serde_json::from_slice(&mb.get_message(mid).unwrap().unwrap()).unwrap();
            v["attachments"].as_u64()
        };
        assert_eq!(read("m-old"), Some(3));
        assert_eq!(read("m-new"), Some(2));
    }

    #[tokio::test]
    async fn a_backfill_over_no_mail_says_it_walked_nothing() {
        let state = crate::tests::fresh_state();
        let q = BackfillQuery {
            dry_run: false,
            skip: 0,
            limit: 10,
            pause_ms: 0,
        };
        let report = walk(&state, &q).unwrap();
        assert_eq!(report["messages_walked"], 0);
        assert_eq!(report["done"], true);
    }
}
