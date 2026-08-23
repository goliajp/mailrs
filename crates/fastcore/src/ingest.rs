//! Taking mail from the ingest source into the store, and the header
//! parsing every path through here shares.
//!
//! Split out of `lib.rs` on 2026-08-02. `extract_headers` and
//! `resolve_thread_by_ancestry` are the two the maildir sweep also calls —
//! a second copy of either would thread the sweep's repairs differently
//! from the arrivals they repair, which is the kind of divergence that
//! only shows up as "these two messages should be one conversation".

use std::sync::Arc;

use crate::headers::{
    body_text_for_search, extract_headers, extract_sender_trust, maildir_filename_epoch,
    resolve_thread_by_ancestry,
};
use crate::{FastcoreState, enqueue_webhooks_for_arrival};

/// Write-through ingest for a file the spool drain just delivered to
/// maildir: thread aggregate + message wire + uid + side sinks, all at
/// delivery time.
///
/// Before this existed the drain wrote ONLY maildir and relied on the
/// periodic self-heal to surface the message — but self-heal handles
/// just two shapes (thread hash missing / messages zset empty), so a
/// reply landing in an EXISTING thread never became visible (G14).
/// Self-heal remains the crash-recovery backstop; this is the primary
/// path.
pub(crate) fn ingest_delivered_file(
    state: &Arc<FastcoreState>,
    addr: &str,
    blob_ref: &str,
    body: &[u8],
    target_folder: &str,
) {
    let head = &body[..body.len().min(16 * 1024)];
    let (message_id, in_reply_to, references, subject, date, from, to) = extract_headers(head);
    if message_id.is_empty() {
        // no Message-ID header — leave it to self-heal's filename-based
        // fallbacks rather than fabricating an id here
        return;
    }
    let bare = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    let date = if date > 0 {
        date
    } else {
        maildir_filename_epoch(bare).unwrap_or(0)
    };
    // v2.9.5 threading fix — prefer the thread an ancestor actually
    // landed in (msgid index) over deriving one from raw headers.
    // References[0] is NOT a stable conversation root (each hop can
    // rewrite it), which is how conversations fragmented.
    let root = match resolve_thread_by_ancestry(
        state,
        addr,
        &message_id,
        &in_reply_to,
        &references,
        &subject,
    ) {
        Some(tid) => tid,
        None => {
            if let Some(first) = references.first() {
                first.clone()
            } else if !in_reply_to.is_empty() {
                in_reply_to.clone()
            } else {
                message_id.clone()
            }
        }
    };
    let is_own = mailrs_mailbox_kevy::senders_csv_contains_user(&from, addr);
    let unread = !is_own;
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
    let category = if target_folder.eq_ignore_ascii_case("junk") {
        "spam"
    } else {
        crate::bayes_train::classify_triage(state, body).unwrap_or("inbox")
    };
    // The line under the subject in every row of the list. Passed as ""
    // until 2026-08-09, so every *received* thread showed nothing —
    // only the outbound send and an importer ever wrote one. The MIME
    // parse below is the same one the search index already pays for,
    // hoisted so both readings come out of it.
    let body_text = body_text_for_search(body);
    let preview = body_text
        .as_deref()
        .map(|t| mailrs_clean::preview_line(t, 120))
        .unwrap_or_default();
    // Side effect, never a filter — same shape as the FBL and TLS-RPT
    // hooks in the drain. No-op until MAILRS_APNS_* is configured.
    crate::push::maybe_notify(addr, &from, &subject, category, is_own);
    // Importance follows the latest INBOUND message, like the thread's
    // display fields — the user's own reply must not restate it.
    if !is_own {
        crate::importance::score_inbound(state, addr, &root, &from, head, body);
    }
    // Webhook subscriptions filtered to this sender / conversation. The
    // monolith enqueued here off its event bus; this lane had no
    // subscriber, so a user's webhook never fired at all.
    enqueue_webhooks_for_arrival(state, addr, &root, &from, &subject);
    crate::live_sync::upsert_contacts(addr, &from);
    crate::live_sync::adjust_usage_bytes(addr, body.len() as i64);
    let m = crate::imap::backend::bump_modseq(state, addr);
    crate::imap::backend::set_file_modseq(state, addr, bare, m);
    let _ = state.notify.send(addr.to_string());
    crate::live_sync::publish_new_mail(addr, &root, &from, &subject, "");
    let uid = state.mailbox.allocate_uid(addr, &message_id).unwrap_or(0);
    // The maildir's copy of the promise. Appended rather than read-checked:
    // delivery is the hot path, the list is append-only by design, and the
    // self-heal reads it back before allocating anything.
    crate::uidlist::record(addr, uid, blob_ref);
    // The invitation, if this is one. Read from the whole body rather
    // than the 16 KB header window above: a calendar part routinely
    // sits past it, and a window-only extractor finds nothing on
    // exactly the invites that matter.
    let invite = crate::invites::find(body);
    if let Some(found) = &invite {
        crate::invites::store(state, &message_id, found);
        crate::invites::file_event(state, addr, found);
    }
    let wire = mailrs_core_api::method::message::MessageWire {
        id: 0,
        mailbox_id: 0,
        uid,
        blob_ref: blob_ref.to_string(),
        sender: from,
        recipients: to,
        subject,
        date,
        internal_date: date,
        size: body.len() as u32,
        flags: if unread { 0 } else { 1 },
        message_id: message_id.clone(),
        in_reply_to,
        sender_trust: extract_sender_trust(body),
        invite_method: invite
            .as_ref()
            .map(|i| i.method.clone())
            .unwrap_or_default(),
        thread_id: root.clone(),
        modseq: 0,
        user_address: addr.to_string(),
    };
    match serde_json::to_vec(&wire) {
        Ok(payload) => {
            // The shared blob plus this user's own row: their maildir
            // filename, their uid, their flags. A thread can have several
            // owners and each has a different file on disk, so a single
            // `blob_ref` on the shared blob is one owner's — 74 messages on
            // production were served to a user the row did not name. See
            // `.claude/rfcs/20260731-per-user-message-projection.md`.
            if let Err(e) = state.mailbox.upsert_user_message(
                addr,
                &root,
                &message_id,
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

    // **The message row first, the arrival second** — the same ordering
    // `deliver_message` documents, and which this path did not have.
    //
    // Two of the membership row's declared columns, `is_sender` and
    // `sent_only`, are derived from the aggregate index over the message
    // rows of the thread. Recording the arrival before writing this
    // message's row asks that index a question about a message it cannot
    // see yet, so every message arriving through the spool — which is
    // every message — was counted without itself. A reply the user sent
    // landed in its thread, showed the right sender, and never reached
    // the Sent list, because the column the Sent axis reads still said
    // the user had not written here.
    let arrival = mailrs_mailbox_kevy::MessageArrival {
        thread_id: &root,
        user: addr,
        subject: &wire.subject,
        senders_csv: &wire.sender,
        latest_date: date,
        latest_preview: &preview,
        category,
        unread,
        is_own,
    };
    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::warn!(error = %e, %addr, %root, "drain ingest: record_message_arrival failed");
    }
    // register this message's id → thread so future replies that cite it
    // (In-Reply-To / References) resolve into the same conversation.
    let _ = state
        .mailbox
        .set_thread_for_message_id(addr, &message_id, &root);
    // Index the body for full-text search. Costs one MIME parse on a
    // path that already has the bytes in hand, and it is what makes
    // search cover message contents rather than just headers.
    if let Some(text) = body_text.as_deref()
        && let Err(e) = state.mailbox.index_message_text(&message_id, &root, text)
    {
        tracing::warn!(error = %e, %addr, %message_id, "index_message_text failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fresh_state;

    const MESSAGE: &[u8] = b"From: Alice <alice@example.com>\r\n\
To: bob@golia.jp\r\n\
Subject: Quarterly report\r\n\
Message-ID: <m1@example.com>\r\n\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Please review the attached figures before Friday. The numbers moved.\r\n";

    /// The line under the subject in every row of the conversation list.
    ///
    /// The drain — the path every received message takes — passed an
    /// empty string for it, so the most-read surface in the client had
    /// nothing to read. Only two paths ever wrote a real one: the
    /// outbound send, and an importer.
    #[test]
    fn a_received_message_leaves_a_preview_on_the_thread() {
        let state = fresh_state();
        ingest_delivered_file(&state, "bob@golia.jp", "m1.eml", MESSAGE, "INBOX");

        let row = state
            .mailbox
            .get_thread_for_user("bob@golia.jp", "m1@example.com")
            .expect("thread read")
            .expect("the thread exists");
        assert!(
            row.latest_preview
                .starts_with("Please review the attached figures"),
            "preview was {:?}",
            row.latest_preview
        );
    }

    /// An invitation carries `text/calendar`, and the row must say so.
    ///
    /// This is the assertion the defect hid behind. Extractor, parser,
    /// RSVP routes and web card were all built; the step that reads the
    /// calendar part out of an *arriving* message lived in the lane that
    /// stopped being built, so production ingested invitations and
    /// stored nothing about them. Nothing failed, because nothing asked.
    ///
    /// The fixture is the corpus's Outlook REQUEST — Exchange's shape,
    /// which is what a Teams invite arrives as.
    #[test]
    fn an_invitation_leaves_its_method_on_the_row_and_its_event_beside_it() {
        const OUTLOOK_REQUEST: &[u8] =
            include_bytes!("../../ical/tests/fixtures/itip/outlook/request.eml");
        let state = fresh_state();
        let user = "bob@golia.jp";
        ingest_delivered_file(&state, user, "invite.eml", OUTLOOK_REQUEST, "INBOX");

        let (mid, tid) = {
            let head =
                String::from_utf8_lossy(&OUTLOOK_REQUEST[..OUTLOOK_REQUEST.len().min(16_384)]);
            let mid = head
                .lines()
                .find_map(|l| l.strip_prefix("Message-ID:"))
                .expect("fixture has a Message-ID")
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            (mid.clone(), mid)
        };

        let rows = state
            .mailbox
            .list_thread_messages(user, &tid)
            .expect("read the thread");
        let wire: mailrs_core_api::method::message::MessageWire =
            serde_json::from_slice(rows.first().expect("the message is there"))
                .expect("the row is a MessageWire");
        assert_eq!(
            wire.invite_method, "REQUEST",
            "an invitation arrived and the row does not say so"
        );

        let payload = crate::invites::payload_json(&state, &mid)
            .expect("the event was stored beside the message");
        assert!(
            payload.contains("\"summary\""),
            "the stored payload is not a typed invite: {payload}"
        );
        // And the instant, resolved through the invite's own VTIMEZONE
        // at ingest. Without it a client has only a wall-clock string
        // and a zone name it cannot evaluate, and the web's answer to
        // that was to read the string as UTC — seven hours out for a
        // Pacific meeting read in Tokyo.
        assert!(
            payload.contains("\"dtstart_utc\":\""),
            "the stored payload has no resolved instant: {payload}"
        );
    }

    /// A reply the user writes into a thread somebody else started
    /// reaches the Sent axis.
    ///
    /// The production shape, and the one a fresh-thread test cannot
    /// see: the thread already exists with `is_sender` false, because
    /// the first message came from outside. If recording the arrival
    /// does not re-derive that column, the reply lands in its thread,
    /// shows the right sender, and never appears in Send — which is how
    /// this was found, by sending one and looking for it.
    #[test]
    fn a_reply_into_someone_elses_thread_reaches_the_sent_axis() {
        const INBOUND: &[u8] = b"From: Support <support@example.com>\r\n\
To: bob@golia.jp\r\n\
Subject: Your request\r\n\
Message-ID: <ticket1@example.com>\r\n\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\
\r\n\
we have reviewed it\r\n";
        const REPLY: &[u8] = b"From: Hao Li <bob@golia.jp>\r\n\
To: support@example.com\r\n\
Subject: RE: Your request\r\n\
Message-ID: <myreply@golia.jp>\r\n\
In-Reply-To: <ticket1@example.com>\r\n\
References: <ticket1@example.com>\r\n\
Date: Tue, 5 Aug 2026 10:00:00 +0900\r\n\
\r\n\
four days on, still blocked\r\n";
        let state = fresh_state();
        let user = "bob@golia.jp";
        ingest_delivered_file(&state, user, "ticket1.eml", INBOUND, "INBOX");
        ingest_delivered_file(&state, user, "myreply.eml", REPLY, "INBOX");

        let sent = state
            .mailbox
            .list_thread_ids_by_flag_via_table(user, "is_sender", 50, 0, None)
            .expect("read the sent axis");
        assert!(
            sent.contains(&"ticket1@example.com".to_string()),
            "the user replied in this thread and it is not on the Sent axis: {sent:?}"
        );
    }

    /// And ordinary mail says nothing about calendars — otherwise the
    /// assertion above passes on a field that is simply always set.
    #[test]
    fn ordinary_mail_leaves_no_invite_method() {
        let state = fresh_state();
        let user = "bob@golia.jp";
        ingest_delivered_file(&state, user, "m1.eml", MESSAGE, "INBOX");
        let rows = state
            .mailbox
            .list_thread_messages(user, "m1@example.com")
            .expect("read the thread");
        let wire: mailrs_core_api::method::message::MessageWire =
            serde_json::from_slice(rows.first().expect("the message is there"))
                .expect("the row is a MessageWire");
        assert_eq!(
            wire.invite_method, "",
            "a plain message claimed to carry an invitation"
        );
    }
}
