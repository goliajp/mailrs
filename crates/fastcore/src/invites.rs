//! Read the invitation out of a delivered message, and keep it.
//!
//! Production ingested mail for a year without ever looking at a
//! `text/calendar` part: the extractor existed, the parser existed, the
//! web card existed, and the step between them was in the lane that no
//! longer runs. This is that step.
//!
//! Two things are stored, deliberately apart:
//!
//! - **`invite_method` on the message row** — three to seven bytes,
//!   read on every list and thread fetch, and all a client needs to
//!   know an invite when it sees one.
//! - **`mailrs:invite:{message_id}`** — the typed event, a few
//!   kilobytes, fetched only when a message is opened.
//!
//! An invite is a property of the *message*, not of a recipient, so it
//! is keyed by message-id and shared. What differs per recipient — the
//! answer given — already lives elsewhere, under `rsvp:{user}:{uid}`.

use std::sync::Arc;

use crate::FastcoreState;

pub(crate) use mailrs_intake::invite::{FoundInvite, find};

/// Store the event beside the message, keeping the newest revision.
///
/// A meeting is re-sent on every change, each time with a higher
/// `SEQUENCE` and the same `UID`. Every copy is a real message and stays
/// in the mailbox; what must not happen is an older copy overwriting the
/// event a later one already corrected. So a write with a lower
/// `SEQUENCE` for the same `UID` is refused — which is also what makes
/// re-running a backfill over old mail safe in any order.
pub(crate) fn store(state: &Arc<FastcoreState>, message_id: &str, found: &FoundInvite) {
    let key = format!("mailrs:invite:{message_id}");
    let _ = state.mailbox.store_ref().atomic(|ctx| {
        if let Ok(Some(existing)) = ctx.hget(key.as_bytes(), b"sequence".as_slice()) {
            let prior: i32 = String::from_utf8_lossy(&existing)
                .trim()
                .parse()
                .unwrap_or(-1);
            if prior > found.sequence {
                return Ok(());
            }
        }
        ctx.hset(
            key.as_bytes(),
            &[
                (b"method".as_slice(), found.method.as_bytes()),
                (b"uid".as_slice(), found.uid.as_bytes()),
                (
                    b"sequence".as_slice(),
                    found.sequence.to_string().as_bytes(),
                ),
                (b"payload".as_slice(), found.payload_json.as_bytes()),
            ],
        )?;
        Ok(())
    });
}

/// The stored event for a message, as JSON, or `None`.
pub fn payload_json(state: &Arc<FastcoreState>, message_id: &str) -> Option<String> {
    let key = format!("mailrs:invite:{message_id}");
    let raw = state
        .mailbox
        .store_ref()
        .hget(key.as_bytes(), b"payload")
        .ok()??;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

/// File the invitation as an event; see `mailrs_intake::invite::file_event`.
pub(crate) fn file_event(state: &Arc<FastcoreState>, user: &str, found: &FoundInvite) {
    mailrs_intake::invite::file_event(state.net_conn(), user, found);
}
