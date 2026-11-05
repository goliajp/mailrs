//! What the ops daemon reads from a core: the build in the health answer,
//! and `/v1/ops/status` with the drain's progress and the envelope facts
//! of quarantined spool files.

use mailrs_intake::spool::attempts::Record;
use mailrs_intake::spool::quarantine::Quarantined;

use super::harness::{Cores, Diffs};

/// A quarantined file and its record, as the drain leaves them.
fn quarantine(maildir_root: &str, file: &str, at: i64) {
    let dir = std::path::Path::new(&mailrs_core::spool::spool_root(maildir_root)).join("quarantine");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(file), b"Subject: kept out of the status\r\n\r\nbody").unwrap();
    let q = Quarantined {
        file: file.to_string(),
        quarantined_at: at,
        reverse_path: "sender@remote.test".into(),
        forward_paths: vec!["alice@example.test".into()],
        size: 42,
        record: Record {
            attempts: 3,
            first_at: at - 60,
            last_at: at,
            stage: "deliver".into(),
            done: vec![],
        },
    };
    std::fs::write(
        dir.join(format!("{file}.json")),
        serde_json::to_vec(&q).unwrap(),
    )
    .unwrap();
}

pub(super) async fn run(cores: &Cores, d: &mut Diffs) {
    for root in [
        &cores.fastcore_state.maildir_root,
        &cores.pg_core_state.maildir_root,
    ] {
        quarantine(root, "1700000000.M1P1.a", 1_700_000_000);
        quarantine(root, "1700000100.M1P1.b", 1_700_000_100);
    }
    let answers = d.both(
        "ops_status",
        cores.fastcore.ops_status().await,
        cores.pg_core.ops_status().await,
        &[],
    );
    if let Some((fc, _)) = answers {
        let files: Vec<_> = fc.quarantined.iter().map(|q| q.file.as_str()).collect();
        if fc.quarantined_total != 2 || files != ["1700000100.M1P1.b", "1700000000.M1P1.a"] {
            d.0.push(format!(
                "ops_status: expected two records, newest first; got {} {files:?}",
                fc.quarantined_total
            ));
        }
    }
    let builds = d.both(
        "healthz build",
        cores.fastcore.healthz().await.map(|h| h.build),
        cores.pg_core.healthz().await.map(|h| h.build),
        &[],
    );
    if let Some((None, _)) = builds {
        d.0.push("healthz: no build reported".into());
    }
}
