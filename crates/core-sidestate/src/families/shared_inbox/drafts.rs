//! The reply a team member is writing. The draft itself stays in its
//! author's own drafts; the conversation carries a pointer to it, so the
//! rest of the team sees who is writing and can read it. One author at a
//! time: another member takes it over by copying it into their own drafts,
//! a fact that is kept, and from then on only they move the pointer.

use std::path::Path;

use kevy_client::Connection;
use mailrs_core_api::method::admin::DraftWire;
use serde::{Deserialize, Serialize};

use super::ConvRef;
use crate::families::prefs::{drafts_counter_key, drafts_key};
use crate::families::uploads;

/// The reply being written for the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftPointer {
    pub author: String,
    pub draft_id: i64,
    pub updated_at: i64,
}

/// One takeover, as it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    pub at: i64,
    pub from: String,
    pub to: String,
    /// The draft in `from`'s drafts that was taken over.
    pub from_draft_id: i64,
    /// Its copy in `to`'s drafts.
    pub draft_id: i64,
}

/// The current pointer, if anyone is writing.
pub fn pointer(conn: &mut Connection, r: ConvRef) -> std::io::Result<Option<DraftPointer>> {
    Ok(conn
        .get(r.key("draft").as_bytes())?
        .and_then(|v| serde_json::from_slice(&v).ok()))
}

fn put(conn: &mut Connection, r: ConvRef, p: &DraftPointer) -> std::io::Result<()> {
    let body = serde_json::to_vec(p).map_err(std::io::Error::other)?;
    conn.set(r.key("draft").as_bytes(), &body)?;
    Ok(())
}

/// `author` saved draft `draft_id` as the conversation's reply. Refused
/// with the current pointer when someone else holds it: after a takeover
/// the earlier author's saves no longer move it.
pub fn announce(
    conn: &mut Connection,
    r: ConvRef,
    author: &str,
    draft_id: i64,
    now: i64,
) -> std::io::Result<Result<DraftPointer, DraftPointer>> {
    let author = author.trim().to_ascii_lowercase();
    if let Some(held) = pointer(conn, r)?
        && held.author != author
    {
        return Ok(Err(held));
    }
    let p = DraftPointer {
        author,
        draft_id,
        updated_at: now,
    };
    put(conn, r, &p)?;
    Ok(Ok(p))
}

/// The reply was sent or thrown away by its author: nobody is writing.
pub fn clear(conn: &mut Connection, r: ConvRef, author: &str) -> std::io::Result<bool> {
    match pointer(conn, r)? {
        Some(p) if p.author.eq_ignore_ascii_case(author.trim()) => {
            conn.del(&[r.key("draft").as_bytes()])?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// The draft the pointer names, read from its author's drafts.
pub fn read(conn: &mut Connection, p: &DraftPointer) -> std::io::Result<Option<DraftWire>> {
    Ok(conn
        .hget(
            drafts_key(&p.author).as_bytes(),
            p.draft_id.to_string().as_bytes(),
        )?
        .and_then(|v| serde_json::from_slice(&v).ok()))
}

/// `to` takes the reply over: the draft is copied into their drafts, the
/// takeover recorded, and the pointer moved to the copy, which answers
/// `to`'s own copy of the conversation. `None` when
/// nobody is writing, the draft is gone, or `to` already holds it.
///
/// The draft's files are given to `to` first, in the upload store under
/// `root`; when one cannot be, nothing is copied, so no draft names a file
/// its owner does not have.
pub fn take_over(
    conn: &mut Connection,
    root: &Path,
    r: ConvRef,
    to: &str,
    now: i64,
) -> std::io::Result<Option<Handoff>> {
    take_over_linking(conn, root, r, to, now, |a, b| std::fs::hard_link(a, b))
}

pub(crate) fn take_over_linking(
    conn: &mut Connection,
    root: &Path,
    r: ConvRef,
    to: &str,
    now: i64,
    link: fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<Option<Handoff>> {
    let to = to.trim().to_ascii_lowercase();
    let Some(p) = pointer(conn, r)? else {
        return Ok(None);
    };
    if p.author == to {
        return Ok(None);
    }
    let Some(mut draft) = read(conn, &p)? else {
        return Ok(None);
    };
    let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(now.max(0) as u64);
    for a in &draft.attachments {
        let Some(sha) = uploads::sha_of(&a.blob_id) else {
            return Err(std::io::Error::other(format!(
                "draft file {} is not an upload",
                a.blob_id
            )));
        };
        uploads::give(root, &p.author, &to, &sha, at, link)?;
    }
    let id = conn.incr(drafts_counter_key(&to).as_bytes())?;
    draft.id = id;
    // the reply answers the taker's own copy of the conversation
    draft.reply_to_thread_id = super::copies(conn, r)?
        .into_iter()
        .find(|(m, _)| *m == to)
        .map(|(_, tid)| tid);
    draft.created_at = now;
    draft.updated_at = now;
    crate::families::draft_store::put(conn, &to, &draft)?;
    let h = Handoff {
        at: now,
        from: p.author,
        to: to.clone(),
        from_draft_id: p.draft_id,
        draft_id: id,
    };
    let fact = serde_json::to_vec(&h).map_err(std::io::Error::other)?;
    conn.rpush(r.key("handoffs").as_bytes(), &[fact.as_slice()])?;
    put(
        conn,
        r,
        &DraftPointer {
            author: to,
            draft_id: id,
            updated_at: now,
        },
    )?;
    Ok(Some(h))
}

/// Every takeover of the conversation's reply, oldest first.
pub fn handoffs(conn: &mut Connection, r: ConvRef) -> std::io::Result<Vec<Handoff>> {
    Ok(conn
        .lrange(r.key("handoffs").as_bytes(), 0, -1)?
        .iter()
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect())
}
