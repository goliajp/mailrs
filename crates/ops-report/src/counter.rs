//! The start counter, instance id and liveness file in a process's own
//! state directory (`MAILRS_OPS_STATE_DIR`).
//!
//! The counter is raised and fsynced before the start report goes out, so a
//! process that dies before reporting still leaves its start counted: the
//! next report's count shows the daemon how many starts it never saw.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const STARTS: &str = "starts";
const INSTANCE: &str = "instance";
const ALIVE: &str = "alive";

/// What one start found in the state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub instance: String,
    /// `None` when the directory could not be written.
    pub start_count: Option<u64>,
}

/// Raise the start counter in `dir` and read the instance id, making both
/// on first use. With no directory, or one that cannot be written, the
/// instance id is made up for this start and the count is unknown.
pub fn start(dir: Option<&Path>) -> Started {
    let Some(dir) = dir else {
        return Started {
            instance: fresh_id(),
            start_count: None,
        };
    };
    let instance = instance(dir).unwrap_or_else(|_| fresh_id());
    let start_count = bump(dir).ok();
    Started {
        instance,
        start_count,
    }
}

fn instance(dir: &Path) -> std::io::Result<String> {
    let path = dir.join(INSTANCE);
    match std::fs::read_to_string(&path) {
        Ok(id) if !id.trim().is_empty() => Ok(id.trim().to_string()),
        _ => {
            std::fs::create_dir_all(dir)?;
            let id = fresh_id();
            write_synced(dir, &path, id.as_bytes())?;
            Ok(id)
        }
    }
}

fn bump(dir: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(STARTS);
    // a file that cannot be parsed counts as never started: the daemon sees
    // the count go down and records a reset, not a restart
    let n = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    write_synced(dir, &path, n.to_string().as_bytes())?;
    Ok(n)
}

/// Write `bytes` to `path` through a temporary file, fsyncing the file and
/// the directory, so a crash leaves the old value or the new one.
fn write_synced(dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp: PathBuf = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

fn fresh_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let host = std::env::var("HOSTNAME").unwrap_or_default();
    let seed = format!("{}-{nanos}-{host}", std::process::id());
    hex::encode(&Sha256::digest(seed.as_bytes())[..8])
}

/// Record that the process's main loop is alive, for the container
/// healthcheck (`mailrs-ops check-alive`).
pub fn touch_alive(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_synced(
        dir,
        &dir.join(ALIVE),
        crate::build::now().to_string().as_bytes(),
    )
}

/// Seconds since the last [`touch_alive`] in `dir`; `None` when there has
/// been none or the file cannot be read.
pub fn alive_age(dir: &Path, now: i64) -> Option<i64> {
    let at: i64 = std::fs::read_to_string(dir.join(ALIVE))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(now - at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_start_raises_the_count_and_keeps_the_instance() {
        let dir = tempfile::tempdir().unwrap();
        let a = start(Some(dir.path()));
        let b = start(Some(dir.path()));
        assert_eq!(a.start_count, Some(1));
        assert_eq!(b.start_count, Some(2));
        assert_eq!(a.instance, b.instance);
        assert_eq!(a.instance.len(), 16);
    }

    #[test]
    fn a_cleared_directory_starts_counting_again() {
        let dir = tempfile::tempdir().unwrap();
        start(Some(dir.path()));
        start(Some(dir.path()));
        std::fs::remove_file(dir.path().join(STARTS)).unwrap();
        assert_eq!(start(Some(dir.path())).start_count, Some(1));
    }

    #[test]
    fn no_directory_means_an_unknown_count() {
        let s = start(None);
        assert_eq!(s.start_count, None);
        assert_eq!(s.instance.len(), 16);
    }

    #[test]
    fn an_unwritable_directory_means_an_unknown_count() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(start(Some(&file)).start_count, None);
    }

    #[test]
    fn the_alive_file_reports_its_age() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(alive_age(dir.path(), 100), None);
        touch_alive(dir.path()).unwrap();
        let age = alive_age(dir.path(), crate::build::now() + 5).unwrap();
        assert!((4..=6).contains(&age), "{age}");
    }
}
