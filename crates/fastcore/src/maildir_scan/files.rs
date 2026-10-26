//! Maildir file helpers, shared by the sweep and the maintenance routes.
//!
//! `read_maildir_file` is the one way in. Callers must not rebuild a
//! filename by hand — maildir encodes flags into it, so a message that has
//! been read does not live where its `blob_ref` says, and every sent copy
//! is marked Seen the moment it is mirrored.

use crate::extract_headers;

/// Read a message's raw bytes from its maildir file. `blob_ref` is a
/// bare filename for INBOX or `.Folder/<filename>` for a Maildir++
/// subfolder; both `cur` and `new` are tried since a message moves
/// between them as flags change. `None` when the ref is empty or the
/// file is gone.
pub(crate) fn read_maildir_file(user: &str, blob_ref: &str) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(root).join(domain).join(local);
    // Both the reference convention and the `:2,FLAGS` matching live in the
    // stone. This function held its own version of each: it split on `/`
    // unconditionally and rebuilt the filename by hand, so a message that
    // had been marked Seen was unreadable and the threading backfill lost
    // every `References` edge belonging to a sent copy.
    let (dir, id) = mailrs_maildir::locate(&base, blob_ref)?;
    dir.fetch(&id).ok().flatten()
}

/// Apply a `u32` flag bitmask to a message's file, keeping the flags the
/// mask cannot express. `Ok(false)` means the file already said this, or
/// there is no file — nothing was renamed.
///
/// **Not a replacement of the flag set.** `maildir_flags_to_bitmask` maps
/// `P` (passed / forwarded) to `0`, so a mask can neither carry it nor
/// distinguish "not passed" from "cannot say". Writing the mask's flags
/// outright would delete a `P` that is on the file, and nothing would
/// report the loss — `mailrs_maildir`'s own
/// `a_bitmask_round_trip_would_lose_passed` demonstrates it. So: read what
/// is there, set the bits this caller owns, keep the rest.
///
/// The boolean matters to callers that report progress: a backfill has to
/// tell "changed 14,704" from "walked 32,445 and everything already
/// agreed", or its counter cannot come out zero.
/// A user's mailbox directory, or `None` for an address with no domain.
///
/// One definition: this was spelled out in four places — here, and once in
/// each of the three sidecar modules — which is the shape
/// `check-shared-keys.sh` exists to catch for kevy keys. A path built
/// two ways is a file found by one caller and not the other, and this repo
/// has already paid for that once in `locate`.
pub(crate) fn mailbox_dir(user: &str) -> Option<std::path::PathBuf> {
    let (local, domain) = user.split_once('@')?;
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    Some(std::path::PathBuf::from(root).join(domain).join(local))
}

/// Every message file in a mailbox, by base id, with what its name says.
///
/// One directory scan for the whole mailbox. The alternative — asking per
/// message — costs a `read_dir` of `cur/` **per message**, because that is
/// how a flag suffix is matched, so a thirty-thousand-message mailbox does
/// a billion directory entries. `maintenance:reindex` took minutes on
/// production that way before this existed.
pub(crate) fn names_on_disk(
    state: &std::sync::Arc<crate::FastcoreState>,
    user: &str,
) -> std::collections::HashMap<String, (Vec<mailrs_maildir::Flag>, Vec<char>)> {
    let mut out = std::collections::HashMap::new();
    for e in entries_on_disk(state, user) {
        // keywords come off the name the flags were parsed from, no stat
        let name = e.path.file_name().unwrap_or_default().to_string_lossy();
        let keywords = match name.find(':') {
            Some(i) => mailrs_maildir::keywords_of(&name[i..]),
            None => Vec::new(),
        };
        out.insert(e.id.0, (e.flags, keywords));
    }
    out
}

/// [`names_on_disk`]'s single scan, giving each file's path.
pub(crate) fn paths_on_disk(
    state: &std::sync::Arc<crate::FastcoreState>,
    user: &str,
) -> std::collections::HashMap<String, std::path::PathBuf> {
    entries_on_disk(state, user)
        .map(|e| (e.id.0, e.path))
        .collect()
}

fn entries_on_disk(
    state: &std::sync::Arc<crate::FastcoreState>,
    user: &str,
) -> impl Iterator<Item = mailrs_maildir::Entry> {
    crate::imap::backend::list_mailboxes(state, user)
        .into_iter()
        .flat_map(|mb| {
            let md = mailrs_maildir::Maildir::open(&mb.path);
            md.scan_new()
                .unwrap_or_default()
                .into_iter()
                .chain(md.scan_cur().unwrap_or_default())
        })
}

/// The flag-free, folder-free identity a `blob_ref` names — the key
/// [`names_on_disk`] is built with.
pub(crate) fn base_id(blob_ref: &str) -> &str {
    let file = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    file.split(':').next().unwrap_or(file)
}

pub(crate) fn apply_flag_bitmask(user: &str, blob_ref: &str, bits: u32) -> std::io::Result<bool> {
    use mailrs_core_api::method::message::bitmask_to_maildir_flags;
    use mailrs_maildir::Flag;

    let Some((local, domain)) = user.split_once('@') else {
        return Ok(false);
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(root).join(domain).join(local);
    let Some((dir, id)) = mailrs_maildir::locate(&base, blob_ref) else {
        return Ok(false);
    };
    let Some(current) = dir.flags_of(&id)? else {
        return Ok(false);
    };

    let mut want = bitmask_to_maildir_flags(bits);
    // Every flag the mask has no bit for stays exactly as the file has it.
    for f in &current {
        if matches!(f, Flag::Passed) && !want.contains(f) {
            want.push(*f);
        }
    }
    want.sort();
    want.dedup();

    let mut now = current;
    now.sort();
    now.dedup();
    if now == want {
        return Ok(false);
    }
    dir.mark_processed(&id, &want)?;
    Ok(true)
}

/// Read the full References chain of a message from its maildir file
/// (the kevy wire only stores In-Reply-To). Returns [] when the blob_ref
/// is empty or the file is gone — the caller just gets fewer edges.
pub(crate) fn maildir_references(user: &str, blob_ref: &str) -> Option<Vec<String>> {
    // `None` means the file could not be opened; `Some(vec![])` means it was
    // read and names no ancestor. Returning an empty vec for both hid the
    // 2026-07-30 defect for as long as it lasted: every sent copy was
    // unreadable through a hand-built filename, and the caller could not tell
    // that from "this mail is not a reply".
    let bytes = read_maildir_file(user, blob_ref)?;
    let head = &bytes[..bytes.len().min(16 * 1024)];
    let (_, _, references, ..) = extract_headers(head);
    Some(references)
}

/// Minimal string-keyed union-find for the rethread backfill.
#[derive(Default)]
pub(crate) struct UnionFind {
    parent: std::collections::HashMap<String, String>,
}

impl UnionFind {
    pub(crate) fn find(&mut self, x: &str) -> String {
        let p = match self.parent.get(x) {
            Some(p) => p.clone(),
            None => {
                self.parent.insert(x.to_string(), x.to_string());
                return x.to_string();
            }
        };
        if p == x {
            return p;
        }
        let root = self.find(&p);
        self.parent.insert(x.to_string(), root.clone());
        root
    }

    pub(crate) fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

/// Every maildir file a user has, indexed by `Message-ID`.
///
/// One pass, 16 KB of each file. Used by the per-user message backfill to
/// answer "does this user have their own copy of this message, and under
/// what filename" — which is the question the shared message blob could
/// not answer, because it holds one owner's filename for all of them.
pub(crate) fn user_files_by_message_id(
    maildir_root: &str,
    user: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    let Some((local, domain)) = user.split_once('@') else {
        return out;
    };
    let base = std::path::PathBuf::from(maildir_root)
        .join(domain)
        .join(local);
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for sub in ["cur", "new"] {
        if let Ok(iter) = std::fs::read_dir(base.join(sub)) {
            for e in iter.flatten() {
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    files.push((String::new(), e.path()));
                }
            }
        }
    }
    if let Ok(iter) = std::fs::read_dir(&base) {
        for entry in iter.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') {
                continue;
            }
            for sub in ["cur", "new"] {
                if let Ok(iter) = std::fs::read_dir(entry.path().join(sub)) {
                    for e in iter.flatten() {
                        if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                            files.push((name.clone(), e.path()));
                        }
                    }
                }
            }
        }
    }
    for (subfolder, path) in files {
        let bare = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let blob_ref = match subfolder.is_empty() {
            true => bare,
            false => format!("{subfolder}/{bare}"),
        };
        // the headers are all that is read, not the attachments behind them
        let mut head = Vec::with_capacity(16 * 1024);
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        if std::io::Read::read_to_end(&mut std::io::Read::take(file, 16 * 1024), &mut head).is_err()
        {
            continue;
        }
        let (header_id, ..) = extract_headers(&head);
        let message_id = crate::file_message_id::message_id_or_file_id(header_id, &blob_ref);
        // Every file, not the first: one Message-ID can sit in a mailbox
        // several times (Sent and INBOX, or INBOX once per alias), and a
        // delete that knows one of them leaves the rest behind.
        out.entry(message_id).or_default().push(blob_ref);
    }
    for files in out.values_mut() {
        files.sort();
    }
    out
}

/// Every domain named by a `From:` header value, lowercased.
///
/// **All** of them, not the first or the last. The question this serves is
/// "does this header claim to be one of ours", and a header naming two
/// addresses claims both — picking one turns the answer into a guess about
/// which the reader's client will display. It also removes the choice that
/// was wrong twice in one sitting: taking the first `@` reads the domain
/// out of `"billing@paypal.com" <attacker@evil.example>`, and taking the
/// last one reads `other.com` out of `a@golia.jp, b@other.com`.
///
/// Reduction to an addr-spec goes through `mailrs_rfc5322`, which already
/// owns the name-addr rules eight other sites used to re-derive.
pub(crate) fn from_header_domains(from_line: &str) -> Vec<String> {
    let value = from_line
        .split_once(':')
        .map(|(_, v)| v)
        .unwrap_or(from_line);
    value
        .split(',')
        .filter_map(|mailbox| {
            let spec = mailrs_rfc5322::addr_spec(mailbox);
            let domain = spec.rsplit_once('@')?.1;
            let d = domain
                .trim()
                .trim_end_matches(['>', '.'])
                .to_ascii_lowercase();
            match d.is_empty() {
                true => None,
                false => Some(d),
            }
        })
        .collect()
}
