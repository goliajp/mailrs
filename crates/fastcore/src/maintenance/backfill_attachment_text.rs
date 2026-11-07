//! Queue mail whose attachments were never read, or were read by an older
//! extractor.
//!
//! Two kinds of mail are behind, and they cost very differently, so a call
//! names one with `select`:
//!
//! - `qr_only`: the text is current and only the QR codes were not read.
//!   The worker reads the codes alone, without OCR or the text readers.
//!   One paged call queues them.
//! - `text`: never read, or read before OCR or the Office readers: the
//!   worker reads everything again, OCR included. With `dry_run=false`
//!   this starts (or resumes) the backlog run in `attachment_backlog`,
//!   which queues a few at a time, keeps its cursor in the store and stops
//!   by itself when fastcore's memory passes a limit. With `stop=true` it
//!   stops after the batch in hand. A dry run walks a page and counts.
//!
//! Each walk reads every stored message's file and reports what it walked
//! beside what it chose, so a walk that read nothing is not mistaken for
//! one that found nothing to do. Mail already read at this generation is
//! skipped, so a second run queues nothing. Bounded and paused like every
//! sweep here since the outage on 2026-08-26, and dry by default.

use std::collections::HashMap;
use std::path::PathBuf;

use super::prelude::*;
use super::thread_cursor::{After, next, threads_after, users_from};

/// Messages between pauses.
pub(super) const PAUSE_EVERY: u64 = 100;

/// Which mail a call is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Select {
    QrOnly,
    Text,
}

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Required: `qr_only` or `text`.
    select: Option<Select>,
    /// Report without queueing. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    /// `select=text` only: stop the backlog run.
    #[serde(default)]
    stop: bool,
    /// `select=text` only: the memory limit the run stops at, in MiB.
    max_rss_mb: Option<u64>,
    /// The last thread the previous page finished: its `next_after`.
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

/// `POST /v1/admin/maintenance:backfill-attachment-text?select=qr_only&dry_run=false`
pub(crate) async fn backfill_attachment_text_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    let Some(select) = q.select else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "select=qr_only or select=text",
        )
            .into_response();
    };
    if select == Select::Text && q.stop {
        return Json(super::attachment_backlog::stop(&state)).into_response();
    }
    if select == Select::Text && !q.dry_run {
        let limit = q
            .max_rss_mb
            .map_or(super::attachment_backlog::MAX_RSS, |mb| mb * 1024 * 1024);
        return Json(super::attachment_backlog::start(&state, limit)).into_response();
    }
    // reading and parsing every stored file takes minutes: off the runtime
    match super::hangup::run_blocking(move |h| walk(&state, select, &q, h)).await {
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

/// What a walk found one stored copy to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found {
    /// The copy or its file is gone.
    NoFile,
    /// No PDF, image or Office part.
    NothingReadable,
    /// Read at this generation.
    Done(mailrs_mailbox_kevy::AttachmentTextState),
    /// Waiting for the worker already.
    Queued,
    /// Text current, QR codes not read.
    QrOnly,
    /// Never read (`read_before` false), or read before OCR or Office.
    Text { read_before: bool },
}

impl Found {
    pub(super) fn selected(&self, select: Select) -> bool {
        matches!(
            (self, select),
            (Found::QrOnly, Select::QrOnly) | (Found::Text { .. }, Select::Text)
        )
    }
}

/// What `user`'s copy of `mid` is, against extractor generation `version`.
pub(super) fn classify(
    state: &FastcoreState,
    user: &str,
    mid: &str,
    on_disk: &HashMap<String, PathBuf>,
    version: u32,
) -> Found {
    let Some(raw) = state
        .mailbox
        .user_message_facts(user, mid)
        .ok()
        .flatten()
        .and_then(|copy| {
            on_disk
                .get(crate::maildir_scan::base_id(&copy.blob_ref))
                .cloned()
        })
        .and_then(|path| std::fs::read(path).ok())
    else {
        return Found::NoFile;
    };
    if !crate::attachment_text::may_carry_readable(&raw)
        || crate::attachment_text::readable_parts(&mailrs_mime::parse(&raw))
            .next()
            .is_none()
    {
        return Found::NothingReadable;
    }
    let done = state.mailbox.attachment_text_state(mid).ok().flatten();
    if let Some(done) = done.clone()
        && crate::attachment_text::covers(done.version, version)
    {
        return Found::Done(done);
    }
    if state
        .mailbox
        .attachment_text_queued(user, mid)
        .unwrap_or(false)
    {
        return Found::Queued;
    }
    match done {
        Some(d) if crate::attachment_text::lacks_only_qr(d.version, version) => Found::QrOnly,
        Some(_) => Found::Text { read_before: true },
        None => Found::Text { read_before: false },
    }
}

/// Counts of one walk, by what each copy was.
#[derive(Debug, Default)]
pub(super) struct Tally {
    pub walked: u64,
    pub no_file: u64,
    pub with_readable: u64,
    pub already_done: u64,
    pub done_failed: u64,
    pub done_too_large: u64,
    pub done_truncated: u64,
    pub already_queued: u64,
    pub qr_only: u64,
    pub never_read: u64,
    pub outdated_text: u64,
}

impl Tally {
    pub(super) fn add(&mut self, found: &Found) {
        self.walked += 1;
        self.with_readable += u64::from(!matches!(found, Found::NoFile | Found::NothingReadable));
        match found {
            Found::NoFile => self.no_file += 1,
            Found::NothingReadable => {}
            Found::Done(d) => {
                self.already_done += 1;
                self.done_failed += u64::from(d.failed);
                self.done_too_large += u64::from(d.too_large);
                self.done_truncated += u64::from(d.truncated);
            }
            Found::Queued => self.already_queued += 1,
            Found::QrOnly => self.qr_only += 1,
            Found::Text { read_before: true } => self.outdated_text += 1,
            Found::Text { read_before: false } => self.never_read += 1,
        }
    }

    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "messages_walked": self.walked,
            // Read off disk. Far below `messages_walked` means the maildir
            // is not where this process looks, and every count after it is
            // low.
            "files_read": self.walked - self.no_file,
            "no_file": self.no_file,
            "with_readable": self.with_readable,
            "already_done": self.already_done,
            "already_done_failed": self.done_failed,
            "already_done_too_large": self.done_too_large,
            "already_done_truncated": self.done_truncated,
            "already_queued": self.already_queued,
            // behind, by kind; each selector takes its own
            "qr_only": self.qr_only,
            "never_read": self.never_read,
            "outdated_text": self.outdated_text,
        })
    }
}

fn walk(
    state: &Arc<FastcoreState>,
    select: Select,
    q: &BackfillQuery,
    hangup: &super::hangup::Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let version = crate::attachment_text::version_here();
    let mut t = Tally::default();
    let mut queued = 0u64;
    let mut stopped_early = false;
    let after = After::from_query(q.after_user.as_deref(), q.after_thread.as_deref());
    let mut last: Option<After> = None;
    'walk: for user in &users_from(users.clone(), after.as_ref()) {
        // one directory scan per mailbox: finding a flagged file by its id
        // lists cur/ each time, which made a full walk take half an hour
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in threads_after(state, user, after.as_ref()) {
            // a page ends between threads, so the cursor names a whole one
            if t.walked >= q.limit {
                stopped_early = true;
                break 'walk;
            }
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                if hangup.hung_up() {
                    tracing::warn!(
                        walked = t.walked,
                        "backfill-attachment-text: the caller hung up, stopping"
                    );
                    stopped_early = true;
                    break 'walk;
                }
                if (t.walked + 1).is_multiple_of(PAUSE_EVERY) {
                    std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
                }
                let found = classify(state, user, &mid, &on_disk, version);
                t.add(&found);
                if q.dry_run || !found.selected(select) {
                    continue;
                }
                match state.mailbox.queue_attachment_text(user, &mid) {
                    Ok(()) => queued += 1,
                    Err(e) => {
                        tracing::warn!(error = %e, %user, %mid, "queueing attachment text failed")
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
        ?select,
        walked = t.walked,
        queued,
        already_done = t.already_done,
        dry_run = q.dry_run,
        "backfill-attachment-text"
    );
    let mut out = t.json();
    let fields = [
        ("select", serde_json::to_value(select).unwrap_or_default()),
        ("done", (!stopped_early).into()),
        ("next_after", next(&last)),
        ("dry_run", q.dry_run.into()),
        ("accounts", users.len().into()),
        ("version", version.into()),
        // Zero on a dry run, and on a second run over the same mail: the
        // selected kind's count above is what a run would queue.
        ("queued", queued.into()),
    ];
    for (k, v) in fields {
        out[k] = v;
    }
    if select == Select::Text {
        out["backlog"] = super::attachment_backlog::progress(state);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(select: Option<Select>, dry_run: bool) -> BackfillQuery {
        BackfillQuery {
            select,
            dry_run,
            stop: false,
            max_rss_mb: None,
            after_user: None,
            after_thread: None,
            limit: 100,
            pause_ms: 0,
        }
    }

    async fn answer(state: Arc<FastcoreState>, q: BackfillQuery) -> (u16, serde_json::Value) {
        let res = backfill_attachment_text_route(State(state), Query(q)).await;
        let status = res.status().as_u16();
        let body = axum::body::to_bytes(res.into_body(), 1 << 16)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap_or_default())
    }

    /// No accounts answers with zeros that say so, rather than reading as
    /// "walked everything and found nothing".
    #[tokio::test]
    async fn a_backfill_over_no_mail_says_it_walked_nothing() {
        let state = crate::tests::fresh_state();
        let (status, out) = answer(state, query(Some(Select::QrOnly), false)).await;
        assert_eq!(status, 200);
        assert_eq!(out["accounts"], 0, "{out}");
        assert_eq!(out["messages_walked"], 0, "{out}");
        assert_eq!(out["queued"], 0, "{out}");
        assert_eq!(out["done"], true, "{out}");
    }

    /// Without a selector nothing is walked: the two kinds of backlog are
    /// separate calls.
    #[tokio::test]
    async fn a_call_must_say_which_mail_it_is_for() {
        let state = crate::tests::fresh_state();
        let (status, _) = answer(state, query(None, true)).await;
        assert_eq!(status, 400);
    }

    #[test]
    fn a_walk_stops_soon_after_the_caller_hangs_up() {
        use super::super::hangup::{Hangup, seed};
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 5);
        let q = query(Some(Select::QrOnly), true);
        let all = walk(&state, Select::QrOnly, &q, &Hangup::never()).unwrap();
        assert_eq!(all["messages_walked"], 5, "{all}");
        // the seeded copies have no file: walked, and said so
        assert_eq!(all["no_file"], 5, "{all}");
        let cut = walk(&state, Select::QrOnly, &q, &Hangup::after(2)).unwrap();
        assert_eq!(cut["messages_walked"], 2, "{cut}");
        assert_eq!(cut["done"], false, "{cut}");
    }

    #[test]
    fn each_selector_takes_only_its_own_kind() {
        let qr = Found::QrOnly;
        let never = Found::Text { read_before: false };
        let older = Found::Text { read_before: true };
        assert!(qr.selected(Select::QrOnly) && !qr.selected(Select::Text));
        assert!(never.selected(Select::Text) && !never.selected(Select::QrOnly));
        assert!(older.selected(Select::Text));
        for other in [Found::NoFile, Found::NothingReadable, Found::Queued] {
            assert!(!other.selected(Select::QrOnly) && !other.selected(Select::Text));
        }
    }
}
