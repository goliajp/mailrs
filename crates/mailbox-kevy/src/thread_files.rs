//! Which files on disk belong to a thread, for one user.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// `(message id, this user's uid, this user's maildir file)` for every
    /// message of `thread_id`. The uid and file are read from the user's own
    /// row first: the shared blob is stripped of them by design, and a delete
    /// that read only the shared blob found no file and left every message
    /// on disk, where IMAP kept listing it.
    pub fn thread_message_files(
        &self,
        user: &str,
        thread_id: &str,
    ) -> io::Result<Vec<(String, Option<u32>, Option<String>)>> {
        let msgs_zset = keys::thread_messages(thread_id);
        let store = self.store();
        // Enumerated outside the delete's atomic block: AtomicCtx has no
        // `zrange`, and every blob is a plain `get` — one round trip
        // per message on a typical thread (< 20 hops).
        let members = store.zrange(msgs_zset.as_bytes(), 0, -1)?;
        let mut per_msg: Vec<(String, Option<u32>, Option<String>)> =
            Vec::with_capacity(members.len());
        for (mid_bytes, _score) in &members {
            let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                continue;
            };
            let blob = store.get(keys::message_blob(mid).as_bytes())?;
            let (uid, blob_ref) = match blob.as_deref() {
                Some(bytes) => {
                    let v: serde_json::Value =
                        serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null);
                    let uid = v
                        .get("uid")
                        .and_then(|x| x.as_u64())
                        .filter(|u| *u > 0)
                        .map(|u| u as u32);
                    let blob_ref = v
                        .get("blob_ref")
                        .and_then(|x| x.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    (uid, blob_ref)
                }
                None => (None, None),
            };
            // the file and uid are this user's facts, kept on their own row;
            // the shared blob is stripped of them, so reading only it found
            // no file and every delete left the messages on disk
            let own = self.user_message_facts(user, mid)?;
            let blob_ref = own
                .as_ref()
                .map(|f| f.blob_ref.clone())
                .filter(|b| !b.is_empty())
                .or(blob_ref);
            let uid = own.as_ref().map(|f| f.uid).filter(|u| *u > 0).or(uid);
            per_msg.push((mid.to_string(), uid, blob_ref));
        }
        Ok(per_msg)
    }
}
