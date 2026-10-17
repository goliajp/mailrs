//! A spool file nobody can take is set aside after a week.
//!
//! It stays in the spool at first because the fix is usually on our side
//! (an account or alias added late). After `STALE` it moves to
//! `<spool>/unresolved/`, beside `incoming/`, so the drain stops rereading
//! it every tick; the file is kept for a human, never bounced.

use std::path::Path;
use std::time::{Duration, SystemTime};

pub(super) const STALE: Duration = Duration::from_secs(7 * 24 * 3600);

/// `dir` is the spool directory being drained (`<spool>/incoming/new` or
/// `cur`). True when the file was moved out of it.
pub(super) fn set_aside_if_stale(dir: &Path, path: &Path, filename: &str) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    let stale = SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= STALE);
    if !stale {
        return false;
    }
    let Some(spool_root) = dir.parent().and_then(Path::parent) else {
        return false;
    };
    let unresolved = spool_root.join("unresolved");
    let moved = std::fs::create_dir_all(&unresolved)
        .and_then(|()| std::fs::rename(path, unresolved.join(filename)));
    match moved {
        Ok(()) => {
            tracing::info!(
                file = %filename,
                to = %unresolved.display(),
                "spool file unresolved for a week; set aside"
            );
            true
        }
        Err(e) => {
            tracing::warn!(file = %filename, error = %e, "setting aside a stale spool file failed");
            false
        }
    }
}
