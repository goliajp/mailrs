//! The run over mail whose attachment text predates OCR or the Office
//! readers, or was never read: hundreds of messages, each read again with
//! OCR, on a four-core host beside the live mail service.
//!
//! So it goes a few at a time. A batch is the next [`BATCH`] such copies
//! after the cursor, put on the attachment-text queue; the run waits until
//! the worker has taken all of them, then moves the cursor past them and
//! saves it. The progress lives in the embedded store, so the run picks up
//! after a restart where it stopped, and it can be stopped and started
//! again by the maintenance route. Before each batch, and while it waits,
//! it reads fastcore's own resident memory; past the limit (default
//! [`MAX_RSS`]) it pauses and stays paused until started again. Memory it
//! cannot read pauses it too: the stop condition must be checkable.
//!
//! It converges: a copy read at this generation is skipped, so a run
//! started after one finished walks everything and queues nothing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::backfill_attachment_text::{PAUSE_EVERY, Select, Tally, classify};
use super::prelude::*;
use super::thread_cursor::{After, threads_after, users_from};

/// Copies queued per batch: one worker round's worth.
pub(crate) const BATCH: usize = 8;
/// Messages walked per batch at most, so a long stretch with nothing to
/// read still saves its cursor.
const WALK_PER_BATCH: u64 = 2000;
/// Fastcore's resident memory the run pauses at: 3.5 GiB, under the
/// container's 4 GiB.
pub(crate) const MAX_RSS: u64 = 3584 * 1024 * 1024;
const _: () = assert!(MAX_RSS < 4 * 1024 * 1024 * 1024);
/// How often a waiting run looks at the queue and the memory.
const POLL: Duration = Duration::from_secs(2);
/// Pause between messages walked, every [`PAUSE_EVERY`].
const WALK_PAUSE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunState {
    #[default]
    Running,
    Stopped,
    PausedMemory,
    Done,
}

/// The saved progress of the run.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Progress {
    pub state: RunState,
    /// The last thread whose copies are all read or skipped.
    pub after_user: Option<String>,
    pub after_thread: Option<String>,
    pub max_rss_bytes: u64,
    /// Fastcore's resident memory at the last reading; `None` when it
    /// could not be read.
    pub rss_bytes: Option<u64>,
    pub messages_walked: u64,
    pub queued: u64,
    pub never_read: u64,
    pub outdated_text: u64,
    pub already_done: u64,
    pub already_queued: u64,
    pub no_file: u64,
    pub batches: u64,
    pub started_at: i64,
    pub updated_at: i64,
}

/// One task per process; a second start while it runs only says so.
static ALIVE: AtomicBool = AtomicBool::new(false);
/// Every read-change-write of the saved progress.
static SAVE: Mutex<()> = Mutex::new(());

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn load(state: &FastcoreState) -> Option<Progress> {
    let raw = state.mailbox.attachment_backlog().ok().flatten()?;
    match serde_json::from_str(&raw) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::error!(error = %e, "attachment backlog: saved progress unreadable");
            None
        }
    }
}

fn save(state: &FastcoreState, p: &mut Progress) {
    p.updated_at = now();
    let raw = serde_json::to_string(p).unwrap_or_default();
    if let Err(e) = state.mailbox.set_attachment_backlog(&raw) {
        tracing::warn!(error = %e, "attachment backlog: progress not saved");
    }
}

/// Change the saved progress under the lock; `None` when there is none.
fn update(state: &FastcoreState, f: impl FnOnce(&mut Progress)) -> Option<Progress> {
    let _held = SAVE.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = load(state)?;
    f(&mut p);
    save(state, &mut p);
    Some(p)
}

/// This process's resident memory, in bytes, from `/proc/self/status`.
pub(crate) fn own_rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

fn answer(p: Option<Progress>) -> serde_json::Value {
    let mut out = serde_json::to_value(p).unwrap_or_default();
    if out.is_object() {
        out["alive"] = ALIVE.load(Ordering::SeqCst).into();
    }
    out
}

/// The run's saved progress, `null` before the first.
pub(crate) fn progress(state: &FastcoreState) -> serde_json::Value {
    answer(load(state))
}

/// Start the run, or resume it from its cursor; a finished run starts over
/// (and, every copy read, queues nothing).
pub(crate) fn start(state: &Arc<FastcoreState>, max_rss_bytes: u64) -> serde_json::Value {
    let p = {
        let _held = SAVE.lock().unwrap_or_else(|e| e.into_inner());
        let mut p = match load(state) {
            Some(p) if p.state != RunState::Done => p,
            _ => Progress {
                started_at: now(),
                ..Progress::default()
            },
        };
        p.state = RunState::Running;
        p.max_rss_bytes = max_rss_bytes;
        save(state, &mut p);
        p
    };
    spawn(state.clone());
    answer(Some(p))
}

/// Stop the run after the batch in hand; the cursor stays for a resume.
pub(crate) fn stop(state: &FastcoreState) -> serde_json::Value {
    answer(update(state, |p| {
        if p.state == RunState::Running {
            p.state = RunState::Stopped;
        }
    }))
}

/// At boot: carry on with a run that was going when the process stopped.
pub(crate) fn resume_on_boot(state: Arc<FastcoreState>) {
    if load(&state).is_some_and(|p| p.state == RunState::Running) {
        tracing::info!("attachment backlog: resuming the run");
        spawn(state);
    }
}

fn spawn(state: Arc<FastcoreState>) {
    if ALIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        loop {
            run(&state).await;
            ALIVE.store(false, Ordering::SeqCst);
            // a start that came while this one was ending found it alive
            let again = load(&state).is_some_and(|p| p.state == RunState::Running);
            if !again || ALIVE.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    });
}

/// Still running, and the memory under the limit; otherwise the saved
/// state says why not, and the run ends.
fn may_go_on(state: &FastcoreState) -> bool {
    let rss = own_rss();
    let p = update(state, |p| {
        p.rss_bytes = rss;
        if p.state == RunState::Running && rss.is_none_or(|r| r > p.max_rss_bytes) {
            tracing::error!(
                event = "attachment_backlog_paused",
                rss = ?rss,
                limit = p.max_rss_bytes,
                "attachment backlog: fastcore's memory is past the limit or unreadable; paused"
            );
            p.state = RunState::PausedMemory;
        }
    });
    p.is_some_and(|p| p.state == RunState::Running)
}

async fn run(state: &Arc<FastcoreState>) {
    loop {
        if !may_go_on(state) {
            return;
        }
        let Some(p) = load(state) else { return };
        let after = After::from_query(p.after_user.as_deref(), p.after_thread.as_deref());
        let walker = state.clone();
        let batch = match tokio::task::spawn_blocking(move || next_batch(&walker, after)).await {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => {
                tracing::error!(error = %e, "attachment backlog: listing accounts failed");
                return;
            }
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        };
        let mut queued = 0u64;
        for (user, mid) in &batch.items {
            match state.mailbox.queue_attachment_text(user, mid) {
                Ok(()) => queued += 1,
                Err(e) => tracing::warn!(error = %e, %user, %mid, "attachment backlog: not queued"),
            }
        }
        update(state, |p| p.queued += queued);
        // the cursor moves only past a batch the worker has taken
        while batch
            .items
            .iter()
            .any(|(u, m)| state.mailbox.attachment_text_queued(u, m).unwrap_or(false))
        {
            tokio::time::sleep(POLL).await;
            if !may_go_on(state) {
                return;
            }
        }
        update(state, |p| {
            let t = &batch.tally;
            p.messages_walked += t.walked;
            p.never_read += t.never_read;
            p.outdated_text += t.outdated_text;
            p.already_done += t.already_done;
            p.already_queued += t.already_queued;
            p.no_file += t.no_file;
            p.batches += 1;
            if let Some(last) = &batch.last {
                p.after_user = Some(last.user.clone());
                p.after_thread = Some(last.thread.clone());
            }
            if batch.done && p.state == RunState::Running {
                p.state = RunState::Done;
            }
        });
        if batch.done {
            tracing::info!("attachment backlog: done");
            return;
        }
    }
}

/// The next copies to read after a cursor.
struct Batch {
    items: Vec<(String, String)>,
    tally: Tally,
    /// The last thread walked whole.
    last: Option<After>,
    /// The walk reached the end of every mailbox.
    done: bool,
}

fn next_batch(state: &Arc<FastcoreState>, after: Option<After>) -> std::io::Result<Batch> {
    let users = state.mailbox.list_account_addresses()?;
    let version = crate::attachment_text::version_here();
    let mut b = Batch {
        items: Vec::new(),
        tally: Tally::default(),
        last: None,
        done: true,
    };
    'walk: for user in &users_from(users, after.as_ref()) {
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in threads_after(state, user, after.as_ref()) {
            // a batch ends between threads, so the cursor names a whole one
            if b.items.len() >= BATCH || b.tally.walked >= WALK_PER_BATCH {
                b.done = false;
                break 'walk;
            }
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                if (b.tally.walked + 1).is_multiple_of(PAUSE_EVERY) {
                    std::thread::sleep(WALK_PAUSE);
                }
                let found = classify(state, user, &mid, &on_disk, version);
                b.tally.add(&found);
                if found.selected(Select::Text) {
                    b.items.push((user.clone(), mid));
                }
            }
            b.last = Some(After {
                user: user.clone(),
                thread: tid,
            });
        }
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_written_by_one_version_reads_back() {
        let p = Progress {
            state: RunState::PausedMemory,
            after_user: Some("u@x".into()),
            after_thread: Some("t9".into()),
            max_rss_bytes: MAX_RSS,
            rss_bytes: Some(3_800_000_000),
            queued: 8,
            ..Progress::default()
        };
        let raw = serde_json::to_string(&p).unwrap();
        assert!(raw.contains(r#""state":"paused_memory""#), "{raw}");
        assert_eq!(serde_json::from_str::<Progress>(&raw).unwrap(), p);
    }

    /// A walk over mail with nothing to read finishes the run at once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_over_nothing_to_read_is_done_and_queues_nothing() {
        let state = crate::tests::fresh_state();
        super::super::hangup::seed(&state, "u@x", 3);
        assert_eq!(progress(&state), serde_json::Value::Null);
        start(&state, u64::MAX);
        for _ in 0..50 {
            if load(&state).is_some_and(|p| p.state != RunState::Running) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let p = load(&state).unwrap();
        // on a host without /proc the memory cannot be read: paused, said so
        match own_rss() {
            Some(_) => {
                assert_eq!(p.state, RunState::Done, "{p:?}");
                assert_eq!(p.messages_walked, 3, "{p:?}");
                assert_eq!(p.no_file, 3, "{p:?}");
                assert_eq!(p.queued, 0, "{p:?}");
            }
            None => assert_eq!(p.state, RunState::PausedMemory, "{p:?}"),
        }
    }
}
