//! Team inboxes in this store: the two declared columns a member's copy of
//! a team conversation carries (`shared_conv`, `assignee`), the "assigned
//! to me" axis over them (`table_query::assignee`), and the reply claim, decided in one atomic block
//! so two members' claims on the same count cannot both win.

use std::io;

use mailrs_mailbox::shared_reply::{self, Claim, ReplyState};

use crate::KevyMailboxStore;
use crate::keys;

/// The declared columns this module writes and nothing else does.
pub(crate) const COLUMNS: [&str; 2] = ["shared_conv", "assignee"];

fn reply_key(tag: &str) -> String {
    format!("mailrs:shared-reply:{tag}")
}

impl KevyMailboxStore {
    /// Mark `user`'s thread as their copy of team conversation `tag`,
    /// assigned to `assignee` (empty for nobody). A row that already says
    /// so is left alone.
    pub fn set_shared_copy(
        &self,
        user: &str,
        thread_id: &str,
        tag: &str,
        assignee: &str,
    ) -> io::Result<()> {
        let key = keys::thread_user(user, thread_id);
        let (now_tag, now_assignee) = self.shared_fields(user, thread_id)?;
        if now_tag == tag && now_assignee == assignee {
            return Ok(());
        }
        self.store().hset(
            key.as_bytes(),
            &[
                (b"shared_conv" as &[u8], tag.as_bytes()),
                (b"assignee", assignee.as_bytes()),
            ],
        )?;
        Ok(())
    }

    /// Copy the team's assignee onto `user`'s thread.
    pub fn set_assignee(&self, user: &str, thread_id: &str, assignee: &str) -> io::Result<()> {
        let key = keys::thread_user(user, thread_id);
        if self.store().hget(key.as_bytes(), b"assignee")?.as_deref() == Some(assignee.as_bytes()) {
            return Ok(());
        }
        self.store().hset(
            key.as_bytes(),
            &[(b"assignee" as &[u8], assignee.as_bytes())],
        )?;
        Ok(())
    }

    /// The row's team conversation (empty when it is not a team copy) and
    /// assignee (empty for nobody).
    pub fn shared_fields(&self, user: &str, thread_id: &str) -> io::Result<(String, String)> {
        let key = keys::thread_user(user, thread_id);
        let read = |f: &[u8]| -> io::Result<String> {
            Ok(self
                .store()
                .hget(key.as_bytes(), f)?
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default())
        };
        Ok((read(b"shared_conv")?, read(b"assignee")?))
    }

    /// The reply claims of team conversation `tag`.
    pub fn reply_state(&self, tag: &str) -> io::Result<ReplyState> {
        Ok(self
            .store()
            .get(reply_key(tag).as_bytes())?
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default())
    }

    /// Decide `user`'s claim on the reply and store it when it wins, in
    /// one atomic block.
    pub fn claim_reply(
        &self,
        tag: &str,
        user: &str,
        seen_seq: u64,
        force: bool,
        at: i64,
    ) -> io::Result<Claim> {
        let key = reply_key(tag);
        self.store()
            .atomic(|ctx| {
                let current: ReplyState = ctx
                    .get(key.as_bytes())?
                    .and_then(|v| serde_json::from_slice(&v).ok())
                    .unwrap_or_default();
                let decided = shared_reply::claim(&current, user, seen_seq, force, at);
                if let Claim::Won { next } = &decided {
                    let body = serde_json::to_vec(next).unwrap_or_default();
                    ctx.set(key.as_bytes(), &body);
                }
                Ok(decided)
            })
            .map_err(io::Error::from)
    }

    /// Take back claim `claim_id`, in one atomic block; whether it was
    /// still the last claim.
    pub fn release_reply(&self, tag: &str, claim_id: &str) -> io::Result<bool> {
        let key = reply_key(tag);
        self.store()
            .atomic(|ctx| {
                let current: ReplyState = ctx
                    .get(key.as_bytes())?
                    .and_then(|v| serde_json::from_slice(&v).ok())
                    .unwrap_or_default();
                let Some(back) = shared_reply::release(&current, claim_id) else {
                    return Ok(false);
                };
                let body = serde_json::to_vec(&back).unwrap_or_default();
                ctx.set(key.as_bytes(), &body);
                Ok(true)
            })
            .map_err(io::Error::from)
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
