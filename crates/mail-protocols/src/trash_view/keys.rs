//! A message of a view, and the helpers every view shares.

use crate::backend::{Message, ShownMessage};

/// A message of the view: the store's key, and in `Trash` where the message
/// is filed and the uid it has there.
#[derive(Clone)]
pub struct ViewKey<M, K> {
    pub(super) key: K,
    pub(super) filed: Option<(M, u32)>,
}

pub(super) fn wrap<M, K>(m: Message<K>) -> Message<ViewKey<M, K>> {
    Message {
        uid: m.uid,
        seqno: m.seqno,
        flags: m.flags,
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: ViewKey {
            key: m.key,
            filed: None,
        },
    }
}

/// The message as its store knows it, and the mailbox it is filed in when
/// that is not `mailbox`.
pub(super) fn unwrap<M: Clone, K: Clone>(m: &Message<ViewKey<M, K>>) -> (Message<K>, Option<M>) {
    let (uid, filed) = match &m.key.filed {
        Some((mb, uid)) => (*uid, Some(mb.clone())),
        None => (m.uid, None),
    };
    let msg = Message {
        uid,
        seqno: m.seqno,
        flags: m.flags.clone(),
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: m.key.key.clone(),
    };
    (msg, filed)
}

pub(super) fn renumber<K>(messages: &mut [Message<K>]) {
    for (i, m) in messages.iter_mut().enumerate() {
        m.seqno = i as u32 + 1;
    }
}

/// `Trash`'s UIDVALIDITY, from when the latest conversation went there.
/// Never 0, which RFC 3501 does not allow, and different for an empty
/// Trash than for any moment something went there.
pub(super) fn trash_uidvalidity(latest: i64) -> u32 {
    u32::try_from(latest)
        .unwrap_or(u32::MAX - 1)
        .min(u32::MAX - 1)
        + 1
}

pub(super) fn trash_entry<M: Clone, K: Clone>(t: &ShownMessage<M, K>) -> Message<ViewKey<M, K>> {
    let m = &t.message;
    Message {
        uid: t.uid,
        seqno: 0,
        flags: m.flags.clone(),
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: ViewKey {
            key: m.key.clone(),
            filed: Some((t.mailbox.clone(), m.uid)),
        },
    }
}
