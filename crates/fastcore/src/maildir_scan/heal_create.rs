//! Building a thread aggregate from scratch out of the files on disk, for
//! mail that arrived with no row at all.

use mailrs_core_api::method::message::FLAG_SEEN;
use std::sync::Arc;

use super::scan::MailFile;
use crate::FastcoreState;

/// What the sweep loads once per mailbox and reads for every message.
pub(super) struct MailboxState {
    pub(super) uids: Option<mailrs_uidlist::UidList>,
    pub(super) kw: mailrs_keywords::Keywords,
    pub(super) log: mailrs_threadstate::ThreadState,
}

/// Create the thread `root` and its messages from `bucket`.
pub(super) fn create_thread(
    state: &Arc<FastcoreState>,
    user: &str,
    root: &str,
    bucket: &[&MailFile],
    mailbox: &MailboxState,
) {
    // Create a minimal thread aggregate from scratch — inbound
    // OR outbound. Skipping non-sender threads here was the
    // reason fresh Gmail arrivals (files present in maildir but
    // no matching kevy hash) never showed up in the inbox: the
    // "heal missing messages" branch above only touches threads
    // already in the by_activity zset, so a genuinely new
    // arrival had no path in. Create here for every bucket.
    let mut ordered: Vec<&MailFile> = bucket.to_vec();
    ordered.sort_by_key(|m| m.date);
    for m in &ordered {
        record_message(state, user, root, m, mailbox.uids.as_ref());
    }
    // The half that makes the keyword bits worth writing: a
    // decision the maildir carries is put back on the row a
    // rebuilt index serves from. Any message in the thread
    // carrying the bit archives (or pins) the thread, because the
    // bit is per message and the decision was made per thread.
    let kw = &mailbox.kw;
    let archived = ordered
        .iter()
        .any(|m| crate::keywords::file_has(kw, &m.keywords, crate::keywords::ARCHIVED));
    let pinned = ordered
        .iter()
        .any(|m| crate::keywords::file_has(kw, &m.keywords, crate::keywords::PINNED));
    if archived {
        let _ = state.mailbox.set_archived(user, root, true);
    }
    if pinned {
        let _ = state.mailbox.set_pinned(user, root, true);
    }
    // The reading half of the thread-state log.
    crate::threadstate::apply_to_row(state, &mailbox.log, user, root);
}

/// The arrival, the contact, the message row and its message-id link for
/// one file.
fn record_message(
    state: &Arc<FastcoreState>,
    user: &str,
    root: &str,
    m: &MailFile,
    uids: Option<&mailrs_uidlist::UidList>,
) {
    let category = super::scan::arrival_category(state, user, m);
    let is_own = mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user);
    let unread = !m.seen && !is_own;
    let arrival = mailrs_mailbox_kevy::MessageArrival {
        thread_id: root,
        user,
        subject: &m.subject,
        senders_csv: &m.from,
        latest_date: m.date,
        latest_preview: "",
        category,
        unread,
        is_own,
    };
    let _ = state.mailbox.record_message_arrival(&arrival);
    // Side sink: contacts autocomplete.
    crate::live_sync::upsert_contacts(user, &m.from);
    // Also write the message blob for enrich_with_body.
    //
    // The UID comes from the maildir's own list when it has
    // one: this sweep *is* the rebuild, and a rebuild that
    // invents fresh UIDs is what makes every IMAP client
    // resync.
    let uid = crate::uidlist::uid_for(state, uids, user, &m.message_id, &m.filename);
    let wire = mailrs_core_api::method::message::MessageWire {
        id: 0,
        mailbox_id: 0,
        uid,
        blob_ref: m.filename.clone(),
        sender: m.from.clone(),
        recipients: m.to.clone(),
        subject: m.subject.clone(),
        date: m.date,
        internal_date: m.date,
        size: m.size,
        // From the file name, not assumed. Maildir states the
        // read state in the `:2,FLAGS` suffix, and the scan has
        // already parsed it into `m.seen` — this said `1`, marking
        // every message it healed as read whatever the file said,
        // which is the same fact-versus-guess mistake the read-state
        // work exists to remove.
        flags: if m.seen { FLAG_SEEN } else { 0 },
        message_id: m.message_id.clone(),
        in_reply_to: m.in_reply_to.clone(),
        sender_trust: m.sender_trust.clone(),
        // As in `heal_messages`: a row rebuilt from disk
        // comes back with what the delivered one had.
        invite_method: m
            .invite
            .as_ref()
            .map(|i| i.method.clone())
            .unwrap_or_default(),
        thread_id: root.to_string(),
        modseq: 0,
        user_address: user.to_string(),
    };
    if let Ok(payload) = serde_json::to_vec(&wire) {
        let _ = state.mailbox.upsert_user_message(
            user,
            root,
            &m.message_id,
            m.date,
            &payload,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &wire.blob_ref,
                uid: wire.uid,
                flags: wire.flags,
                modseq: wire.modseq,
            },
        );
    }
    let _ = state
        .mailbox
        .set_thread_for_message_id(user, &m.message_id, root);
}
