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

/// File `body` into `user`'s INBOX, unread, or into Sent when its From is
/// the user. Whether it was read, and
/// whether it was archived, the caller sets afterwards through the same
/// calls a reader makes. `fraud_verdict` is what the caller judged it to
/// be, the way the receiver hands one to a delivery: the hold it implies
/// is applied here, by the core, exactly as for delivered mail.
pub fn file<S: SentStore + ?Sized>(
    core: &S,
    user: &str,
    body: &[u8],
    fraud_verdict: Option<&str>,
) -> io::Result<Filed> {
    // an import counts against the quota like any mail, and stops at it
    if crate::live_sync::quota_exceeded(user) {
        return Err(io::Error::new(
            io::ErrorKind::QuotaExceeded,
            format!("{user} is over quota"),
        ));
    }
    let body = &*crate::sent::without_forged_results(core, body);
    let root = core.maildir_root();
    // the user's own mail is filed where their sent mail is, so it reads
    // as theirs in every core and never lands in their Inbox
    let from = mailrs_rfc5322::Message::new(body)
        .header_str("From")
        .unwrap_or_default()
        .to_string();
    let own = mailrs_rfc5322::list_contains(&from, user);
    let (dir, folder) = match own {
        true => (crate::sent::sent_dir(root, user)?, "Sent"),
        false => (inbox_dir(root, user)?, "INBOX"),
    };
    let md = mailrs_maildir::Maildir::create(&dir)?;
    let id = md.deliver(body)?;
    let blob_ref = match own {
        true => format!("{}/{}", crate::sent::SENT_FOLDER, id.0),
        false => id.0.clone(),
    };
    let mut a = derive(
        core,
        core.net_conn(),
        user,
        &blob_ref,
        body,
        folder,
        core.authserv_id(),
    );
    crate::live_sync::adjust_usage_bytes(user, body.len() as i64);
    crate::live_sync::upsert_contacts(user, &a.from);
    match own {
        true => {
            a.is_own = true;
            core.index_own(user, &blob_ref, body, &a)?;
        }
        false => core.index_arrival(user, &blob_ref, body, &a, fraud_verdict)?,
    }
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
