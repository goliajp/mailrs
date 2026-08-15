//! Per-message storage — write a JSON-serialized payload per message
//! + add to the per-thread index zset (score = internal_date).
//!
//! Phase 7.11. The kevy layout:
//!   mailrs:msg:<message_id>           string  — serde-json of any
//!                                                  type the caller
//!                                                  passes in
//!   mailrs:thread:<tid>:messages      zset    — message_id → internal_date
//!
//! The caller picks the JSON shape. `mailrs-fastcore` writes the same
//! `mailrs_core_api::method::message::MessageWire` rows the monolith
//! returns, so webapi's consumer code is unchanged.

use super::KevyMailboxStore;
use super::keys;

mod group;
mod projection;
mod read;
mod uid;

pub(crate) use group::group_key;

/// What is true of one user's copy of a message, borrowed for the write.
#[derive(Debug, Clone, Copy)]
pub struct UserMessageFacts<'a> {
    /// The maildir filename **in this user's mailbox**.
    pub blob_ref: &'a str,
    /// This user's IMAP UID for it.
    pub uid: u32,
    /// This user's flags.
    pub flags: u32,
    /// This user's mod-sequence.
    pub modseq: u64,
}

/// The same, owned, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedUserMessageFacts {
    /// The maildir filename in this user's mailbox.
    pub blob_ref: String,
    /// This user's IMAP UID.
    pub uid: u32,
    /// This user's flags.
    pub flags: u32,
    /// This user's mod-sequence.
    pub modseq: u64,
    /// The thread this copy belongs to.
    ///
    /// The row carried no thread until the declared counters needed one:
    /// the association lived only in `mailrs:usermsgs:{user}:{tid}`, so a
    /// row could not say what it was part of and an index grouping by
    /// thread had nothing to group on. Empty on rows written before that.
    pub tid: String,
    /// Whether this user sent the message.
    ///
    /// Derived once, on write, from the payload's sender — see
    /// `upsert_user_message`. It decides which of the three counters the
    /// row feeds, and a message you sent is never unread for you.
    pub own: bool,
}

/// Blank the per-user fields on a serialized `MessageWire` before it is
/// stored where several users read it.
///
/// Zeroed rather than removed: `MessageWire` does not default them, so a
/// reader on an older binary would fail to deserialize a row without them
/// and the message would vanish. A zero is the honest value for a field
/// whose true value depends on who is asking.
fn strip_per_user_fields(payload: &[u8]) -> Vec<u8> {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return payload.to_vec();
    };
    let Some(obj) = v.as_object_mut() else {
        return payload.to_vec();
    };
    obj.insert("blob_ref".into(), "".into());
    obj.insert("uid".into(), 0.into());
    obj.insert("flags".into(), 0.into());
    obj.insert("modseq".into(), 0.into());
    obj.insert("mailbox_id".into(), 0.into());
    obj.insert("user_address".into(), "".into());
    serde_json::to_vec(&v).unwrap_or_else(|_| payload.to_vec())
}

/// Replace the per-user fields on a serialized `MessageWire` with this
/// user's own.
///
/// Field-level rather than typed, so this crate does not have to depend on
/// the wire type: the shared blob is opaque here by design (`upsert_message`
/// takes bytes). A blob that is not an object is returned unchanged — it is
/// not this function's place to decide the payload is malformed.
fn overlay_user_facts(shared: &[u8], facts: &OwnedUserMessageFacts) -> Vec<u8> {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(shared) else {
        return shared.to_vec();
    };
    let Some(obj) = v.as_object_mut() else {
        return shared.to_vec();
    };
    obj.insert("blob_ref".into(), facts.blob_ref.clone().into());
    obj.insert("uid".into(), facts.uid.into());
    obj.insert("flags".into(), facts.flags.into());
    obj.insert("modseq".into(), facts.modseq.into());
    // The blob names one user; served to another it was simply wrong.
    serde_json::to_vec(&v).unwrap_or_else(|_| shared.to_vec())
}
