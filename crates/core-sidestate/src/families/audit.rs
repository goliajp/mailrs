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

/// The action of a conversation's mail deleted for good, from any entry
/// point: the web, MCP, JMAP, IMAP.
pub const DELETED_FOREVER: &str = "mail.deleted_forever";

/// How the deleting request signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// A web or app session.
    Session,
    /// A password (account or app password) over HTTP Basic, IMAP or POP3.
    Password,
}

impl Credential {
    pub fn as_str(self) -> &'static str {
        match self {
            Credential::Session => "session",
            Credential::Password => "password",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deletion_keeps_the_id_prefix_count_and_credential() {
        let mut c = kevy_client::Connection::connect("mem://audit-deleted-forever").unwrap();
        deleted_forever(
            &mut c,
            "u@x.test",
            "0123456789abcdef",
            3,
            "jmap",
            Credential::Password,
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
        assert_eq!(fact.detail, "messages=3 via=jmap credential=password");
    }

    #[test]
    fn messages_deleted_one_by_one_are_counted_per_conversation() {
        let mut c = kevy_client::Connection::connect("mem://audit-deleted-each").unwrap();
        let convs = [
            Some("aaaaaaaaaa".to_string()),
            None,
            Some("aaaaaaaaaa".to_string()),
        ];
        deleted_forever_each(&mut c, "u@x.test", &convs, "imap", Credential::Password, 7).unwrap();
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
                ("(none)", "messages=1 via=imap credential=password"),
                ("aaaaaaaa", "messages=2 via=imap credential=password"),
            ]
        );
    }
}
