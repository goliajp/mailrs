//! A message brought over from another mailbox (an mbox upload), filed by
//! the core that owns the user's mailbox.
//!
//! It is written into INBOX and indexed as an arriving message is, with
//! the facts derived the same way, but nothing a delivery sets off happens:
//! no Sieve, no forwarding, no webhook, no notification. The mail arrived
//! long ago; only its place in this mailbox is new. Usage and contacts are
//! kept, because they describe what the mailbox holds.

use std::io;
use std::path::PathBuf;

use crate::arrival::derive;
use crate::sent::{Filed, SentStore};

/// File `body` into `user`'s INBOX, unread. Whether it was read, and
/// whether it was archived, the caller sets afterwards through the same
/// calls a reader makes.
pub fn file<S: SentStore + ?Sized>(core: &S, user: &str, body: &[u8]) -> io::Result<Filed> {
    let root = core.maildir_root();
    let md = mailrs_maildir::Maildir::create(&inbox_dir(root, user)?)?;
    let id = md.deliver(body)?;
    let blob_ref = id.0.clone();
    let a = derive(core, core.net_conn(), user, &blob_ref, body, "INBOX");
    crate::live_sync::adjust_usage_bytes(user, body.len() as i64);
    crate::live_sync::upsert_contacts(user, &a.from);
    core.index_arrival(user, &blob_ref, body, &a)?;
    // out of new/: an imported message is not one IMAP should call recent
    md.mark_processed(&id, &[])?;
    Ok(Filed {
        thread_id: a.thread_id,
        message_id: a.message_id,
        blob_ref,
    })
}

fn inbox_dir(root: &str, user: &str) -> io::Result<PathBuf> {
    let Some((local, domain)) = user.split_once('@') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not an address: {user}"),
        ));
    };
    if !crate::maildir::ensure_maildir(root, user) {
        return Err(io::Error::other(format!("no maildir for {user}")));
    }
    Ok(PathBuf::from(root).join(domain).join(local))
}
