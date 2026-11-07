//! Text from PDF, image and Office attachments, read after delivery.
//!
//! Delivery only queues a copy whose message carries a readable part
//! ([`has_readable_parts`]). This worker takes a few queued copies at a
//! time, reads each one's file, extracts the text off the async runtime
//! (OCR in a child process, see `ocr`), and stores it where search and the
//! fraud sweep read it. Every item leaves the queue after one attempt,
//! whatever came of it, and the stored state says what did. An attempt is
//! counted on disk before it starts, so an item the process dies on (a
//! parser that aborts) is given up on after [`ATTEMPTS`] tries instead of
//! taking the process down at every restart.
//!
//! The queue and the stored text are the core's own
//! ([`AttachmentTextStore`]); everything else is here, once.
//!
//! **It does not re-judge.** No shipped rule reads the attachment facts,
//! and a re-judge here would act per conversation with today's domain
//! counters — moving old verdicts for reasons that have nothing to do with
//! the text. A rule that does read them reaches stored mail the way every
//! rule change does: through the fraud sweep, which reads the stored text.
//!
//! **Bounded, and it pauses.** The kevy core's embedded store has one
//! lock; a loop that reaches for it without stopping starves every reader
//! behind it, which is how an unbounded sweep took the mail service down
//! on 2026-08-26.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

mod extract;
mod numbers;
mod ocr;
mod qr_shadow;
mod store;
mod worker;

pub use extract::{
    MAX_PDF_BYTES, VERSION_PDF_ONLY, VERSION_WITH_OCR, WITH_OFFICE, WITH_QR, covers, lacks_only_qr,
    may_carry_readable,
};
pub use ocr::OcrSetup;
pub use store::{
    AttachmentTextStore, QR_MAX, TEXT_CAP, cap_text, check_attachment_text_store, kept_qr_urls,
};
pub use worker::{Round, run_once};

/// Messages taken per round.
const BATCH: usize = 8;
/// Wait between messages inside a round.
const PAUSE: Duration = Duration::from_millis(100);
/// Wait after a round that had work.
const BUSY_INTERVAL: Duration = Duration::from_secs(5);
/// Longest wait while the queue stays empty.
const IDLE_INTERVAL: Duration = Duration::from_secs(300);
/// Interrupted attempts at one item before it is given up on. The same
/// reasoning as the spool's: a deploy or another task's OOM can interrupt
/// an innocent item once, and three in a row on one item is the item.
/// Only the in-process readers (PDF, Office) can take the process down;
/// OCR runs in a child that is killed alone.
pub const ATTEMPTS: u32 = 3;

/// The never-list with this server's domains: a host on it never gets an
/// automatic reputation change, so mail is never found by it.
pub static NEVER: LazyLock<mailrs_reputation::NeverList> = LazyLock::new(|| {
    let own: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    mailrs_reputation::NeverList::builtin(&own)
});

/// Whether delivery queues a copy of this message: some part is a PDF, an
/// image or an Office file.
pub fn has_readable_parts(parsed: &mailrs_mime::Part<'_>) -> bool {
    extract::readable_parts(parsed).next().is_some()
}

/// Whether `raw` has a readable part, parsed in full; check
/// [`may_carry_readable`] on the bytes first to skip the parse.
pub fn reads_anything(raw: &[u8]) -> bool {
    has_readable_parts(&mailrs_mime::parse(raw))
}

/// The extractor generation this process writes.
pub fn version_here() -> u32 {
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

/// The extractor generation written with `ocr`: a message stored by an
/// older one is read again.
fn current_version(ocr: Option<&OcrSetup>) -> u32 {
    if ocr.is_some() {
        return VERSION_WITH_OCR | WITH_OFFICE | WITH_QR;
    }
    VERSION_PDF_ONLY | WITH_OFFICE
}

/// Run [`run_once`] for the life of the process, backing off while the
/// queue stays empty.
pub fn spawn<S: AttachmentTextStore>(store: Arc<S>) {
    tokio::spawn(async move {
        let mut idle_rounds: u32 = 0;
        loop {
            let round = run_once(&store).await;
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
