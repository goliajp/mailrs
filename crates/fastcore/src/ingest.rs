//! Taking mail from the ingest source into the store.
//!
//! What a message says about itself (its id, date, thread, preview, whether
//! the user wrote it, its category) and what every arrival tells the rest
//! of the system come from `mailrs_intake`, shared with the SQL core; the
//! writes to this store are here.

use std::sync::Arc;

use mailrs_intake::arrival::{Arrival, derive};
use mailrs_intake::effects;

use crate::FastcoreState;
use crate::headers::Threads;

mod steps;
use steps::*;

/// Write-through ingest for a file the spool drain just delivered to
/// maildir: thread aggregate + message wire + uid + side sinks, all at
/// delivery time.
///
/// Before this existed the drain wrote ONLY maildir and relied on the
/// periodic self-heal to surface the message — but self-heal handles
/// just two shapes (thread hash missing / messages zset empty), so a
/// reply landing in an EXISTING thread never became visible.
/// Self-heal remains the crash-recovery backstop; this is the primary
/// path.
pub(crate) fn ingest_delivered_file(
    state: &Arc<FastcoreState>,
    addr: &str,
    blob_ref: &str,
    body: &[u8],
    target_folder: &str,
    // The receiver's four-layer verdict, as JSON, when it found
    // anything. `None` on every other path — an IMAP APPEND and a
    // bounce were never examined, and an absent verdict says so.
    fraud_verdict: Option<&str>,
) {
    let a = derive(
        &Threads(state),
        state.net_conn(),
        addr,
        blob_ref,
        body,
        target_folder,
    );
    // Importance follows the latest INBOUND message, like the thread's
    // display fields — the user's own reply must not restate it.
    if !a.is_own {
        crate::importance::score_inbound(state, addr, &a.thread_id, &a.from, a.head, body);
    }
    effects::before_store(state.net_conn(), addr, &a, body.len());
    let held = store_arrival(state, addr, blob_ref, body, &a, fraud_verdict);
    effects::after_store(addr, &a, held);
}

/// The writes to this store. Returns whether the thread was held as fraud.
fn store_arrival(
    state: &Arc<FastcoreState>,
    addr: &str,
    blob_ref: &str,
    body: &[u8],
    a: &Arrival<'_>,
    fraud_verdict: Option<&str>,
) -> bool {
    let bare = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    let unread = !a.is_own;
    let uid = assign_uid(state, addr, bare, &a.message_id, blob_ref);
    // The invitation, if this is one. Read from the whole body rather
    // than the 16 KB header window: a calendar part routinely sits past it.
    let invite = crate::invites::find(body);
    if let Some(found) = &invite {
        crate::invites::store(state, &a.message_id, found);
        crate::invites::file_event(state, addr, found);
    }
    let wire = mailrs_core_api::method::message::MessageWire {
        id: 0,
        mailbox_id: 0,
        uid,
        blob_ref: blob_ref.to_string(),
        sender: a.from.clone(),
        recipients: a.to.clone(),
        subject: a.subject.clone(),
        date: a.date,
        internal_date: a.date,
        size: body.len() as u32,
        flags: if unread { 0 } else { 1 },
        message_id: a.message_id.clone(),
        in_reply_to: a.in_reply_to.clone(),
        sender_trust: a.sender_trust.clone(),
        invite_method: invite
            .as_ref()
            .map(|i| i.method.clone())
            .unwrap_or_default(),
        thread_id: a.thread_id.clone(),
        modseq: 0,
        user_address: addr.to_string(),
    };
    let root = &a.thread_id;
    write_message_row(state, addr, root, &a.message_id, a.date, blob_ref, &wire);

    // **The message row first, the arrival second.** Two of the membership
    // row's declared columns, `is_sender` and `sent_only`, are derived from
    // the aggregate index over the message rows of the thread; recording
    // the arrival before writing this message's row would count every
    // message without itself, and a reply the user sent would never reach
    // the Sent list.
    let arrival = mailrs_mailbox_kevy::MessageArrival {
        thread_id: root,
        user: addr,
        subject: &wire.subject,
        senders_csv: &wire.sender,
        latest_date: a.date,
        latest_preview: &a.preview,
        category: a.category,
        unread,
        is_own: a.is_own,
    };
    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::warn!(error = %e, %addr, %root, "drain ingest: record_message_arrival failed");
    }
    let held = hold_if_fraud(state, addr, root, &a.message_id, fraud_verdict);
    // register this message's id → thread so future replies that cite it
    // (In-Reply-To / References) resolve into the same conversation.
    let _ = state
        .mailbox
        .set_thread_for_message_id(addr, &a.message_id, root);
    index_contents(
        state,
        addr,
        &a.message_id,
        a.body_text.as_deref(),
        &a.parsed,
    );
    held
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
        ingest_delivered_file(&state, "bob@golia.jp", "m1.eml", MESSAGE, "INBOX", None);

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
        ingest_delivered_file(&state, user, "invite.eml", OUTLOOK_REQUEST, "INBOX", None);

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
        ingest_delivered_file(&state, user, "ticket1.eml", INBOUND, "INBOX", None);
        ingest_delivered_file(&state, user, "myreply.eml", REPLY, "INBOX", None);

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
        ingest_delivered_file(&state, user, "m1.eml", MESSAGE, "INBOX", None);
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

    // ── the fraud verdict, and the hold it implies ───────────────

    fn held_verdict() -> String {
        serde_json::json!({
            "rules_version": "2026-08-29.1",
            "score": 9.5,
            "quarantined": true,
            "layers": [],
        })
        .to_string()
    }

    /// The verdict the receiver formed is on the message afterwards.
    ///
    /// Without this the whole feature is a reader guarded on a field
    /// nobody writes: the screen renders on the verdict, the column
    /// hides the row, and both are satisfied by an absence.
    #[test]
    fn a_verdict_from_the_receiver_lands_on_the_message() {
        let state = fresh_state();
        let user = "bob@golia.jp";
        let json = held_verdict();
        ingest_delivered_file(&state, user, "m1.eml", MESSAGE, "INBOX", Some(&json));

        assert_eq!(
            state
                .mailbox
                .fraud_verdict("m1@example.com")
                .expect("read it back")
                .as_deref(),
            Some(json.as_str())
        );
    }

    /// A held verdict takes the conversation out of the list, and out
    /// of the count with it.
    #[test]
    fn a_held_verdict_takes_the_conversation_out_of_the_list() {
        let state = fresh_state();
        let user = "bob@golia.jp";
        ingest_delivered_file(
            &state,
            user,
            "m1.eml",
            MESSAGE,
            "INBOX",
            Some(&held_verdict()),
        );

        let (rows, total) = state
            .mailbox
            .list_threads_by_activity(
                user,
                &mailrs_mailbox_kevy::ListThreadsFilter::default(),
                0,
                50,
            )
            .expect("list");
        assert!(rows.is_empty(), "a held conversation was still listed");
        assert_eq!(total, 0, "the count still included a held conversation");
    }

    /// The negative case, so the two above cannot pass on a field that
    /// is always set or a list that is always empty: ordinary mail
    /// carries no verdict and stays where it was.
    #[test]
    fn ordinary_mail_carries_no_verdict_and_stays_listed() {
        let state = fresh_state();
        let user = "bob@golia.jp";
        ingest_delivered_file(&state, user, "m1.eml", MESSAGE, "INBOX", None);

        assert_eq!(state.mailbox.fraud_verdict("m1@example.com").unwrap(), None);
        let (rows, total) = state
            .mailbox
            .list_threads_by_activity(
                user,
                &mailrs_mailbox_kevy::ListThreadsFilter::default(),
                0,
                50,
            )
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 1);
    }

    /// A verdict that does not hold is recorded and changes nothing
    /// else. Examining and hiding are two decisions, and an ingest
    /// that hid on any verdict at all would make `quarantined` a
    /// field nobody reads.
    #[test]
    fn a_recorded_but_unheld_verdict_leaves_the_list_alone() {
        let state = fresh_state();
        let user = "bob@golia.jp";
        let json = serde_json::json!({
            "rules_version": "2026-08-29.1",
            "score": 4.5,
            "quarantined": false,
            "layers": [],
        })
        .to_string();
        ingest_delivered_file(&state, user, "m1.eml", MESSAGE, "INBOX", Some(&json));

        assert!(
            state
                .mailbox
                .fraud_verdict("m1@example.com")
                .unwrap()
                .is_some()
        );
        let (rows, _) = state
            .mailbox
            .list_threads_by_activity(
                user,
                &mailrs_mailbox_kevy::ListThreadsFilter::default(),
                0,
                50,
            )
            .expect("list");
        assert_eq!(
            rows.len(),
            1,
            "a verdict that does not hold hid a conversation"
        );
    }
}
