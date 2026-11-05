//! What the backup process tells the ops daemon: its start, a heartbeat
//! every minute with the last run as `last.json` records it, and each
//! run's result when it ends.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mailrs_ops_report::push::Reporter;
use mailrs_ops_report::wire::{BackupBeat, BackupResult, Detail};

use crate::Outcome;

const HEARTBEAT: Duration = Duration::from_secs(60);

pub fn start(target: &Path) -> Reporter {
    let r = Reporter::start("backup");
    r.ready(None);
    let target: PathBuf = target.to_path_buf();
    r.spawn_heartbeat(HEARTBEAT, move || Some(beat(&target)));
    r
}

fn beat(target: &Path) -> Detail {
    let last = crate::last_outcome(target);
    Detail::Backup(BackupBeat {
        last_run_at: last.as_ref().map(|o| o.finished),
        last_ok: last.as_ref().map(|o| o.ok),
        last_success_at: last.and_then(|o| o.last_success),
    })
}

pub fn result(r: &Reporter, o: &Outcome) {
    r.result(Detail::BackupResult(BackupResult {
        ok: o.ok,
        file: o.file.clone(),
        bytes: o.bytes,
        sha256: o.sha256.clone(),
        reason: o.reason.clone(),
    }));
}
