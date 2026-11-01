//! Reading one delivered message back off the disk.
//!
//! Split from the sweep by theme: the sweep decides, this fetches.
//! Both halves have gone wrong on their own — the sweep judged
//! against a threshold one lower than the corpus was measured at,
//! and this side missed a folded `Message-ID` for a day — and they
//! are easier to keep honest apart.
//!
//! Everything here answers the same question the receive path
//! answers about a message arriving now, through the same readers in
//! `mailrs_inbound`. A second reader of one header is how the folded
//! case came to work on one path and not the other.

use super::super::prelude::*;

/// The raw bytes of **one** message, by its id.
///
/// [`newest_raw`] answers "what does this conversation look like",
/// which is the right question when deciding whether to hide it. The
/// rotation backfill asks a different one — *how many domains send
/// to this reply address* — and a conversation's newest message
/// cannot answer it: the fortune-telling campaign put 179 messages
/// across six domains into a single mailbox, and looking only at the
/// newest of each conversation would have seen a fraction of them.
pub(in crate::maintenance) fn raw_for_message(
    state: &Arc<FastcoreState>,
    user: &str,
    mid: &str,
) -> Option<Vec<u8>> {
    let bytes = state.mailbox.user_message_view(user, mid).ok()??;
    let wire =
        serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes).ok()?;
    if wire.blob_ref.is_empty() {
        return None;
    }
    read_maildir_file(user, &wire.blob_ref)
}

/// The raw bytes of a thread's newest message, from this user's copy.
pub(in crate::maintenance) fn newest_raw(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
) -> Option<(String, Vec<u8>)> {
    let mut newest: Option<(i64, String, String)> = None;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
    {
        let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
            continue;
        };
        let Ok(wire) =
            serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
        else {
            continue;
        };
        if wire.blob_ref.is_empty() {
            continue;
        }
        let better = match &newest {
            None => true,
            Some((d, _, _)) => wire.date >= *d,
        };
        if better {
            newest = Some((wire.date, wire.blob_ref, mid.clone()));
        }
    }
    // The row's own id, not one re-derived from the file. Three held
    // conversations on production carry a synthetic
    // `…@mailrs.local` id while the file underneath has a real
    // `Message-ID`, so a verdict keyed off the file was stored where
    // no reader looks: the screen asks with the id the message row
    // carries, and got null for a conversation it was showing as
    // held. Two ids for one message, the verdict under the one nobody
    // reads.
    let (_, blob_ref, message_id) = newest?;
    let raw = read_maildir_file(user, &blob_ref)?;
    Some((message_id, raw))
}

/// Whether `user` marked this message's sender not junk.
///
/// Without a store there is no list, so nothing is on it. A list that
/// exists but cannot be read counts as naming the sender, the same
/// direction as [`reply_rotation`]: be wrong towards the answer that
/// does not hide mail.
pub(super) fn sender_whitelisted(
    conn: &mut Option<kevy_client::Connection>,
    user: &str,
    raw: &[u8],
) -> bool {
    let Some(conn) = conn.as_mut() else {
        return false;
    };
    match mailrs_core_sidestate::families::sender_lists::whitelist(conn, user) {
        // the envelope sender is not kept with the message
        Ok(list) => {
            mailrs_inbound::sender_listed(&list, "", &mailrs_inbound::identity::from_address(raw))
        }
        Err(_) => true,
    }
}
