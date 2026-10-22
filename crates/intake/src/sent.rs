//! The sender's own copy of a message they sent, filed by the core that
//! owns their mailbox.
//!
//! The copy is written into the Sent folder and then indexed exactly like
//! an arriving message the user wrote: the same derived facts, the same
//! side effects (usage, contacts, realtime), the same store writes. The
//! only difference is that it is the user's own whatever its `From:` says,
//! since a send-as address is still the user writing.

use std::io;
use std::path::PathBuf;

use kevy_client::Connection;

use crate::arrival::{Arrival, Threads, derive};
use crate::maildir::ensure_maildir;

/// The Maildir++ folder sent copies are filed in.
pub const SENT_FOLDER: &str = ".Sent";

/// Contacts kept per user; the least recently written-to are dropped past it.
const CONTACTS_CAP: usize = 2000;

/// A core's store, as filing a sent copy writes to it.
pub trait SentStore: Threads {
    fn maildir_root(&self) -> &str;
    fn net_conn(&self) -> Option<Connection>;
    /// Index the arrival `a`, already written as `blob_ref`, as the
    /// user's own message.
    fn index_own(&self, user: &str, blob_ref: &str, body: &[u8], a: &Arrival<'_>)
    -> io::Result<()>;
}

/// Where a filed copy landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filed {
    pub thread_id: String,
    pub message_id: String,
    /// `.Sent/<file>`, resolvable by `mailrs_maildir::locate`.
    pub blob_ref: String,
}

/// File `body` as `user`'s sent copy. `recipients` is the envelope (To,
/// Cc and Bcc): the Bcc addresses are not in the bytes, and every one of
/// them joins the user's contacts.
pub fn file<S: SentStore + ?Sized>(
    core: &S,
    user: &str,
    body: &[u8],
    recipients: &[String],
) -> io::Result<Filed> {
    let root = core.maildir_root();
    let folder = sent_dir(root, user)?;
    let md = mailrs_maildir::Maildir::create(&folder)?;
    let id = md.deliver(body)?;
    let blob_ref = format!("{SENT_FOLDER}/{}", id.0);
    let mut a = derive(core, core.net_conn(), user, &blob_ref, body, "Sent");
    a.is_own = true;
    crate::effects::before_store(core.net_conn(), user, &a, body.len());
    core.index_own(user, &blob_ref, body, &a)?;
    crate::effects::after_store(user, &a, false);
    // read on disk too, for IMAP clients reading the folder
    md.mark_processed(&id, &[mailrs_maildir::Flag::Seen])?;
    if let Some(mut conn) = core.net_conn() {
        record_recipients(&mut conn, user, recipients)?;
    }
    Ok(Filed {
        thread_id: a.thread_id,
        message_id: a.message_id,
        blob_ref,
    })
}

fn sent_dir(root: &str, user: &str) -> io::Result<PathBuf> {
    let Some((local, domain)) = user.split_once('@') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not an address: {user}"),
        ));
    };
    if !ensure_maildir(root, user) {
        return Err(io::Error::other(format!("no maildir for {user}")));
    }
    Ok(PathBuf::from(root)
        .join(domain)
        .join(local)
        .join(SENT_FOLDER))
}

/// Remember who the user wrote to, for address autocomplete. Mock
/// addresses are not people and are left out.
pub fn record_recipients(
    conn: &mut Connection,
    user: &str,
    recipients: &[String],
) -> io::Result<()> {
    let key = format!("mailrs:user:{user}:contacts");
    let ts_key = format!("{key}:ts");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as f64;
    for raw in recipients {
        let shown = raw.trim();
        let addr = addr_spec(shown);
        if addr.is_empty() || mailrs_core_sidestate::families::mock::is_mock(addr) {
            continue;
        }
        conn.hset(key.as_bytes(), &[(addr.as_bytes(), shown.as_bytes())])
            .map_err(io::Error::other)?;
        conn.zadd(ts_key.as_bytes(), &[(now, addr.as_bytes())])
            .map_err(io::Error::other)?;
    }
    let size = conn.zcard(ts_key.as_bytes()).map_err(io::Error::other)?;
    if size <= CONTACTS_CAP {
        return Ok(());
    }
    let oldest = conn
        .zrange(ts_key.as_bytes(), 0, (size - CONTACTS_CAP) as i64 - 1)
        .map_err(io::Error::other)?;
    let refs: Vec<&[u8]> = oldest.iter().map(Vec::as_slice).collect();
    conn.hdel(key.as_bytes(), &refs).map_err(io::Error::other)?;
    conn.zrem(ts_key.as_bytes(), &refs)
        .map_err(io::Error::other)?;
    Ok(())
}

/// `user@host` out of `Name <user@host>`, or the token itself.
fn addr_spec(raw: &str) -> &str {
    match (raw.rfind('<'), raw.rfind('>')) {
        (Some(start), Some(end)) if end > start => raw[start + 1..end].trim(),
        _ => raw,
    }
}

#[cfg(test)]
#[path = "sent_tests.rs"]
mod tests;
