//! Internal notes on a team conversation: what the team says to itself
//! about a message, never to its sender.
//!
//! Stored as facts, oldest first: a note, an edit of one, a deletion of
//! one. Reading folds them: an edit replaces the text, a deletion hides the
//! note. The text a deletion hid stays in the list; [`deleted_original`]
//! is the one reader that returns it, and its caller must record that it
//! did.

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

use super::{ConvRef, note_counter_key};

/// One fact about a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteFact {
    Add {
        id: i64,
        at: i64,
        by: String,
        text: String,
    },
    Edit {
        of: i64,
        at: i64,
        by: String,
        text: String,
    },
    Delete {
        of: i64,
        at: i64,
        by: String,
    },
}

/// A note as the team sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub at: i64,
    pub by: String,
    pub text: String,
    /// When it was last edited, if it was.
    pub edited_at: Option<i64>,
}

/// What a deleted note said, every version of it, for the audit view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedNote {
    pub id: i64,
    pub by: String,
    pub deleted_at: i64,
    pub deleted_by: String,
    /// Oldest first.
    pub versions: Vec<String>,
}

/// Why a change to a note was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// No such note, or it is deleted.
    Missing,
    /// Only the author changes a note.
    NotAuthor,
}

fn facts(conn: &mut Connection, r: ConvRef) -> std::io::Result<Vec<NoteFact>> {
    Ok(conn
        .lrange(r.key("notes").as_bytes(), 0, -1)?
        .iter()
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect())
}

fn push(conn: &mut Connection, r: ConvRef, fact: &NoteFact) -> std::io::Result<()> {
    let body = serde_json::to_vec(fact).map_err(std::io::Error::other)?;
    conn.rpush(r.key("notes").as_bytes(), &[body.as_slice()])?;
    Ok(())
}

/// Add a note by `by`; its id.
pub fn add(
    conn: &mut Connection,
    r: ConvRef,
    by: &str,
    text: &str,
    at: i64,
) -> std::io::Result<i64> {
    let id = conn.incr(note_counter_key(r.gid).as_bytes())?;
    push(
        conn,
        r,
        &NoteFact::Add {
            id,
            at,
            by: by.to_string(),
            text: text.to_string(),
        },
    )?;
    Ok(id)
}

/// The live notes, oldest first, each with its latest text.
pub fn list(conn: &mut Connection, r: ConvRef) -> std::io::Result<Vec<Note>> {
    let mut notes: Vec<Note> = Vec::new();
    for fact in facts(conn, r)? {
        match fact {
            NoteFact::Add { id, at, by, text } => notes.push(Note {
                id,
                at,
                by,
                text,
                edited_at: None,
            }),
            NoteFact::Edit { of, at, text, .. } => {
                if let Some(n) = notes.iter_mut().find(|n| n.id == of) {
                    n.text = text;
                    n.edited_at = Some(at);
                }
            }
            NoteFact::Delete { of, .. } => notes.retain(|n| n.id != of),
        }
    }
    Ok(notes)
}

fn author_of(conn: &mut Connection, r: ConvRef, id: i64) -> std::io::Result<Option<String>> {
    Ok(list(conn, r)?
        .into_iter()
        .find(|n| n.id == id)
        .map(|n| n.by))
}

fn may_change(
    conn: &mut Connection,
    r: ConvRef,
    id: i64,
    by: &str,
) -> std::io::Result<Result<(), Refused>> {
    Ok(match author_of(conn, r, id)? {
        None => Err(Refused::Missing),
        Some(author) if !author.eq_ignore_ascii_case(by) => Err(Refused::NotAuthor),
        Some(_) => Ok(()),
    })
}

/// Replace note `id`'s text; only its author may.
pub fn edit(
    conn: &mut Connection,
    r: ConvRef,
    id: i64,
    by: &str,
    text: &str,
    at: i64,
) -> std::io::Result<Result<(), Refused>> {
    if let Err(e) = may_change(conn, r, id, by)? {
        return Ok(Err(e));
    }
    push(
        conn,
        r,
        &NoteFact::Edit {
            of: id,
            at,
            by: by.to_string(),
            text: text.to_string(),
        },
    )?;
    Ok(Ok(()))
}

/// Hide note `id`; only its author may. Its text stays in the facts.
pub fn delete(
    conn: &mut Connection,
    r: ConvRef,
    id: i64,
    by: &str,
    at: i64,
) -> std::io::Result<Result<(), Refused>> {
    if let Err(e) = may_change(conn, r, id, by)? {
        return Ok(Err(e));
    }
    push(
        conn,
        r,
        &NoteFact::Delete {
            of: id,
            at,
            by: by.to_string(),
        },
    )?;
    Ok(Ok(()))
}

/// What deleted note `id` said. `None` unless it was deleted. The caller
/// records each read: this is the one path to a deleted note's text.
pub fn deleted_original(
    conn: &mut Connection,
    r: ConvRef,
    id: i64,
) -> std::io::Result<Option<DeletedNote>> {
    let mut out: Option<DeletedNote> = None;
    let mut deleted = None;
    for fact in facts(conn, r)? {
        match fact {
            NoteFact::Add {
                id: n, by, text, ..
            } if n == id => {
                out = Some(DeletedNote {
                    id,
                    by,
                    deleted_at: 0,
                    deleted_by: String::new(),
                    versions: vec![text],
                });
            }
            NoteFact::Edit { of, text, .. } if of == id => {
                if let Some(o) = out.as_mut() {
                    o.versions.push(text);
                }
            }
            NoteFact::Delete { of, at, by } if of == id => deleted = Some((at, by)),
            _ => {}
        }
    }
    let (Some(mut o), Some((at, by))) = (out, deleted) else {
        return Ok(None);
    };
    o.deleted_at = at;
    o.deleted_by = by;
    Ok(Some(o))
}
