//! Which conversations are held right now, for `compare`.
//!
//! A pack's diff against another pack is not what activating it does: the
//! re-judge that follows holds every message the new pack holds whose
//! conversation is not held now, whatever the old pack said. So given the
//! conversations held today, `compare` also reports the messages the
//! candidate holds in conversations that are not held, and how many of
//! those a person released.
//!
//! Files: one `account<TAB>thread_id[<TAB>message_id]` per line. With the
//! message id, a file is matched to its conversation exactly. Without it,
//! a file is matched by its own `Message-ID`, its first `References` id or
//! its `In-Reply-To`, which finds a conversation by its root and can miss
//! one whose id is none of those.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;

type Key = (String, String);

#[derive(Default)]
pub struct Threads {
    held: HashSet<Key>,
    released: HashSet<Key>,
    /// (account, message id) → thread id, from a third column.
    by_message: HashMap<Key, String>,
}

/// What the candidate holds in conversations not held now.
#[derive(Default)]
pub struct Unheld {
    messages: u64,
    threads: BTreeSet<Key>,
    released_threads: BTreeSet<Key>,
}

fn read(
    path: &str,
    into: &mut HashSet<Key>,
    by_message: &mut HashMap<Key, String>,
) -> Result<(), String> {
    for line in fs::read_to_string(path)
        .map_err(|e| format!("{path}: {e}"))?
        .lines()
    {
        let mut cols = line.split('\t');
        let (Some(account), Some(tid)) = (cols.next(), cols.next()) else {
            continue;
        };
        let account = account.trim().to_ascii_lowercase();
        if let Some(mid) = cols.next().map(str::trim).filter(|m| !m.is_empty()) {
            by_message.insert((account.clone(), bare(mid)), tid.trim().to_string());
        }
        into.insert((account, tid.trim().to_string()));
    }
    Ok(())
}

fn bare(id: &str) -> String {
    id.trim().trim_matches(['<', '>']).to_string()
}

impl Threads {
    /// `held`: the conversations held now; `released`: those a person
    /// released, if known.
    pub fn load(held: &str, released: Option<&str>) -> Result<Self, String> {
        let mut t = Threads::default();
        read(held, &mut t.held, &mut t.by_message)?;
        if let Some(r) = released {
            read(r, &mut t.released, &mut t.by_message)?;
        }
        Ok(t)
    }

    /// The conversations `raw` may belong to, most specific first.
    fn candidates(&self, account: &str, raw: &[u8]) -> Vec<String> {
        let header = |name: &[u8]| mailrs_fraud_lua::stored::header_value(raw, name);
        let own = header(b"message-id:").map(|m| bare(&m));
        let mut out = Vec::new();
        if let Some(tid) = own
            .as_ref()
            .and_then(|m| self.by_message.get(&(account.to_string(), m.clone())))
        {
            out.push(tid.clone());
        }
        if let Some(first) =
            header(b"references:").and_then(|r| r.split_whitespace().next().map(bare))
        {
            out.push(first);
        }
        if let Some(parent) = header(b"in-reply-to:").map(|r| bare(&r)) {
            out.push(parent);
        }
        out.extend(own);
        out
    }

    /// Whether `raw`'s conversation is held now, and its key.
    pub fn held_now(&self, account: &str, raw: &[u8]) -> (bool, Option<Key>) {
        let candidates = self.candidates(account, raw);
        let key = |tid: &String| (account.to_string(), tid.clone());
        if let Some(k) = candidates.iter().map(key).find(|k| self.held.contains(k)) {
            return (true, Some(k));
        }
        let released = candidates
            .iter()
            .map(key)
            .find(|k| self.released.contains(k));
        (false, released.or_else(|| candidates.last().map(key)))
    }

    /// Count a message the candidate holds.
    pub fn count(&self, unheld: &mut Unheld, held_now: bool, key: Option<Key>) {
        if held_now {
            return;
        }
        unheld.messages += 1;
        let Some(k) = key else {
            return;
        };
        if self.released.contains(&k) {
            unheld.released_threads.insert(k.clone());
        }
        unheld.threads.insert(k);
    }
}

impl Unheld {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "messages": self.messages,
            "threads": self.threads.len(),
            "person_released_threads": self.released_threads.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn threads(held: &str, released: &str) -> Threads {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fraud-threads-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let h = dir.join("held.tsv");
        let r = dir.join("released.tsv");
        fs::write(&h, held).expect("write");
        fs::write(&r, released).expect("write");
        let t = Threads::load(h.to_str().expect("path"), Some(r.to_str().expect("path")))
            .expect("load");
        fs::remove_dir_all(&dir).expect("clean");
        t
    }

    #[test]
    fn a_reply_is_matched_to_its_held_root() {
        let t = threads("bob@x.com\troot@a\n", "");
        let raw = b"Message-ID: <reply@a>\r\nReferences: <root@a> <mid@a>\r\n\r\nx";
        assert!(t.held_now("bob@x.com", raw).0);
    }

    #[test]
    fn a_third_column_matches_a_message_exactly() {
        let t = threads("bob@x.com\tsynthetic-thread\tone@a\n", "");
        let raw = b"Message-ID: <one@a>\r\n\r\nx";
        assert!(t.held_now("bob@x.com", raw).0);
    }

    #[test]
    fn a_hold_in_an_unheld_released_conversation_is_counted_as_such() {
        let t = threads("bob@x.com\tother@a\n", "bob@x.com\troot@a\n");
        let raw = b"Message-ID: <root@a>\r\n\r\nx";
        let (now, key) = t.held_now("bob@x.com", raw);
        assert!(!now);
        let mut u = Unheld::default();
        t.count(&mut u, now, key);
        assert_eq!(
            u.json(),
            serde_json::json!({"messages": 1, "threads": 1, "person_released_threads": 1})
        );
    }

    #[test]
    fn a_hold_in_a_held_conversation_is_not_counted() {
        let t = threads("bob@x.com\troot@a\n", "");
        let raw = b"Message-ID: <root@a>\r\n\r\nx";
        let (now, key) = t.held_now("bob@x.com", raw);
        let mut u = Unheld::default();
        t.count(&mut u, now, key);
        assert_eq!(u.json()["messages"], 0);
    }
}
