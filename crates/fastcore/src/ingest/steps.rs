//! The steps of an arrival that each decide or write one thing.

use std::sync::Arc;

use crate::FastcoreState;

/// Bump the mailbox modseq, then allocate and record this message's UID.
pub(super) fn assign_uid(
    state: &Arc<FastcoreState>,
    addr: &str,
    bare: &str,
    message_id: &str,
    blob_ref: &str,
) -> u32 {
    let m = crate::imap::backend::bump_modseq(state, addr);
    crate::imap::backend::set_file_modseq(state, addr, bare, m);
    let uid = state.mailbox.allocate_uid(addr, message_id).unwrap_or(0);
    // The maildir's copy of the promise. Appended rather than read-checked:
    // delivery is the hot path, the list is append-only by design, and the
    // self-heal reads it back before allocating anything.
    crate::uidlist::record(addr, uid, blob_ref);
    uid
}

/// The shared message blob and this user's own row for it.
pub(super) fn write_message_row(
    state: &Arc<FastcoreState>,
    addr: &str,
    root: &str,
    message_id: &str,
    date: i64,
    blob_ref: &str,
    wire: &mailrs_core_api::method::message::MessageWire,
) {
    match serde_json::to_vec(wire) {
        Ok(payload) => {
            // The shared blob plus this user's own row: their maildir
            // filename, their uid, their flags. A thread can have several
            // owners and each has a different file on disk, so a single
            // `blob_ref` on the shared blob is one owner's — 74 messages on
            // production were served to a user the row did not name.
            if let Err(e) = state.mailbox.upsert_user_message(
                addr,
                root,
                message_id,
                date,
                &payload,
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref,
                    uid: wire.uid,
                    flags: wire.flags,
                    modseq: wire.modseq,
                },
            ) {
                tracing::warn!(error = %e, %addr, %root, "drain ingest: upsert_user_message failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "drain ingest: wire serialize failed"),
    }
}

/// Store the receiver's fraud verdict and hold the thread when it says to.
/// Returns whether the thread was held.
pub(super) fn hold_if_fraud(
    state: &Arc<FastcoreState>,
    addr: &str,
    root: &str,
    message_id: &str,
    fraud_verdict: Option<&str>,
) -> bool {
    // The finding, and the hold it implies. After the arrival, because
    // `set_quarantined` writes the membership row and the arrival is
    // what creates it — held before it exists is held on nothing.
    let mut held = false;
    if let Some(json) = fraud_verdict {
        if let Err(e) = state.mailbox.set_fraud_verdict(message_id, json) {
            tracing::warn!(error = %e, %addr, %message_id, "storing the fraud verdict failed");
        }
        match serde_json::from_str::<mailrs_inbound::FraudVerdict>(json) {
            Ok(v) if v.quarantined => match state.mailbox.set_quarantined(addr, root, true) {
                Ok(true) => {
                    // Read, at the same moment. A held conversation
                    // that is also unread is bold, adds to the badge
                    // and rings the phone — the attempt to defraud
                    // getting exactly the attention it was sent to
                    // get, which is what holding exists to remove.
                    //
                    // Both halves, through the function that writes
                    // both: the axis column and the per-user rows the
                    // count comes off.
                    crate::routes::thread_actions::mark_thread_read_everywhere(state, addr, root);
                    held = true;
                    tracing::info!(
                        %addr, %root, score = v.score, rules = %v.rules_version,
                        "held: suspected fraud"
                    )
                }
                Ok(false) => tracing::warn!(
                    %addr, %root,
                    "held nothing: no membership row to hold"
                ),
                Err(e) => tracing::warn!(error = %e, %addr, %root, "holding failed"),
            },
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, %addr, "the stored verdict did not parse"),
        }
    }
    held
}

/// Index the body text for search and queue readable attachments.
pub(super) fn index_contents(
    state: &Arc<FastcoreState>,
    addr: &str,
    message_id: &str,
    body_text: Option<&str>,
    parsed: &mailrs_mime::Part<'_>,
) {
    // Index the body for full-text search. Costs one MIME parse on a
    // path that already has the bytes in hand, and it is what makes
    // search cover message contents rather than just headers.
    if let Some(text) = body_text
        && let Err(e) = state.mailbox.index_message_text(message_id, text)
    {
        tracing::warn!(error = %e, %addr, %message_id, "index_message_text failed");
    }
    // A PDF or image is only noted here; reading it is the worker's job,
    // off this path.
    crate::attachment_text::queue_if_it_has_readable_parts(state, addr, message_id, parsed);
}
