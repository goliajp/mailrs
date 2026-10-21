//! Text from PDF and image attachments, read after delivery.
//!
//! Delivery only queues a message that carries a PDF or an image
//! (`queue_if_it_has_readable_parts`). This worker takes a few queued
//! messages at a time, reads each one's file, extracts the text off the
//! async runtime (OCR in a child process, see `ocr`), and stores it where
//! search and the fraud sweep read it. Every item leaves the queue after
//! one attempt, whatever came of it, and the stored state says what did.
//!
//! **It does not re-judge.** No shipped rule reads the attachment facts,
//! and a re-judge here would act per conversation with today's domain
//! counters — moving old verdicts for reasons that have nothing to do with
//! the text. A rule that does read them reaches stored mail the way every
//! rule change does: its new version starts `fraud_backfill`, whose sweep
//! reads the stored text.
//!
//! **Bounded, and it pauses.** The embedded store has one lock; a loop that
//! reaches for it without stopping starves every reader behind it, which is
//! how an unbounded sweep took the mail service down on 2026-08-26.

use std::sync::Arc;
use std::time::Duration;

use crate::FastcoreState;

mod extract;
mod ocr;

pub use extract::{MAX_PDF_BYTES, VERSION_PDF_ONLY, VERSION_WITH_OCR};
pub(crate) use extract::{may_carry_readable, readable_parts};
pub(crate) use ocr::OcrSetup;

/// Messages taken per round.
const BATCH: usize = 8;
/// Wait between messages inside a round.
const PAUSE: Duration = Duration::from_millis(100);
/// Wait after a round that had work.
const BUSY_INTERVAL: Duration = Duration::from_secs(5);
/// Longest wait while the queue stays empty.
const IDLE_INTERVAL: Duration = Duration::from_secs(300);

/// Queue `user`'s copy of the message if any part is a PDF or an image.
pub(crate) fn queue_if_it_has_readable_parts(
    state: &FastcoreState,
    user: &str,
    message_id: &str,
    parsed: &mailrs_mime::Part<'_>,
) {
    if readable_parts(parsed).next().is_some()
        && let Err(e) = state.mailbox.queue_attachment_text(user, message_id)
    {
        tracing::warn!(error = %e, %user, %message_id, "queueing attachment text failed");
    }
}

/// The extractor generation this process writes.
pub(crate) fn version_here() -> u32 {
    current_version(process_ocr())
}

/// OCR as this process found it at first use: on, or off with the reason
/// logged once.
fn process_ocr() -> Option<&'static OcrSetup> {
    static SETUP: std::sync::OnceLock<Option<OcrSetup>> = std::sync::OnceLock::new();
    SETUP
        .get_or_init(|| match ocr::setup() {
            Ok(s) => {
                tracing::info!(bin = %s.bin.display(), models = %s.models.display(), "attachment text: ocr on");
                Some(s)
            }
            Err(reason) => {
                tracing::warn!(%reason, "attachment text: ocr off, images and scans are not read");
                None
            }
        })
        .as_ref()
}

/// What one round did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Round {
    /// Items taken off the queue.
    pub taken: u64,
    /// Messages whose attachments were read this round.
    pub extracted: u64,
    /// Attachments whose text was stored.
    pub stored: u64,
    /// Of the stored, read by OCR.
    pub ocr: u64,
    /// Attachments that could not be read.
    pub failed: u64,
    /// Attachments over a size limit, not read.
    pub too_large: u64,
    /// Images too small to carry text.
    pub too_small: u64,
    /// Attachments whose text was cut at the per-attachment cap.
    pub truncated: u64,
    /// Items whose message had already been read for another recipient.
    pub already_done: u64,
    /// Items whose copy or file is gone.
    pub no_file: u64,
}

/// Take one bounded batch off the queue and process it, with OCR as this
/// process is set up for it.
pub async fn run_once(state: &Arc<FastcoreState>) -> Round {
    let ocr = process_ocr();
    let mut round = Round::default();
    let batch = match state.mailbox.pending_attachment_texts(BATCH) {
        Ok(batch) => batch,
        Err(e) => {
            tracing::warn!(error = %e, "attachment text: reading the queue failed");
            return round;
        }
    };
    for (i, (user, mid)) in batch.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(PAUSE).await;
        }
        round.taken += 1;
        process(state, &user, &mid, ocr, &mut round).await;
        if let Err(e) = state.mailbox.dequeue_attachment_text(&user, &mid) {
            tracing::warn!(error = %e, %user, %mid, "attachment text: dequeue failed");
        }
    }
    if round.taken > 0 {
        tracing::info!(?round, "attachment text round");
    }
    round
}

async fn process(
    state: &Arc<FastcoreState>,
    user: &str,
    mid: &str,
    ocr: Option<&OcrSetup>,
    round: &mut Round,
) {
    let Ok(Some(copy)) = state.mailbox.user_message_facts(user, mid) else {
        round.no_file += 1;
        return;
    };
    if let Ok(Some(done)) = state.mailbox.attachment_text_state(mid)
        && done.version >= current_version(ocr)
    {
        round.already_done += 1;
        return;
    }
    let Some(raw) = crate::maildir_scan::read_maildir_file(user, &copy.blob_ref) else {
        round.no_file += 1;
        return;
    };
    let setup = ocr.cloned();
    let (texts, outcome) =
        match tokio::task::spawn_blocking(move || extract::extract_all(&raw, setup.as_ref())).await
        {
            Ok(done) => done,
            Err(e) => {
                tracing::warn!(error = %e, %user, %mid, "attachment text: extraction task failed");
                let failed = mailrs_mailbox_kevy::AttachmentTextState {
                    version: current_version(ocr),
                    failed: 1,
                    ..Default::default()
                };
                (Vec::new(), failed)
            }
        };
    if let Err(e) = state.mailbox.store_attachment_texts(mid, &texts, &outcome) {
        tracing::warn!(error = %e, %user, %mid, "attachment text: storing failed");
        return;
    }
    round.extracted += 1;
    round.stored += u64::from(outcome.stored);
    round.ocr += u64::from(outcome.ocr);
    round.failed += u64::from(outcome.failed);
    round.too_large += u64::from(outcome.too_large);
    round.too_small += u64::from(outcome.too_small);
    round.truncated += u64::from(outcome.truncated);
}

/// The extractor generation written with `ocr`: a message stored by an
/// older one is read again.
fn current_version(ocr: Option<&OcrSetup>) -> u32 {
    if ocr.is_some() {
        return VERSION_WITH_OCR;
    }
    VERSION_PDF_ONLY
}

/// Run [`run_once`] for the life of the process, backing off while the
/// queue stays empty.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(async move {
        let mut idle_rounds: u32 = 0;
        loop {
            let round = run_once(&state).await;
            idle_rounds = match round.taken {
                0 => idle_rounds.saturating_add(1),
                _ => 0,
            };
            tokio::time::sleep(wait_after(idle_rounds)).await;
        }
    });
}

/// The wait `spawn` takes after `idle_rounds` empty rounds in a row.
pub fn wait_after(idle_rounds: u32) -> Duration {
    crate::idle_backoff::idle_backoff(BUSY_INTERVAL, IDLE_INTERVAL, idle_rounds)
}
