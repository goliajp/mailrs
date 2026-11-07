//! The upload store: files a person uploaded and has not sent yet, for a
//! JMAP client's message or for a draft.
//!
//! Under `MAILRS_JMAP_UPLOAD_DIR` (`/data/jmap-uploads`), one directory per
//! account, each file named by its SHA-256: the same bytes uploaded twice
//! by one account are one file, and accounts never share. Each account may
//! keep [`ACCOUNT_LIMIT`] bytes. A file untouched for [`KEEP_FOR`] that no
//! draft of its account names is deleted by [`sweep::sweep`]. Every process
//! that reads or writes drafts reads this same directory.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

pub mod sweep;

/// What one account may keep uploaded.
pub const ACCOUNT_LIMIT: u64 = 1024 * 1024 * 1024;

/// How long a file nothing names is kept after its last use. RFC 8620
/// §6.1 asks JMAP servers for an hour at least.
pub const KEEP_FOR: Duration = Duration::from_secs(24 * 3600);

/// The most a message's attachments may come to, and so the most a draft's
/// files may: a draft that can be saved can be sent. Also the largest form
/// the web tier takes.
pub const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

pub fn root() -> PathBuf {
    std::env::var("MAILRS_JMAP_UPLOAD_DIR")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .map_or_else(|| PathBuf::from("/data/jmap-uploads"), PathBuf::from)
}

/// An account's directory: a hash of the address, so no address is a path.
pub fn account_dir(root: &Path, user: &str) -> PathBuf {
    let h = Sha256::digest(user.trim().to_ascii_lowercase().as_bytes());
    root.join(hex(&h[..16]))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The id an upload is named by on the wire: `U` and its SHA-256.
pub fn blob_id(sha: &str) -> String {
    format!("U{sha}")
}

/// The SHA-256 an upload id names, lowercased; `None` for anything else.
pub fn sha_of(blob_id: &str) -> Option<String> {
    let sha = blob_id.strip_prefix('U')?;
    (sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| sha.to_ascii_lowercase())
}

/// Where the upload `blob_id` of `user` is, if the id is an upload's.
pub fn path_of(root: &Path, user: &str, blob_id: &str) -> Option<PathBuf> {
    Some(account_dir(root, user).join(sha_of(blob_id)?))
}

/// Mark the file used at `now`, which keeps it another [`KEEP_FOR`].
pub fn touch(path: &Path, now: SystemTime) -> io::Result<()> {
    std::fs::File::options()
        .append(true)
        .open(path)?
        .set_modified(now)
}

fn used(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// What [`put`] did with the bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum Put {
    /// Stored, or already there and used again; the SHA-256.
    Stored(String),
    /// The account's uploads would pass [`ACCOUNT_LIMIT`].
    Full,
}

/// Store `bytes` for `user`.
pub fn put(root: &Path, user: &str, bytes: &[u8], now: SystemTime) -> io::Result<Put> {
    let sha = hex(&Sha256::digest(bytes));
    let dir = account_dir(root, user);
    if !dir.join(&sha).exists() && used(&dir) + bytes.len() as u64 > ACCOUNT_LIMIT {
        return Ok(Put::Full);
    }
    store(&dir, sha, bytes, now).map(Put::Stored)
}

/// Store `bytes` for `user` past [`ACCOUNT_LIMIT`], for an account restore,
/// which must not fail. Returns the SHA-256.
pub fn keep(root: &Path, user: &str, bytes: &[u8], now: SystemTime) -> io::Result<String> {
    let sha = hex(&Sha256::digest(bytes));
    store(&account_dir(root, user), sha, bytes, now)
}

fn store(dir: &Path, sha: String, bytes: &[u8], now: SystemTime) -> io::Result<String> {
    let path = dir.join(&sha);
    std::fs::create_dir_all(dir)?;
    if path.exists() {
        touch(&path, now)?;
        return Ok(sha);
    }
    let tmp = dir.join(format!(".{sha}.part"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(sha)
}

/// Total bytes held, for /api/health.
pub fn total_bytes(root: &Path) -> u64 {
    std::fs::read_dir(root)
        .map(|it| it.filter_map(Result::ok).map(|d| used(&d.path())).sum())
        .unwrap_or(0)
}

/// Give `to` the upload `sha` of `from`: a hard link when the two
/// directories share a volume, a copy when linking fails. Either way `to`
/// ends with a file of its own, so `from` deleting theirs takes nothing
/// from `to`. `link` is `std::fs::hard_link` outside tests.
pub fn give(
    root: &Path,
    from: &str,
    to: &str,
    sha: &str,
    now: SystemTime,
    link: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let src = account_dir(root, from).join(sha);
    let dir = account_dir(root, to);
    let dst = dir.join(sha);
    if dst.exists() {
        return touch(&dst, now);
    }
    std::fs::create_dir_all(&dir)?;
    if link(&src, &dst).is_err() {
        let tmp = dir.join(format!(".{sha}.part"));
        std::fs::copy(&src, &tmp)?;
        std::fs::rename(&tmp, &dst)?;
    }
    touch(&dst, now)
}

/// The account purge: remove `user`'s directory. Returns how many files it
/// held; `dry_run` only counts.
pub fn purge_account(root: &Path, user: &str, dry_run: bool) -> io::Result<u64> {
    let dir = account_dir(root, user);
    let files = match std::fs::read_dir(&dir) {
        Ok(it) => it.filter_map(Result::ok).count() as u64,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    if !dry_run {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
