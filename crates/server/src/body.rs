//! Where a message body lives: in its mailbox's Maildir++ folder under the
//! user's maildir, as fastcore keeps it. INBOX is the user's maildir itself;
//! any other mailbox is the `.Name` subfolder beside it.
//!
//! A row's `maildir_id` is the body's blob ref relative to the user's
//! maildir: the bare file name for INBOX, `.Name/<file>` for a subfolder —
//! the form fastcore stores and webapi reads, so a body is found the same
//! way whichever core wrote it. Every reader resolves it with
//! [`mailrs_maildir::locate`].

use std::io;
use std::path::PathBuf;

use mailrs_intake::maildir::maildir_subfolder;
use mailrs_maildir::{Maildir, MessageId};

use crate::message_store::MessageStore;

/// The user's maildir under `maildir_root`.
pub(crate) fn user_root(maildir_root: &str, user: &str) -> io::Result<PathBuf> {
    let (local, domain) = user.split_once('@').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{user} is not an address"),
        )
    })?;
    Ok(PathBuf::from(maildir_root).join(domain).join(local))
}

/// The folder `mailbox`'s bodies are in, relative to the user's maildir;
/// `None` for INBOX, which is the maildir itself.
fn subfolder(mailbox: &str) -> Option<String> {
    match mailbox.eq_ignore_ascii_case("INBOX") {
        true => None,
        false => Some(maildir_subfolder(mailbox)),
    }
}

/// The directory `mailbox`'s bodies are in.
pub(crate) fn folder(user_root: &std::path::Path, mailbox: &str) -> PathBuf {
    match subfolder(mailbox) {
        Some(sub) => user_root.join(sub),
        None => user_root.to_path_buf(),
    }
}

/// The blob ref of file `id` in `mailbox`.
pub(crate) fn blob_ref(mailbox: &str, id: &str) -> String {
    match subfolder(mailbox) {
        Some(sub) => format!("{sub}/{id}"),
        None => id.to_string(),
    }
}

/// The maildir and file a row's `maildir_id` names.
pub(crate) fn locate(
    maildir_root: &str,
    user: &str,
    maildir_id: &str,
) -> io::Result<(Maildir, MessageId)> {
    mailrs_maildir::locate(user_root(maildir_root, user)?, maildir_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{user}: a message with no file"),
        )
    })
}

/// The body a row's `maildir_id` names; `None` when the file is gone.
pub(crate) async fn read(
    store: &dyn MessageStore,
    maildir_root: &str,
    user: &str,
    maildir_id: &str,
) -> io::Result<Option<Vec<u8>>> {
    let (dir, id) = locate(maildir_root, user, maildir_id)?;
    store.fetch(&dir.path().to_string_lossy(), &id).await
}

/// Write `bytes` into `mailbox`'s folder; returns the new body's blob ref.
pub(crate) async fn deliver(
    store: &dyn MessageStore,
    maildir_root: &str,
    user: &str,
    mailbox: &str,
    bytes: &[u8],
) -> io::Result<String> {
    let dir = folder(&user_root(maildir_root, user)?, mailbox);
    let ids = store
        .deliver_batch(&dir.to_string_lossy(), &[bytes])
        .await?;
    let id = ids
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::other("delivery returned no message id"))?;
    Ok(blob_ref(mailbox, &id.0))
}

/// Remove the body a row's `maildir_id` names. One already gone is gone.
pub(crate) async fn remove(
    store: &dyn MessageStore,
    maildir_root: &str,
    user: &str,
    maildir_id: &str,
) -> io::Result<()> {
    let (dir, id) = locate(maildir_root, user, maildir_id)?;
    match store.delete(&dir.path().to_string_lossy(), &id).await {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_is_the_maildir_and_other_mailboxes_are_subfolders() {
        let root = user_root("/m", "bob@x.test").unwrap();
        assert_eq!(root, PathBuf::from("/m/x.test/bob"));
        assert_eq!(folder(&root, "INBOX"), root);
        assert_eq!(folder(&root, "Sent"), root.join(".Sent"));
        assert_eq!(blob_ref("inbox", "1.M2"), "1.M2");
        assert_eq!(blob_ref("Sent", "1.M2"), ".Sent/1.M2");
    }

    #[test]
    fn a_blob_ref_locates_back_to_its_folder() {
        let (dir, id) = locate("/m", "bob@x.test", &blob_ref("Sent", "1.M2")).unwrap();
        assert_eq!(dir.path(), std::path::Path::new("/m/x.test/bob/.Sent"));
        assert_eq!(id.0, "1.M2");
        let (dir, _) = locate("/m", "bob@x.test", "1.M2").unwrap();
        assert_eq!(dir.path(), std::path::Path::new("/m/x.test/bob"));
    }
}
