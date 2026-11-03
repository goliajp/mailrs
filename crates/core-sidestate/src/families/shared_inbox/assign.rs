//! Who on the team answers a conversation. Each change is a fact appended
//! to the conversation's list; the current assignee is the last fact's
//! `to`, and the cores copy it onto every member's thread row so "assigned
//! to me" is a list each core can read off its own index.

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

use super::ConvRef;

/// One assignment, as it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignFact {
    /// Epoch seconds.
    pub at: i64,
    /// The member who assigned.
    pub by: String,
    /// The member it went to; `None` takes the assignment back.
    pub to: Option<String>,
}

/// Append `fact`.
pub fn append(conn: &mut Connection, r: ConvRef, fact: &AssignFact) -> std::io::Result<()> {
    let body = serde_json::to_vec(fact).map_err(std::io::Error::other)?;
    conn.rpush(r.key("assign").as_bytes(), &[body.as_slice()])?;
    Ok(())
}

/// Every assignment of the conversation, oldest first.
pub fn history(conn: &mut Connection, r: ConvRef) -> std::io::Result<Vec<AssignFact>> {
    Ok(conn
        .lrange(r.key("assign").as_bytes(), 0, -1)?
        .iter()
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect())
}

/// Who the conversation is assigned to now, `None` when nobody.
pub fn current(conn: &mut Connection, r: ConvRef) -> std::io::Result<Option<String>> {
    Ok(history(conn, r)?.pop().and_then(|f| f.to))
}

/// Why an assignment was refused.
#[derive(Debug)]
pub enum Refused {
    /// The caller is not a current member of the team inbox.
    NotMember,
    /// The assignee is not a current member.
    AssigneeNotMember,
    Io(std::io::Error),
}

impl From<std::io::Error> for Refused {
    fn from(e: std::io::Error) -> Self {
        Refused::Io(e)
    }
}

/// `by` assigns the conversation to `to` (`None` takes it back): the one
/// writer of both the fact and what is derived from it. The fact is
/// appended, then `write_row(member, thread_id, assignee)` copies the
/// assignee onto every member's thread, each member's webhooks get
/// `thread.assigned`, and the members' open pages are told. Returns the
/// assignee and how many threads were written.
pub fn assign(
    conn: &mut Connection,
    r: ConvRef,
    by: &str,
    to: Option<&str>,
    at: i64,
    mut write_row: impl FnMut(&str, &str, &str) -> std::io::Result<()>,
) -> Result<(Option<String>, u32), Refused> {
    use crate::families::email_groups::is_team_member;
    if !is_team_member(conn, r.gid, by)? {
        return Err(Refused::NotMember);
    }
    let to = to
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty());
    if let Some(t) = &to
        && !is_team_member(conn, r.gid, t)?
    {
        return Err(Refused::AssigneeNotMember);
    }
    append(
        conn,
        r,
        &AssignFact {
            at,
            by: by.trim().to_ascii_lowercase(),
            to: to.clone(),
        },
    )?;
    let assignee = to.clone().unwrap_or_default();
    let copies = super::copies(conn, r)?;
    for (member, tid) in &copies {
        write_row(member, tid, &assignee)?;
        let event = crate::families::webhook_events::Event {
            kind: crate::families::webhook_events::kinds::THREAD_ASSIGNED,
            user: member,
            sender: None,
            thread_id: Some(tid),
            data: serde_json::json!({ "gid": r.gid, "conv": r.conv, "assignee": to }),
        };
        crate::families::webhook_events::enqueue(conn, &event, at)?;
    }
    let members = super::members(conn, r.gid)?;
    super::publish(
        conn,
        &members,
        &serde_json::json!({
            "type": "SharedAssigned",
            "gid": r.gid,
            "conv": r.conv,
            "assignee": to,
        }),
    )?;
    Ok((to, copies.len() as u32))
}
