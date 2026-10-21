//! The steps of an arrival that each decide or write one thing.

use std::sync::Arc;

use crate::FastcoreState;
use crate::headers::resolve_thread_by_ancestry;

/// The conversation this message belongs to.
pub(super) fn thread_root(
    state: &Arc<FastcoreState>,
    addr: &str,
    message_id: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> String {
    // v2.9.5 threading fix — prefer the thread an ancestor actually
    // landed in (msgid index) over deriving one from raw headers.
    // References[0] is NOT a stable conversation root (each hop can
    // rewrite it), which is how conversations fragmented.
    match resolve_thread_by_ancestry(state, addr, message_id, in_reply_to, references, subject) {
        Some(tid) => tid,
        None => {
            if let Some(first) = references.first() {
                first.clone()
            } else if !in_reply_to.is_empty() {
                in_reply_to.to_string()
            } else {
                message_id.to_string()
            }
        }
    }
}

/// The per-thread category the arrival is filed under.
pub(super) fn arrival_category(
    state: &Arc<FastcoreState>,
    target_folder: &str,
    body: &[u8],
) -> &'static str {
    // v2.4.0 Phase 2 (RFC-A) — plumb the SMTP-level target_folder
    // decision (from `crates/receiver/src/smtp_session/events/data/antispam.rs`
    // where DeliveryDecision::Junk yields target_folder="Junk") into the
    // per-thread category. mailbox-kevy's `upsert_thread` reads
    // `category ∈ {"spam", "scam"}` as the Junk-zset trigger, so
    // stamping "spam" here makes the antispam verdict actually route
    // to the Junk folder on the read side. Any sieve fileinto target
    // that maps to "Junk" is treated the same. Everything else
    // (INBOX / custom sieve folders) keeps category="inbox".
    // v2.9 triage — non-junk mail is further sorted into
    // inbox/notification/promotion by the multi-class Bayes classifier
    // (`bucket_of` then routes it to the matching folder zset).
    // Cold-start / low-confidence → "inbox".
    if target_folder.eq_ignore_ascii_case("junk") {
        "spam"
    } else {
        crate::bayes_train::classify_triage(state, body).unwrap_or("inbox")
    }
}

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
