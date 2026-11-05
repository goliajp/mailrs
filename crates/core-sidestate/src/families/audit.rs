//! The audit log: one immutable fact per admin-side action, appended by
//! whichever process took it. Newest first, capped at [`CAP`] rows; a
//! counter gives each row a stable id.

use serde::{Deserialize, Serialize};

use crate::keys::admin::{AUDIT_LOG, AUDIT_LOG_COUNTER};

/// Retention cap — ~90 days of typical admin activity.
pub const CAP: i64 = 50_000;

/// One immutable audit fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFact {
    /// Monotonic row id.
    pub id: i64,
    /// When the action happened (unix seconds). Fact time.
    pub occurred_at: i64,
    /// When we recorded it. System time.
    pub recorded_at: i64,
    /// Authenticated actor (email) or "system".
    pub actor: String,
    /// Verb: `account.create`, `alias.delete`, `sieve.update`, …
    pub action: String,
    /// What was acted on (address / id / name).
    pub target: String,
    /// Free-text context (old→new, reason, …).
    pub detail: String,
}

/// Who the ops daemon is in the audit log.
pub const OPS_DAEMON: &str = "ops-daemon";

/// Record `action` with the ops daemon as the actor, on the shared store
/// `core` reaches. Not recorded (and logged) when that store cannot be
/// reached: the calls audited this way change nothing.
pub fn by_ops_daemon<C: crate::NetKevy>(core: &C, action: &str, detail: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let written = core
        .net_conn()
        .ok_or_else(|| std::io::Error::other("no shared store"))
        .and_then(|mut c| append(&mut c, OPS_DAEMON, action, "core-api", detail, now));
    if let Err(e) = written {
        tracing::error!(error = %e, action, "ops daemon call not audited");
    }
}

/// Append one fact recorded at `now`.
pub fn append(
    c: &mut kevy_client::Connection,
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
    now: i64,
) -> std::io::Result<()> {
    let fact = AuditFact {
        id: c.incr(AUDIT_LOG_COUNTER)?,
        occurred_at: now,
        recorded_at: now,
        actor: actor.to_string(),
        action: action.to_string(),
        target: target.to_string(),
        detail: detail.to_string(),
    };
    let payload = serde_json::to_vec(&fact).unwrap_or_default();
    c.lpush(AUDIT_LOG, &[payload.as_slice()])?;
    // kevy-client has no LTRIM: when the list grows a window past the cap,
    // rewrite it to the newest CAP rows, once per CAP/10 rows
    let len = c.llen(AUDIT_LOG)? as i64;
    if len > CAP + CAP / 10 {
        let keep = c.lrange(AUDIT_LOG, 0, CAP - 1)?;
        c.del(&[AUDIT_LOG])?;
        for row in keep.iter().rev() {
            c.lpush(AUDIT_LOG, &[row.as_slice()])?;
        }
    }
    Ok(())
}

/// One row a core writes for an action it has already taken. The action
/// stands either way, so a row that cannot be written (no network store,
/// a failed write) is logged rather than returned.
pub fn record_by_core(
    net: &impl crate::NetKevy,
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
    now: i64,
) {
    let Some(mut c) = net.net_conn() else {
        tracing::warn!(%action, %target, "no network kevy: the audit line was not written");
        return;
    };
    if let Err(error) = append(&mut c, actor, action, target, detail, now) {
        tracing::warn!(%error, %action, %target, "the audit line was not written");
    }
}

/// An account made, written by the core that made it, from either core.
pub const ACCOUNT_CREATE: &str = "account.create";
/// An account removed, written by the core that removed it.
pub const ACCOUNT_DELETE: &str = "account.delete";

/// The action of a conversation's mail deleted for good, from any entry
/// point: the web, MCP, JMAP, IMAP.
pub const DELETED_FOREVER: &str = "mail.deleted_forever";

/// How the deleting request signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// A web or app session.
    Session,
    /// A password (account or app password) over HTTP Basic, IMAP or POP3.
    PasswordOrAppPassword,
}

impl Credential {
    pub fn as_str(self) -> &'static str {
        match self {
            Credential::Session => "session",
            Credential::PasswordOrAppPassword => "password_or_app_password",
        }
    }
}

/// One row for `messages` messages of `user`'s conversation `thread_id`
/// deleted for good through `via` (`web`, `mcp`, `jmap`, `imap`). Only the
/// first 8 characters of the conversation id are kept.
pub fn deleted_forever(
    c: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    messages: usize,
    via: &str,
    credential: Credential,
    now: i64,
) -> std::io::Result<()> {
    let prefix: String = thread_id.chars().take(8).collect();
    let detail = format!(
        "messages={messages} via={via} credential={}",
        credential.as_str()
    );
    append(c, user, DELETED_FOREVER, &prefix, &detail, now)
}

/// [`deleted_forever`] for messages deleted one by one (an IMAP EXPUNGE):
/// one row per conversation, `conversations` naming each message's, or
/// `None` for a file no conversation held. Rows go in first-seen order.
pub fn deleted_forever_each(
    c: &mut kevy_client::Connection,
    user: &str,
    conversations: &[Option<String>],
    via: &str,
    credential: Credential,
    now: i64,
) -> std::io::Result<()> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for tid in conversations {
        let tid = tid.as_deref().unwrap_or("(none)");
        match counts.iter_mut().find(|(t, _)| *t == tid) {
            Some((_, n)) => *n += 1,
            None => counts.push((tid, 1)),
        }
    }
    for (tid, n) in counts {
        deleted_forever(c, user, tid, n, via, credential, now)?;
    }
    Ok(())
}

/// The action of a Trash purge pass deleting an account's mail that had
/// been in the Trash 30 days.
pub const TRASH_PURGED: &str = "mail.trash_purged";

/// What one purge pass deleted, per account, in the order first deleted.
#[derive(Debug, Default)]
pub struct Purged(Vec<(String, PurgedTotals)>);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgedTotals {
    pub conversations: usize,
    pub messages: usize,
    pub bytes: u64,
}

impl Purged {
    /// One conversation of `user`'s, `messages` files and `bytes`.
    pub fn add(&mut self, user: &str, messages: usize, bytes: u64) {
        let i = match self.0.iter().position(|(u, _)| u == user) {
            Some(i) => i,
            None => {
                self.0.push((user.to_string(), PurgedTotals::default()));
                self.0.len() - 1
            }
        };
        let t = &mut self.0[i].1;
        t.conversations += 1;
        t.messages += messages;
        t.bytes += bytes;
    }

    /// One row per account, actor `system`; none for a pass that deleted
    /// nothing. How many conversations were deleted. A row that cannot be
    /// written is logged: the mail is already gone.
    pub fn record(self, c: Option<&mut kevy_client::Connection>, now: i64) -> usize {
        let total = self.0.iter().map(|(_, t)| t.conversations).sum();
        let Some(c) = c else {
            if total > 0 {
                tracing::warn!(
                    total,
                    "trash purge audit rows not written: no network store"
                );
            }
            return total;
        };
        for (user, t) in self.0 {
            let detail = format!(
                "conversations={} messages={} bytes={}",
                t.conversations, t.messages, t.bytes
            );
            if let Err(e) = append(c, "system", TRASH_PURGED, &user, &detail, now) {
                tracing::warn!(error = %e, %user, "trash purge audit row not written");
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_purge_writes_one_row_per_account_and_none_when_empty() {
        let mut c = kevy_client::Connection::connect("mem://audit-trash-purged").unwrap();
        assert_eq!(Purged::default().record(Some(&mut c), 7), 0);
        assert_eq!(c.llen(AUDIT_LOG).unwrap(), 0);
        let mut p = Purged::default();
        p.add("a@x.test", 2, 100);
        p.add("b@x.test", 1, 5);
        p.add("a@x.test", 3, 50);
        assert_eq!(p.record(Some(&mut c), 7), 3);
        let rows: Vec<AuditFact> = c
            .lrange(AUDIT_LOG, 0, -1)
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(r).unwrap())
            .collect();
        let got: Vec<(&str, &str, &str)> = rows
            .iter()
            .map(|f| (f.actor.as_str(), f.target.as_str(), f.detail.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("system", "b@x.test", "conversations=1 messages=1 bytes=5"),
                ("system", "a@x.test", "conversations=2 messages=5 bytes=150"),
            ]
        );
    }

    #[test]
    fn a_deletion_keeps_the_id_prefix_count_and_credential() {
        let mut c = kevy_client::Connection::connect("mem://audit-deleted-forever").unwrap();
        deleted_forever(
            &mut c,
            "u@x.test",
            "0123456789abcdef",
            3,
            "jmap",
            Credential::PasswordOrAppPassword,
            7,
        )
        .unwrap();
        let row = c.lrange(AUDIT_LOG, 0, 0).unwrap();
        let fact: AuditFact = serde_json::from_slice(&row[0]).unwrap();
        assert_eq!(
            (
                fact.actor.as_str(),
                fact.action.as_str(),
                fact.target.as_str()
            ),
            ("u@x.test", DELETED_FOREVER, "01234567")
        );
        assert_eq!(
            fact.detail,
            "messages=3 via=jmap credential=password_or_app_password"
        );
    }

    #[test]
    fn messages_deleted_one_by_one_are_counted_per_conversation() {
        let mut c = kevy_client::Connection::connect("mem://audit-deleted-each").unwrap();
        let convs = [
            Some("aaaaaaaaaa".to_string()),
            None,
            Some("aaaaaaaaaa".to_string()),
        ];
        deleted_forever_each(
            &mut c,
            "u@x.test",
            &convs,
            "imap",
            Credential::PasswordOrAppPassword,
            7,
        )
        .unwrap();
        let rows: Vec<AuditFact> = c
            .lrange(AUDIT_LOG, 0, -1)
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(r).unwrap())
            .collect();
        let got: Vec<(&str, &str)> = rows
            .iter()
            .map(|f| (f.target.as_str(), f.detail.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "(none)",
                    "messages=1 via=imap credential=password_or_app_password"
                ),
                (
                    "aaaaaaaa",
                    "messages=2 via=imap credential=password_or_app_password"
                ),
            ]
        );
    }
}
