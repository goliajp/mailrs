//! Every message a user has, as the few facts that decide where it is and
//! how it is marked: uid, thread, flags, whether the user wrote it, and
//! per thread whether it is archived, in the Trash, junk, and its labels.
//!
//! A JMAP server works out what changed between two moments by comparing
//! two of these, so the answer does not depend on every writer
//! remembering to record that it wrote. The body is a compact binary
//! encoding in one canonical order, so both cores answer the same bytes
//! for the same mail and an unchanged index is recognised by its ETag
//! without sending it again.

use crate::types::ConversationSummaryWire;

pub const PATH_MESSAGE_INDEX: &str = "/v1/users/{user}/messages/index";

/// The media type of the encoded index.
pub const CONTENT_TYPE: &str = "application/x-mailrs-message-index";

const MAGIC: &[u8; 4] = b"MIX1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexThread {
    pub thread_id: String,
    pub archived: bool,
    pub trashed: bool,
    pub junk: bool,
    /// Label ids, sorted.
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexMessage {
    pub uid: u32,
    /// Position of the message's thread in [`MessageIndex::threads`].
    pub thread: u32,
    pub flags: u32,
    /// The user wrote it.
    pub own: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageIndex {
    /// Sorted by thread id.
    pub threads: Vec<IndexThread>,
    /// Sorted by uid.
    pub messages: Vec<IndexMessage>,
}

/// One message as a core reads it, before the index is put in order.
pub struct Entry<'a> {
    pub uid: u32,
    pub thread_id: &'a str,
    pub flags: u32,
    pub own: bool,
}

impl MessageIndex {
    /// The index of `conversations` and the messages in them, in canonical
    /// order. A message whose thread has no conversation row, a
    /// conversation with no message, and one held for review are left out:
    /// none is something a mailbox shows.
    pub fn build(conversations: &[ConversationSummaryWire], messages: &[Entry<'_>]) -> Self {
        let mut convs: Vec<&ConversationSummaryWire> = conversations
            .iter()
            .filter(|c| !c.quarantined)
            .filter(|c| messages.iter().any(|m| m.thread_id == c.thread_id.as_str()))
            .collect();
        convs.sort_by(|a, b| a.thread_id.as_str().cmp(b.thread_id.as_str()));
        convs.dedup_by(|a, b| a.thread_id == b.thread_id);
        let threads: Vec<IndexThread> = convs
            .iter()
            .map(|c| {
                let mut labels = c.labels.clone();
                labels.sort();
                labels.dedup();
                IndexThread {
                    thread_id: c.thread_id.to_string(),
                    archived: c.archived,
                    trashed: c.trashed_at > 0,
                    junk: mailrs_mailbox::scope::bucket_of(&c.category)
                        == mailrs_mailbox::scope::Bucket::Junk,
                    labels,
                }
            })
            .collect();
        let mut out: Vec<IndexMessage> = messages
            .iter()
            .filter(|m| m.uid > 0)
            .filter_map(|m| {
                let pos = threads
                    .binary_search_by(|t| t.thread_id.as_str().cmp(m.thread_id))
                    .ok()?;
                Some(IndexMessage {
                    uid: m.uid,
                    thread: pos as u32,
                    flags: m.flags,
                    own: m.own,
                })
            })
            .collect();
        out.sort_by_key(|m| m.uid);
        out.dedup_by_key(|m| m.uid);
        Self {
            threads,
            messages: out,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(16 + self.threads.len() * 48 + self.messages.len() * 13);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&(self.threads.len() as u32).to_le_bytes());
        for t in &self.threads {
            put_str(&mut b, &t.thread_id);
            let bits = u8::from(t.archived) | (u8::from(t.trashed) << 1) | (u8::from(t.junk) << 2);
            b.push(bits);
            b.extend_from_slice(&(t.labels.len() as u16).to_le_bytes());
            for l in &t.labels {
                put_str(&mut b, l);
            }
        }
        b.extend_from_slice(&(self.messages.len() as u32).to_le_bytes());
        for m in &self.messages {
            b.extend_from_slice(&m.uid.to_le_bytes());
            b.extend_from_slice(&m.thread.to_le_bytes());
            b.extend_from_slice(&m.flags.to_le_bytes());
            b.push(u8::from(m.own));
        }
        b
    }

    /// `None` for anything that is not an encoded index.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC {
            return None;
        }
        let n = r.u32()? as usize;
        let mut threads = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            let thread_id = r.str()?;
            let bits = r.take(1)?[0];
            let nl = r.u16()? as usize;
            let mut labels = Vec::with_capacity(nl);
            for _ in 0..nl {
                labels.push(r.str()?);
            }
            threads.push(IndexThread {
                thread_id,
                archived: bits & 1 != 0,
                trashed: bits & 2 != 0,
                junk: bits & 4 != 0,
                labels,
            });
        }
        let n = r.u32()? as usize;
        let mut messages = Vec::with_capacity(n.min(1 << 22));
        for _ in 0..n {
            let uid = r.u32()?;
            let thread = r.u32()?;
            let flags = r.u32()?;
            let own = r.take(1)?[0] != 0;
            if thread as usize >= threads.len() {
                return None;
            }
            messages.push(IndexMessage {
                uid,
                thread,
                flags,
                own,
            });
        }
        r.0.is_empty().then_some(Self { threads, messages })
    }
}

/// The ETag of an encoded index: FNV-1a over its bytes, as hex.
pub fn etag(encoded: &[u8]) -> String {
    format!("{:016x}", fnv64(encoded))
}

/// 64-bit FNV-1a. Stable across builds and platforms, unlike the
/// standard library's hasher, which is what lets two processes agree.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The answer a core gives for an encoded index: 304 when the caller's
/// `If-None-Match` names it, the bytes with their ETag otherwise.
#[cfg(feature = "server")]
pub fn respond(body: &[u8], headers: &axum::http::HeaderMap) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    let tag = format!("\"{}\"", etag(body));
    let known = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag));
    if known {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
    }
    (
        [
            (header::CONTENT_TYPE, CONTENT_TYPE.to_string()),
            (header::ETAG, tag),
        ],
        body.to_vec(),
    )
        .into_response()
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    let bytes = &s.as_bytes()[..s.len().min(u16::MAX as usize)];
    b.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    b.extend_from_slice(bytes);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(tid: &str, labels: &[&str]) -> ConversationSummaryWire {
        let mut c: ConversationSummaryWire = serde_json::from_value(serde_json::json!({
            "thread_id": tid, "subject": "", "participants": "", "message_count": 1,
            "unread_count": 0, "last_date": 0, "category": "spam", "flagged": false,
            "snippet": "", "pinned": false, "archived": false, "importance_level": "",
            "importance_score": 0.0, "requires_action": false, "sent_count": 0,
        }))
        .unwrap();
        c.labels = labels.iter().map(|l| l.to_string()).collect();
        c
    }

    #[test]
    fn the_same_mail_in_any_order_encodes_the_same_and_round_trips() {
        let convs = [
            conv("t-b", &["2", "1"]),
            conv("t-a", &[]),
            conv("t-empty", &[]),
        ];
        let msgs = [
            Entry {
                uid: 9,
                thread_id: "t-b",
                flags: 1,
                own: false,
            },
            Entry {
                uid: 3,
                thread_id: "t-a",
                flags: 0,
                own: true,
            },
            Entry {
                uid: 4,
                thread_id: "t-orphan",
                flags: 0,
                own: false,
            },
        ];
        let a = MessageIndex::build(&convs, &msgs);
        let mut rev = convs.clone();
        rev.reverse();
        let b = MessageIndex::build(
            &rev,
            &[
                msgs[2].clone_entry(),
                msgs[1].clone_entry(),
                msgs[0].clone_entry(),
            ],
        );
        assert_eq!(a.encode(), b.encode());
        assert_eq!(
            a.threads
                .iter()
                .map(|t| t.thread_id.as_str())
                .collect::<Vec<_>>(),
            ["t-a", "t-b"]
        );
        assert_eq!(a.threads[1].labels, ["1", "2"]);
        assert!(a.threads[1].junk);
        assert_eq!(a.messages.iter().map(|m| m.uid).collect::<Vec<_>>(), [3, 9]);
        assert_eq!(MessageIndex::decode(&a.encode()), Some(a.clone()));
        let mut bad = a.encode();
        bad.push(0);
        assert_eq!(MessageIndex::decode(&bad), None);
        assert_eq!(MessageIndex::decode(b"nope"), None);
        assert_ne!(etag(&a.encode()), etag(&MessageIndex::default().encode()));
    }

    impl Entry<'_> {
        fn clone_entry(&self) -> Self {
            Entry {
                uid: self.uid,
                thread_id: self.thread_id,
                flags: self.flags,
                own: self.own,
            }
        }
    }
}
