//! The maildir both cores deliver into, and the address form every lookup
//! expects.

use std::path::PathBuf;

/// Deliver one file to one recipient. `subfolder` is empty for INBOX
/// or a `.Maildir++Folder` for a sieve fileinto action. Returns `Ok(true)` on success, `Ok(false)` when the
/// target directory is absent.
pub fn deliver(
    maildir_root: &str,
    addr: &str,
    subfolder: &str,
    filename: &str,
    body: &[u8],
) -> std::io::Result<bool> {
    let (local, domain) = match addr.split_once('@') {
        Some(x) => x,
        None => return Ok(false),
    };
    let base = PathBuf::from(maildir_root).join(domain).join(local);
    let user_new = if subfolder.is_empty() {
        base.join("new")
    } else {
        // Auto-create the Maildir++ subfolder skeleton on first fileinto,
        // matching what an IMAP client would do via CREATE — otherwise a
        // freshly-provisioned account can't receive filed mail.
        let sub = base.join(subfolder);
        for leaf in ["cur", "new", "tmp"] {
            let _ = std::fs::create_dir_all(sub.join(leaf));
        }
        sub.join("new")
    };
    if !user_new.is_dir() {
        return Ok(false);
    }
    let target = user_new.join(filename);
    std::fs::write(&target, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));
    }
    Ok(true)
}

/// Make sure `addr` has a maildir: an account gets its skeleton on first
/// delivery, because adding an account never made one and a fresh account's
/// first mail would otherwise sit in the spool for good.
pub fn ensure_maildir(maildir_root: &str, addr: &str) -> bool {
    if has_maildir(maildir_root, addr) {
        return true;
    }
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    let base = PathBuf::from(maildir_root).join(domain).join(local);
    for leaf in ["cur", "new", "tmp"] {
        if std::fs::create_dir_all(base.join(leaf)).is_err() {
            return false;
        }
    }
    tracing::info!(%addr, "provisioned maildir for account on first delivery");
    true
}

/// The form of a recipient address that every lookup here expects.
///
/// Maildir paths, account keys and alias keys are all written lowercase, so
/// a recipient has to be folded once before any of them is consulted.
/// RFC 5321 §2.4 makes the domain case-insensitive outright and urges hosts
/// to treat the mailbox the same way; every writer in this tree already does.
///
/// ASCII folding, not `to_lowercase`: addresses are ASCII or punycode
/// here, and Unicode folding has locale-shaped surprises (the Turkish
/// dotless i) that would map two distinct mailboxes onto one key.
pub fn canonical_recipient(addr: &str) -> String {
    addr.to_ascii_lowercase()
}

/// Whether `addr` has a maildir on disk: its `new/` dir exists. False for
/// malformed addresses.
pub fn has_maildir(maildir_root: &str, addr: &str) -> bool {
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    PathBuf::from(maildir_root)
        .join(domain)
        .join(local)
        .join("new")
        .is_dir()
}

/// Turn a Maildir++ folder name from `fileinto` into a safe
/// disk-path segment. Dot-prefixed, and any embedded `/` is replaced
/// with `.` per the Maildir++ hierarchy convention.
pub fn maildir_subfolder(folder: &str) -> String {
    let cleaned = folder.trim().replace('/', ".").replace(['\\', '\0'], "");
    if cleaned.starts_with('.') {
        cleaned
    } else {
        format!(".{cleaned}")
    }
}
