//! Deleting uploads nothing needs any more.
//!
//! One rule: a file untouched for [`KEEP_FOR`] that no draft of its own
//! account names is deleted. A draft has no expiry, so a file it names is
//! kept as long as the draft is; one whose draft was sent, deleted or taken
//! over goes within two days. Sending or deleting a draft does not delete
//! its files on the spot: the same bytes may be in another draft, or in a
//! JMAP message being made.
//!
//! The same walk keeps the pasted images drafts name (`inline_images`).
//!
//! A pass with nothing to do lists the store and scans the drafts once;
//! the loop running it backs off from [`BASE_INTERVAL`] to
//! [`MAX_INTERVAL`] while that is so ([`next_interval`]).

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use kevy_client::Connection;
use mailrs_core_api::method::admin::DraftWire;

use super::{KEEP_FOR, account_dir, sha_of};
use crate::families::{inline_images, prefs};

pub const BASE_INTERVAL: Duration = Duration::from_secs(3600);
pub const MAX_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// What one pass found and did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    /// Untouched for [`KEEP_FOR`] and named by no draft of the account:
    /// what a pass deletes. Zero after a pass that was not a dry run.
    pub expired_unreferenced: usize,
    /// Untouched for [`KEEP_FOR`] but named by a draft: kept.
    pub expired_referenced: usize,
    pub deleted: usize,
    /// Pasted images a draft names whose expiry was pushed back.
    pub images_kept: usize,
}

impl Pass {
    /// Whether the pass changed anything, which resets the backoff.
    pub fn did_work(&self) -> bool {
        self.deleted > 0 || self.images_kept > 0
    }
}

/// The wait before the next pass: back to [`BASE_INTERVAL`] after one that
/// changed something, otherwise twice the last wait, up to
/// [`MAX_INTERVAL`].
pub fn next_interval(last: Duration, pass: &Pass) -> Duration {
    match pass.did_work() {
        true => BASE_INTERVAL,
        false => (last * 2).clamp(BASE_INTERVAL, MAX_INTERVAL),
    }
}

/// What the drafts name: upload files by path, pasted images by id.
#[derive(Default)]
struct Named {
    files: HashSet<PathBuf>,
    images: HashSet<String>,
}

fn named(conn: &mut Connection, root: &Path) -> io::Result<Named> {
    let pattern = prefs::drafts_key("*");
    let mut out = Named::default();
    let mut cursor = 0u64;
    loop {
        let (next, keys) = conn.scan(cursor, Some(pattern.as_bytes()), Some(500))?;
        for key in keys {
            let key = String::from_utf8_lossy(&key).into_owned();
            let Some(user) = key.strip_prefix(&prefs::drafts_key("")) else {
                continue;
            };
            // `drafts:{user}:counter`, the id counter beside the hash
            if user.ends_with(":counter") {
                continue;
            }
            let dir = account_dir(root, user);
            for d in prefs::hgetall_values::<DraftWire>(conn, &key) {
                for a in &d.attachments {
                    if let Some(sha) = sha_of(&a.blob_id) {
                        out.files.insert(dir.join(sha));
                    }
                }
                for id in inline_images::ids_in(&d.body) {
                    out.images.insert(id.to_string());
                }
            }
        }
        if next == 0 {
            return Ok(out);
        }
        cursor = next;
    }
}

fn expired(path: &Path, now: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| now.duration_since(t).ok())
        .is_some_and(|age| age >= KEEP_FOR)
}

/// Every file untouched for [`KEEP_FOR`], with the directory it is in.
fn expired_files(root: &Path, now: SystemTime) -> Vec<PathBuf> {
    let Ok(accounts) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for account in accounts.filter_map(Result::ok) {
        let Ok(files) = std::fs::read_dir(account.path()) else {
            continue;
        };
        out.extend(
            files
                .filter_map(Result::ok)
                .map(|f| f.path())
                .filter(|p| expired(p, now)),
        );
    }
    out
}

/// One pass. With `dry_run` it only counts, for /api/health.
pub fn sweep(
    conn: &mut Connection,
    root: &Path,
    now: SystemTime,
    dry_run: bool,
) -> io::Result<Pass> {
    let old = expired_files(root, now);
    let named = named(conn, root)?;
    let mut pass = Pass::default();
    for path in old {
        if named.files.contains(&path) {
            pass.expired_referenced += 1;
            continue;
        }
        if dry_run {
            pass.expired_unreferenced += 1;
            continue;
        }
        // a save may have touched it since it was listed
        if expired(&path, now) && std::fs::remove_file(&path).is_ok() {
            pass.deleted += 1;
            if let Some(dir) = path.parent() {
                // only goes when empty
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
    if !dry_run {
        for id in &named.images {
            pass.images_kept += usize::from(inline_images::keep(conn, id)?);
        }
    }
    Ok(pass)
}
