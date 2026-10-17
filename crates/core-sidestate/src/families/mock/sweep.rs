use std::path::Path;
use std::time::Duration;

use kevy_client::Connection;

use super::*;

/// What one sweep changed. All zero is the resting state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    pub expired: usize,
    pub orphan_files: usize,
    pub bytes_corrected: bool,
}

impl SweepReport {
    pub fn changed_anything(&self) -> bool {
        self.expired > 0 || self.orphan_files > 0 || self.bytes_corrected
    }
}

// a file older than this with no listing is left over from a write whose
// zset insert failed
const ORPHAN_AGE: Duration = Duration::from_secs(RETENTION.as_secs() + 3600);
const BATCH: i64 = 100;

/// Remove expired messages, files nothing lists, and correct the byte
/// total from the files themselves. Reads only, when there is nothing to do.
pub fn sweep(conn: &mut Connection, root: &Path, now_ms: i64) -> MockResult<SweepReport> {
    let mut report = SweepReport::default();
    let cutoff = now_ms - RETENTION.as_millis() as i64;
    let names: Vec<String> = conn.smembers(NS_INDEX)?.into_iter().map(utf8).collect();
    for ns in &names {
        report.expired += expire_namespace(conn, root, ns, cutoff)?;
    }
    let mut bytes = 0i64;
    if let Ok(dirs) = std::fs::read_dir(root) {
        for dir in dirs.flatten() {
            let name = dir.file_name().to_string_lossy().into_owned();
            if !names.contains(&name) {
                if std::fs::remove_dir_all(dir.path()).is_ok() {
                    report.orphan_files += 1;
                }
                continue;
            }
            bytes += sweep_dir(conn, &dir.path(), &name, &mut report)?;
        }
    }
    if total_bytes(conn)? != bytes {
        conn.set(BYTES, bytes.to_string().as_bytes())?;
        report.bytes_corrected = true;
    }
    Ok(report)
}

fn expire_namespace(
    conn: &mut Connection,
    root: &Path,
    ns: &str,
    cutoff: i64,
) -> MockResult<usize> {
    let mut expired = 0;
    loop {
        let oldest = conn.zrange(msgs_key(ns).as_bytes(), 0, BATCH - 1)?;
        let mut stopped = oldest.len() < BATCH as usize;
        for id in &oldest {
            let score = conn.zscore(msgs_key(ns).as_bytes(), id)?.unwrap_or(0.0);
            if score as i64 >= cutoff {
                stopped = true;
                break;
            }
            remove_message(conn, root, ns, &utf8(id.clone()))?;
            expired += 1;
        }
        if stopped || oldest.is_empty() {
            return Ok(expired);
        }
    }
}

// bytes of the files kept in one namespace directory
fn sweep_dir(
    conn: &mut Connection,
    dir: &Path,
    ns: &str,
    report: &mut SweepReport,
) -> MockResult<i64> {
    let mut bytes = 0;
    let Ok(files) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    for file in files.flatten() {
        let Ok(md) = file.metadata() else { continue };
        let old = md
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > ORPHAN_AGE);
        let path = file.path();
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if old
            && conn
                .zscore(msgs_key(ns).as_bytes(), id.as_bytes())?
                .is_none()
        {
            if std::fs::remove_file(&path).is_ok() {
                report.orphan_files += 1;
            }
            continue;
        }
        bytes += md.len() as i64;
    }
    Ok(bytes)
}
