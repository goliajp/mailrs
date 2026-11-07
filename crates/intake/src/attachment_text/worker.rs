//! One bounded round over the queue.

use std::sync::Arc;

use mailrs_mailbox::AttachmentTextState;

use super::{ATTEMPTS, AttachmentTextStore, BATCH, OcrSetup, PAUSE};
use super::{current_version, extract, numbers, process_ocr, qr_shadow};

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
    /// Messages with a QR code carrying a web address.
    pub qr_codes: u64,
    /// Of those, a code to a host the message never names.
    pub qr_host_not_in_message: u64,
    /// Of those, the image prints a domain the code does not go to.
    pub qr_text_mismatch: u64,
    /// Messages whose text was current and whose QR codes alone were read.
    pub qr_refreshed: u64,
    /// Items the process died reading [`ATTEMPTS`] times, given up on.
    pub set_aside: u64,
}

/// Take one bounded batch off the queue and process it, with OCR as this
/// process is set up for it.
pub async fn run_once<S: AttachmentTextStore>(store: &Arc<S>) -> Round {
    let ocr = process_ocr();
    let mut round = Round::default();
    let batch = match store.pending(BATCH).await {
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
        match store.begin(&user, &mid).await {
            Ok(n) if n > ATTEMPTS => {
                set_aside(store, &user, &mid, n - 1, ocr, &mut round).await;
                continue;
            }
            Ok(_) => {}
            // unprotected rather than not read: the count is what is lost
            Err(e) => {
                tracing::warn!(error = %e, %user, %mid, "attachment text: attempt not recorded")
            }
        }
        process(store, &user, &mid, ocr, &mut round).await;
        if let Err(e) = store.dequeue(&user, &mid).await {
            tracing::warn!(error = %e, %user, %mid, "attachment text: dequeue failed");
        }
    }
    if round.taken > 0 {
        tracing::info!(?round, "attachment text round");
    }
    round
}

/// `user`'s copy was started `interrupted` times and the process died each
/// time: stored as failed, so search and the sweep see it was tried, and
/// taken off the queue into the set-aside set.
async fn set_aside<S: AttachmentTextStore>(
    store: &Arc<S>,
    user: &str,
    mid: &str,
    interrupted: u32,
    ocr: Option<&OcrSetup>,
    round: &mut Round,
) {
    tracing::error!(
        event = "attachment_text_set_aside",
        %user,
        %mid,
        interrupted,
        "attachment text: the process stopped reading this message every time; set aside"
    );
    let failed = AttachmentTextState {
        version: current_version(ocr),
        failed: 1,
        ..Default::default()
    };
    if let Err(e) = store.store(mid, &[], &failed).await {
        tracing::warn!(error = %e, %user, %mid, "attachment text: storing failed");
    }
    match store.set_aside(user, mid).await {
        Ok(()) => round.set_aside += 1,
        Err(e) => tracing::warn!(error = %e, %user, %mid, "attachment text: set aside failed"),
    }
}

async fn process<S: AttachmentTextStore>(
    store: &Arc<S>,
    user: &str,
    mid: &str,
    ocr: Option<&OcrSetup>,
    round: &mut Round,
) {
    let Some(raw) = store.raw(user, mid).await else {
        round.no_file += 1;
        return;
    };
    let now = current_version(ocr);
    let done = store.state(mid).await.ok().flatten();
    if let Some(done) = done {
        if extract::covers(done.version, now) {
            round.already_done += 1;
            if let Err(e) = store.share(user, mid).await {
                tracing::warn!(error = %e, %user, %mid, "attachment text: sharing failed");
            }
            // read for another recipient: this one's history still gets
            // the numbers
            let texts = store.texts(mid).await.unwrap_or_default();
            numbers::record(store, user, mid, raw, texts).await;
            return;
        }
        if let Some(setup) = ocr
            && extract::lacks_only_qr(done.version, now)
        {
            let version = done.version | extract::WITH_QR;
            refresh_qr(store, user, mid, raw, setup, version, round).await;
            return;
        }
    }
    let setup = ocr.cloned();
    let read = move || {
        let (texts, outcome, urls) = extract::extract_all(&raw, setup.as_ref());
        let qr = qr_shadow::assess(&raw, &texts, &urls);
        (raw, texts, outcome, urls, qr)
    };
    // the parsers run in child processes; a panic here is this code's own
    let (raw, texts, outcome, urls, qr) = match tokio::task::spawn_blocking(read).await {
        Ok(done) => done,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    };
    if let Err(e) = store.store(mid, &texts, &outcome).await {
        tracing::warn!(error = %e, %user, %mid, "attachment text: storing failed");
        return;
    }
    if let Err(e) = store.store_qr(mid, &urls).await {
        tracing::warn!(error = %e, %user, %mid, "attachment text: storing qr codes failed");
    }
    round.extracted += 1;
    round.stored += u64::from(outcome.stored);
    round.ocr += u64::from(outcome.ocr);
    round.failed += u64::from(outcome.failed);
    round.too_large += u64::from(outcome.too_large);
    round.too_small += u64::from(outcome.too_small);
    round.truncated += u64::from(outcome.truncated);
    count_qr(user, mid, qr, round);
    let texts: Vec<String> = texts.into_iter().map(|(_, t)| t).collect();
    numbers::record(store, user, mid, raw, texts).await;
}

/// Read only the QR codes of a message whose text is current, store their
/// addresses, and mark it read at `version`.
async fn refresh_qr<S: AttachmentTextStore>(
    store: &Arc<S>,
    user: &str,
    mid: &str,
    raw: Vec<u8>,
    setup: &OcrSetup,
    version: u32,
    round: &mut Round,
) {
    let texts = store.texts(mid).await.unwrap_or_default();
    let setup = setup.clone();
    let numbered: Vec<(usize, String)> = texts.iter().cloned().enumerate().collect();
    let read = move || {
        let urls = extract::qr_only(&raw, &setup);
        let qr = qr_shadow::assess(&raw, &numbered, &urls);
        (raw, urls, qr)
    };
    let (raw, urls, qr) = match tokio::task::spawn_blocking(read).await {
        Ok(done) => done,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    };
    if let Err(e) = store.refresh_qr(mid, &urls, version).await {
        tracing::warn!(error = %e, %user, %mid, "attachment text: storing qr codes failed");
        return;
    }
    if let Err(e) = store.share(user, mid).await {
        tracing::warn!(error = %e, %user, %mid, "attachment text: sharing failed");
    }
    round.qr_refreshed += 1;
    count_qr(user, mid, qr, round);
    numbers::record(store, user, mid, raw, texts).await;
}

fn count_qr(user: &str, mid: &str, qr: Option<mailrs_fraud::qr::QrFacts>, round: &mut Round) {
    let Some(qr) = qr else { return };
    tracing::info!(
        event = "qr_shadow",
        %user,
        %mid,
        codes = qr.codes,
        host_not_in_message = qr.host_not_in_message,
        text_mismatch = qr.text_mismatch,
        "attachment text: qr codes"
    );
    round.qr_codes += 1;
    round.qr_host_not_in_message += u64::from(qr.host_not_in_message);
    round.qr_text_mismatch += u64::from(qr.text_mismatch);
}
