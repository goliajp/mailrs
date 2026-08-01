//! Walking one user's maildir and grouping what is there into threads.
//!
//! Phase one of the self-heal sweep, and the only part that touches the
//! filesystem. It reads the first 16 KB of each file for headers — enough
//! for Message-ID, References and Subject, and a bound on a mailbox with
//! 15 MB attachments in it.

use std::sync::Arc;

use crate::{
    FastcoreState, extract_headers, extract_sender_trust, file_mtime_epoch, maildir_filename_epoch,
    maildir_seen_flag, resolve_thread_by_ancestry,
};

pub(crate) struct MailFile {
    /// blob_ref stored in the message wire. Either just the maildir
    /// filename (for INBOX/cur+new files) or `<subfolder>/<filename>`
    /// (for files under Maildir++ subfolders like `.Sent/`). The
    /// prefix lets `enrich_with_body` locate the file when it lives
    /// outside INBOX — otherwise `MaildirStore::fetch` returns None
    /// and the UI shows "(no text content)".
    pub(crate) filename: String,
    pub(crate) size: u32,
    pub(crate) message_id: String,
    pub(crate) in_reply_to: String,
    pub(crate) references: Vec<String>,
    pub(crate) subject: String,
    pub(crate) date: i64,
    pub(crate) from: String,
    pub(crate) to: String,
    /// maildir info-section \Seen flag (`...:2,...S...`) — the on-disk
    /// read/unread fact. Self-heal must respect it or every boot
    /// resurrects already-read mail as unread.
    pub(crate) seen: bool,
    /// Sender-auth verdict from the file's `Authentication-Results`
    /// header (`verified` / `suspicious` / `unverified` / `""`).
    pub(crate) sender_trust: String,
}

/// Every mail file in `user`'s maildir, parsed far enough to thread it.
///
/// `since` bounds the walk to filenames whose maildir timestamp is at
/// least that old; 0 means everything. An empty result means there is
/// nothing on disk to heal from, not that nothing needed healing.
pub(crate) fn scan_maildir(user: &str, since: i64) -> Vec<MailFile> {
    let Some((local, domain)) = user.split_once('@') else {
        return Vec::new();
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(&root).join(domain).join(local);
    // Incremental filter. Every path that writes a maildir file now
    // indexes it write-through (spool_drain, bounce, IMAP APPEND/COPY,
    // REST copy/move), so the only gap this sweep still has to close is
    // a process that died between the file landing and the index write
    // — which makes the file necessarily recent. `since = 0` means a
    // full sweep (boot + the daily backstop).
    //
    // The cutoff reads the timestamp out of the maildir filename rather
    // than stat'ing mtime: maildir names are `<epoch>.<unique>.<host>`
    // by spec and are monotonic per delivery, whereas mtime is rewritten
    // by anything that rsyncs or touches the store.
    let recent_enough = |name: &str| -> bool {
        if since == 0 {
            return true;
        }
        // Unparseable name → always inspect it; being wrong here costs
        // one header read, being wrong the other way loses a message.
        maildir_filename_epoch(name).is_none_or(|ts| ts >= since)
    };
    // Collect (subfolder_prefix, path) pairs. `subfolder_prefix` is
    // empty for INBOX and `.<foldername>` for Maildir++ subfolders.
    // It's later prepended to the blob_ref so `enrich_with_body` can
    // locate the file: INBOX files stay bare filenames (matches the
    // pg-dump migration's convention), subfolder files become
    // `.Sent/<filename>`.
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for sub in ["cur", "new"] {
        let dir = base.join(sub);
        if let Ok(iter) = std::fs::read_dir(&dir) {
            for entry in iter.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && recent_enough(&entry.file_name().to_string_lossy())
                {
                    files.push((String::new(), entry.path()));
                }
            }
        }
    }
    // Maildir++ subfolders (`.Sent`, `.Drafts`, `.Junk`, custom …).
    // IMAP clients that APPEND to `.Sent` write the user's outgoing
    // messages here — without walking these, the Sent tab is stuck at
    // "only threads whose sent-copy landed in INBOX via mirror_send".
    if let Ok(iter) = std::fs::read_dir(&base) {
        for entry in iter.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') {
                continue;
            }
            let sub_base = entry.path();
            for sub in ["cur", "new"] {
                let dir = sub_base.join(sub);
                if let Ok(iter) = std::fs::read_dir(&dir) {
                    for e in iter.flatten() {
                        if e.file_type().map(|t| t.is_file()).unwrap_or(false)
                            && recent_enough(&e.file_name().to_string_lossy())
                        {
                            files.push((name.clone(), e.path()));
                        }
                    }
                }
            }
        }
    }
    if files.is_empty() {
        return Vec::new();
    }

    // Parse headers for every file. Only load the first 16 KB.
    let mut parsed: Vec<MailFile> = Vec::with_capacity(files.len());
    for (subfolder, path) in &files {
        let bare = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        // Prepend Maildir++ subfolder when set so `enrich_with_body`
        // can route `MaildirStore::fetch` to the right sub-maildir.
        let blob_ref = if subfolder.is_empty() {
            bare.clone()
        } else {
            format!("{subfolder}/{bare}")
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let size = bytes.len() as u32;
        let head = &bytes[..bytes.len().min(16 * 1024)];
        let (message_id, in_reply_to, references, subject, date, from, to) = extract_headers(head);
        if message_id.is_empty() {
            continue;
        }
        // If the RFC 5322 `Date:` header was missing or unparseable
        // (some mailers ship malformed dates and many self-injected
        // notifications have none), fall back to the maildir delivery
        // epoch encoded in the filename, then to file mtime, then to
        // 0. Without these fallbacks the affected messages sorted to
        // 1970 and inbound replies could end up ahead of the sent
        // copy in the thread timeline.
        let date = if date > 0 {
            date
        } else {
            maildir_filename_epoch(&bare)
                .or_else(|| file_mtime_epoch(path))
                .unwrap_or(0)
        };
        parsed.push(MailFile {
            filename: blob_ref,
            size,
            message_id,
            in_reply_to,
            references,
            subject,
            date,
            from,
            to,
            seen: maildir_seen_flag(&bare),
            sender_trust: extract_sender_trust(&bytes),
        });
    }
    parsed
}

/// Group parsed files by the thread each one belongs to.
///
/// Threading goes through `resolve_thread_by_ancestry`, the same resolver
/// the ingest path uses — a second implementation here would drift from it
/// and put the sweep's repairs in different threads than the arrivals.
pub(crate) fn group_by_thread<'a>(
    state: &Arc<FastcoreState>,
    user: &str,
    parsed: &'a [MailFile],
) -> std::collections::HashMap<String, Vec<&'a MailFile>> {
    // Bucket by resolved conversation root. v2.9.5: consult the
    // msgid→thread index first (same rule as live ingest) so self-heal
    // groups a reply into the thread its ancestors actually live in;
    // the raw-header guess is only the fallback for unknown chains.
    let mut by_root: std::collections::HashMap<String, Vec<&MailFile>> =
        std::collections::HashMap::new();
    for m in parsed {
        let root = match resolve_thread_by_ancestry(
            state,
            user,
            &m.message_id,
            &m.in_reply_to,
            &m.references,
            &m.subject,
        ) {
            Some(tid) => tid,
            None => {
                if let Some(first) = m.references.first() {
                    first.clone()
                } else if !m.in_reply_to.is_empty() {
                    m.in_reply_to.clone()
                } else {
                    m.message_id.clone()
                }
            }
        };
        by_root.entry(root).or_default().push(m);
    }

    by_root
}
