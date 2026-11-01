//! The caller's mail as JMAP sees it: every message with its thread,
//! keywords and mailboxes, read from the core's message index.
//!
//! Each method of a request reads the same view, so an `Email/get` and the
//! `state` it reports agree. The last view per user is kept, and the core
//! is asked only whether it is still current (ETag), so an unchanged
//! mailbox costs one empty answer rather than the whole index.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::method::message_index::{IndexMessage, MessageIndex};
use mailrs_jmap::error::JmapMethodError;
use mailrs_mailbox::types::FLAG_DRAFT;

use super::ctx::{Ctx, fail};
use super::ids;
use super::mailbox::{Place, Role};

pub struct View {
    tag: String,
    /// When the core last said this view is current.
    checked: Mutex<std::time::Instant>,
    pub index: MessageIndex,
    by_uid: HashMap<u32, usize>,
    /// Per thread: its key, and whether the user wrote every message in it.
    keys: Vec<u64>,
    sent_only: Vec<bool>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub total_emails: u64,
    pub unread_emails: u64,
    pub total_threads: u64,
    pub unread_threads: u64,
}

impl View {
    fn new(tag: String, index: MessageIndex) -> Self {
        let by_uid = index
            .messages
            .iter()
            .enumerate()
            .map(|(n, m)| (m.uid, n))
            .collect();
        let keys = index
            .threads
            .iter()
            .map(|t| ids::thread_key(&t.thread_id))
            .collect();
        let mut sent_only = vec![true; index.threads.len()];
        for m in &index.messages {
            sent_only[m.thread as usize] &= m.own;
        }
        Self {
            tag,
            checked: Mutex::new(std::time::Instant::now()),
            index,
            by_uid,
            keys,
            sent_only,
        }
    }

    #[cfg(test)]
    pub fn for_test(index: MessageIndex) -> Self {
        Self::new(String::new(), index)
    }

    pub fn get(&self, uid: u32) -> Option<&IndexMessage> {
        self.by_uid.get(&uid).map(|n| &self.index.messages[*n])
    }

    pub fn thread_key(&self, m: &IndexMessage) -> u64 {
        self.keys[m.thread as usize]
    }

    pub fn thread_id(&self, m: &IndexMessage) -> &str {
        &self.index.threads[m.thread as usize].thread_id
    }

    /// The messages of the thread with this key, oldest uid first.
    pub fn thread_members(&self, key: u64) -> Vec<u32> {
        self.index
            .messages
            .iter()
            .filter(|m| self.thread_key(m) == key)
            .map(|m| m.uid)
            .collect()
    }

    /// The mailboxes a message is in. Where a conversation is (inbox,
    /// archive, junk, trash) and its labels hold for every message in it;
    /// a message the user wrote is in Sent too, and a conversation the
    /// user only wrote into is in Sent alone. A message the user wrote and
    /// marked `$draft` is in Drafts instead, until it is sent.
    pub fn places(&self, m: &IndexMessage) -> Vec<Place> {
        let t = &self.index.threads[m.thread as usize];
        if m.own && m.flags & FLAG_DRAFT != 0 && !t.trashed {
            let mut out = vec![Place::Role(Role::Drafts)];
            out.extend(t.labels.iter().map(|l| Place::Label(l.clone())));
            return out;
        }
        let place = match (t.trashed, t.junk, t.archived) {
            (true, _, _) => Role::Trash,
            (false, true, _) => Role::Junk,
            (false, false, true) => Role::Archive,
            (false, false, false) => Role::Inbox,
        };
        let mut out = Vec::new();
        if !(m.own && place == Role::Inbox && self.sent_only[m.thread as usize]) {
            out.push(Place::Role(place));
        }
        if m.own {
            out.push(Place::Role(Role::Sent));
        }
        // a label is not a place: the Trash takes a conversation out of it
        if !t.trashed {
            out.extend(t.labels.iter().map(|l| Place::Label(l.clone())));
        }
        out
    }

    pub fn counts(&self) -> HashMap<String, Counts> {
        let mut out: HashMap<String, Counts> = HashMap::new();
        let mut threads: HashSet<(String, u64)> = HashSet::new();
        let mut unread_threads: HashSet<(String, u64)> = HashSet::new();
        for m in &self.index.messages {
            let unread = m.flags & FLAG_SEEN == 0;
            let key = self.thread_key(m);
            for p in self.places(m) {
                let id = p.id();
                let c = out.entry(id.clone()).or_default();
                c.total_emails += 1;
                c.unread_emails += u64::from(unread);
                if threads.insert((id.clone(), key)) {
                    c.total_threads += 1;
                }
                if unread && unread_threads.insert((id, key)) {
                    c.unread_threads += 1;
                }
            }
        }
        out
    }
}

fn cache() -> &'static Mutex<HashMap<String, Arc<View>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<View>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The caller's view, as the core had it at most `age` ago: every push
/// connection of an account shares one question to the core per `age`.
pub async fn recent(ctx: &Ctx, age: std::time::Duration) -> Result<Arc<View>, JmapMethodError> {
    let held = cache()
        .lock()
        .map_err(|_| fail("view cache poisoned"))?
        .get(&ctx.user)
        .cloned();
    match held {
        Some(v)
            if v.checked
                .lock()
                .map_err(|_| fail("view cache poisoned"))?
                .elapsed()
                < age =>
        {
            Ok(v)
        }
        _ => current(ctx).await,
    }
}

/// The caller's view as it is now.
pub async fn current(ctx: &Ctx) -> Result<Arc<View>, JmapMethodError> {
    let held = cache()
        .lock()
        .map_err(|_| fail("view cache poisoned"))?
        .get(&ctx.user)
        .cloned();
    let known = held.as_ref().map(|v| v.tag.as_str());
    let fresh = ctx
        .state
        .core
        .message_index(&ctx.user, known)
        .await
        .map_err(fail)?;
    let view = match (fresh, held) {
        (Some((tag, index)), _) => Arc::new(View::new(tag, index)),
        (None, Some(held)) => {
            *held
                .checked
                .lock()
                .map_err(|_| fail("view cache poisoned"))? = std::time::Instant::now();
            return Ok(held);
        }
        (None, None) => return Err(fail("message index: not modified without a tag")),
    };
    cache()
        .lock()
        .map_err(|_| fail("view cache poisoned"))?
        .insert(ctx.user.clone(), view.clone());
    Ok(view)
}

#[cfg(test)]
mod tests {
    use mailrs_core_api::method::message_index::{IndexThread, MessageIndex};

    use super::*;

    fn thread(id: &str, archived: bool, trashed: bool, labels: &[&str]) -> IndexThread {
        IndexThread {
            thread_id: id.into(),
            archived,
            trashed,
            junk: false,
            labels: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    fn msg(uid: u32, thread: u32, flags: u32, own: bool) -> IndexMessage {
        IndexMessage {
            uid,
            thread,
            flags,
            own,
        }
    }

    #[test]
    fn places_follow_the_conversation_and_who_wrote_it() {
        let index = MessageIndex {
            threads: vec![
                thread("a", false, false, &["7"]),
                thread("b", false, false, &[]),
                thread("c", true, true, &["7"]),
            ],
            messages: vec![
                msg(1, 0, 0, false),
                msg(2, 0, FLAG_SEEN, true),
                msg(3, 1, FLAG_SEEN, true),
                msg(4, 2, 0, false),
                msg(5, 1, FLAG_DRAFT, true),
            ],
        };
        let v = View::new("t".into(), index);
        let ids = |uid| -> Vec<String> {
            v.places(v.get(uid).unwrap())
                .iter()
                .map(Place::id)
                .collect()
        };
        assert_eq!(ids(1), ["inbox", "L7"]);
        assert_eq!(ids(2), ["inbox", "sent", "L7"]);
        // only the user wrote into it: Sent alone
        assert_eq!(ids(3), ["sent"]);
        assert_eq!(ids(4), ["trash"]);
        assert_eq!(ids(5), ["drafts"]);
        let c = v.counts();
        assert_eq!(
            c["inbox"],
            Counts {
                total_emails: 2,
                unread_emails: 1,
                total_threads: 1,
                unread_threads: 1
            }
        );
        assert_eq!(c["sent"].total_threads, 2);
        assert_eq!(c["L7"].total_emails, 2);
        assert!(!c.contains_key("archive"));
        assert_eq!(v.thread_members(v.thread_key(v.get(1).unwrap())), [1, 2]);
    }
}
