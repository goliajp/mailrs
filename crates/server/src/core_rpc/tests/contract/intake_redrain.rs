//! A spool file drained a second time, as after a process that stopped
//! between the store write and the file's removal, adds nothing on either
//! core: one copy on disk, one message, and the user's read state kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_core::spool::encode_spool_blob;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Diffs};
use super::intake::{REPORT, envelope};
use super::{ALICE, accounts};

const FILE: &str = "1790000000.M1P1.redrain";
const TID: &str = "q1@outside.example";

fn spool_file(root: &Path) -> PathBuf {
    root.join("incoming/new").join(FILE)
}

fn put(root: &Path) {
    std::fs::create_dir_all(root.join("incoming/new")).unwrap();
    std::fs::write(spool_file(root), encode_spool_blob(&envelope(None), REPORT)).unwrap();
}

/// Files under `dir`, at any depth, named for the spool file.
fn copies(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type().unwrap().is_dir() {
            true => copies(&e.path()),
            false => usize::from(e.file_name().to_string_lossy().starts_with(FILE)),
        })
        .sum()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_spool_file_drained_again_adds_nothing_on_either_core() {
    let cores = harness::start_on(
        "mem://redrain-fastcore-side-state",
        "mem://redrain-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let s = &cores.pg_core_state;
    let pg = Arc::new(crate::intake::PgCore {
        dir: crate::intake::PgDirectory::new(
            s.domain.clone(),
            s.maildir_root.clone(),
            s.net_url.clone(),
        ),
        mailbox: s.mailbox.clone(),
        event_bus: crate::event_bus::EventBus::new(64),
    });
    let (fc_spool, pg_spool) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let drain = |cores: &harness::Cores| {
        let state = cores.fastcore_state.clone();
        let dir = fc_spool.path().join("incoming/new");
        let maildir = state.maildir_root.clone();
        let fc = tokio::task::spawn_blocking(move || {
            mailrs_fastcore::spool_drain::drain_once(&dir, &maildir, &state).0
        });
        let (core, dirs) = (
            pg.clone(),
            mailrs_intake::spool::SpoolDirs::under(pg_spool.path()),
        );
        let pg = tokio::task::spawn_blocking(move || mailrs_intake::spool::tick(&dirs, &*core));
        (fc, pg)
    };

    for root in [fc_spool.path(), pg_spool.path()] {
        put(root);
    }
    let (fc, pgd) = drain(&cores);
    assert_eq!(fc.await.unwrap(), 1, "fastcore took nothing");
    assert_eq!(pgd.await.unwrap(), 1, "pg-core took nothing");
    for c in [&cores.fastcore, &cores.pg_core] {
        c.mark_thread_read(ALICE, TID).await.expect("read");
    }

    // the file is back, with the record an attempt that died leaves
    for root in [fc_spool.path(), pg_spool.path()] {
        put(root);
        match mailrs_intake::spool::attempts::begin(root, FILE) {
            mailrs_intake::spool::attempts::Begin::Go(_) => {}
            mailrs_intake::spool::attempts::Begin::Quarantine(_) => panic!("quarantined"),
        }
    }
    let (fc, pgd) = drain(&cores);
    assert_eq!(fc.await.unwrap(), 1, "fastcore left the file");
    assert_eq!(pgd.await.unwrap(), 1, "pg-core left the file");
    for root in [fc_spool.path(), pg_spool.path()] {
        assert!(!spool_file(root).exists());
        assert!(!root.join("attempts").join(FILE).exists());
    }

    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    let both = d.both(
        "list_conversations after the same spool file twice",
        cores.fastcore.list_conversations(ALICE, &req).await,
        cores.pg_core.list_conversations(ALICE, &req).await,
        &["id", "mailbox_id", "created_at", "modseq"],
    );
    let (fc_rows, pg_rows) = both.expect("both cores list");
    for (core, rows) in [("fastcore", fc_rows.items), ("pg-core", pg_rows.items)] {
        let row = rows
            .iter()
            .find(|r| r.thread_id == TID)
            .unwrap_or_else(|| panic!("{core}: not listed"));
        assert_eq!(row.message_count, 1, "{core}: counted twice");
        assert_eq!(row.unread_count, 0, "{core}: unread again");
    }
    for (core, root) in [
        ("fastcore", &cores.fastcore_state.maildir_root),
        ("pg-core", &cores.pg_core_state.maildir_root),
    ] {
        let alice = Path::new(root).join("example.test/alice");
        assert_eq!(copies(&alice), 1, "{core}: copies on disk");
    }
    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
