//! One thread's repair: its row decoded and rewritten, its messages'
//! bodies indexed, and an empty preview filled from the newest of them.

use mailrs_core_api::method::message::MessageWire;

use super::Counts;
use crate::FastcoreState;

pub(super) fn repair_thread(state: &FastcoreState, user: &str, tid: &str, c: &mut Counts) {
    let Ok(Some(mut row)) = state.mailbox.get_thread(tid) else {
        return;
    };
    let mut newest: Option<(i64, String)> = None;
    let senders = mailrs_rfc2047::decode(row.senders_csv.as_bytes()).into_owned();
    let subject = mailrs_rfc2047::decode(row.subject.as_bytes()).into_owned();
    // Re-run through `preview_line`, not only decoded: rows
    // written before it learned about rule lines carry the bar
    // a plain-text mail draws across the page —
    // `Hello HAO, ------------------------------ …` opened
    // nearly every row on a phone. Re-running over the stored
    // string is enough, because the dashes are still in it as
    // dashes; the line only gets shorter, never wrong.
    let preview =
        mailrs_clean::preview_line(&mailrs_rfc2047::decode(row.latest_preview.as_bytes()), 120);
    // Rows written before the text index existed carry no
    // `search_blob`, so they are invisible to search until
    // something rewrites them. upsert_thread synthesises the
    // field, so re-writing such a row is all it takes.
    let needs_blob = !state
        .mailbox
        .store_ref()
        .hexists(
            mailrs_mailbox_kevy::keys::thread(tid).as_bytes(),
            mailrs_mailbox_kevy::keys::THREAD_SEARCH_FIELD,
        )
        .unwrap_or(false);
    let dirty = needs_blob
        || senders != row.senders_csv
        || subject != row.subject
        || preview != row.latest_preview;
    if dirty {
        row.senders_csv = senders;
        row.subject = subject;
        row.latest_preview = preview;
        if let Err(e) = state.mailbox.upsert_thread(user, &row) {
            tracing::warn!(err = %e, %user, %tid, "decode backfill: upsert failed");
            return;
        }
        if needs_blob {
            c.blobs_added += 1;
        }
        c.rows_decoded += 1;
        crate::live_sync::upsert_contacts(user, &row.senders_csv);
    }
    // Body text for the message-level search index. Reads each
    // message's maildir file once — the heaviest part of this
    // sweep, and the reason it is an explicit admin action
    // rather than something the ingest path retrofits.
    for blob in state
        .mailbox
        .thread_messages_for_maintenance(tid)
        .unwrap_or_default()
    {
        let Ok(w) = serde_json::from_slice::<MessageWire>(&blob) else {
            continue;
        };
        index_message(state, user, tid, w, &mut newest, c);
    }
    // Only when there is nothing there. Every received thread
    // written before 2026-08-09 has an empty one — the drain
    // passed "" — and a thread that already has a line does not
    // need this sweep's opinion of it.
    if !row.latest_preview.is_empty() {
        c.previews_present += 1;
    }
    if row.latest_preview.is_empty()
        && let Some((_, preview)) = newest
        && !preview.is_empty()
    {
        row.latest_preview = preview;
        if state.mailbox.upsert_thread(user, &row).is_ok() {
            c.previews_filled += 1;
        }
    }
}

/// Index one message's body, stamp its sender verdict if it lacks one, and
/// keep `newest` pointing at the latest message's opening line.
fn index_message(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    mut w: MessageWire,
    newest: &mut Option<(i64, String)>,
    c: &mut Counts,
) {
    if w.message_id.is_empty() {
        return;
    }
    // This user's own filename. The shared blob's names
    // whichever owner wrote last, and reading through it looks
    // in the wrong maildir for anyone else on the thread.
    let blob_ref = state
        .mailbox
        .user_message_facts(user, &w.message_id)
        .ok()
        .flatten()
        .map(|f| f.blob_ref)
        .unwrap_or_else(|| w.blob_ref.clone());
    let Some(raw) = crate::read_maildir_file(user, &blob_ref) else {
        return;
    };
    stamp_sender_trust(state, user, tid, &mut w, &raw, c);
    let text = crate::body_text_for_search(&mailrs_mime::parse(&raw));
    if let Some(text) = text.as_deref()
        && state
            .mailbox
            .index_message_text(&w.message_id, text)
            .is_ok()
    {
        c.bodies_indexed += 1;
    }
    // The newest message's opening line is the row's
    // preview. Tracked by date rather than by position:
    // `thread_messages_for_maintenance` promises no order,
    // and the last one read is not the last one sent.
    if let Some(text) = text.as_deref()
        && newest.as_ref().is_none_or(|(d, _)| w.internal_date >= *d)
    {
        *newest = Some((w.internal_date, mailrs_clean::preview_line(text, 120)));
    }
}

// Stamp the sender-auth verdict on rows ingested before
// the field existed, so browsing old mail shows the same
// badge new mail does. Rewrite the wire only when it
// actually gains a verdict.
fn stamp_sender_trust(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    w: &mut MessageWire,
    raw: &[u8],
    c: &mut Counts,
) {
    if !w.sender_trust.is_empty() {
        return;
    }
    let trust = crate::extract_sender_trust(raw);
    if trust.is_empty() {
        return;
    }
    w.sender_trust = trust;
    if let Ok(payload) = serde_json::to_vec(&w) {
        let _ = state.mailbox.upsert_user_message(
            user,
            tid,
            &w.message_id,
            w.internal_date,
            &payload,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &w.blob_ref,
                uid: w.uid,
                flags: w.flags,
                modseq: w.modseq,
            },
        );
        c.trust_stamped += 1;
    }
}
