//! Deleting a conversation leaves no copy of it on disk, on either core.
//!
//! Mail you send yourself is filed twice under one Message-ID: the Sent
//! copy and the one that comes back to the inbox. The fastcore row names
//! one of them and its delete used to unlink only that; the SQL core lists
//! both and used to unlink neither. A copy left on disk is mail the user
//! deleted and still has, and self-heal files it again.

use super::harness::{self, Diffs};
use super::{ALICE, accounts, intake};

const SELF: &[u8] = b"Date: Tue, 14 Nov 2023 22:40:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Alice <alice@example.test>\r\n\
Subject: Note to self\r\n\
Message-ID: <self1@example.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Remember the keys.\r\n";

/// Files under `root` for alice that carry the note's Message-ID.
fn copies(root: &str) -> usize {
    fn walk(dir: &std::path::Path, n: &mut usize) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, n);
            } else if std::fs::read(&p)
                .map(|b| b.windows(20).any(|w| w == b"<self1@example.test>"))
                .unwrap_or(false)
            {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(
        &std::path::Path::new(root)
            .join("example.test")
            .join("alice"),
        &mut n,
    );
    n
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_deleted_conversation_leaves_no_copy_on_either_core() {
    let cores = harness::start_on(
        "mem://delete-fastcore-side-state",
        "mem://delete-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let (fc, pg) = (&cores.fastcore, &cores.pg_core);
    let (fc_root, pg_root) = (
        cores.fastcore_state.maildir_root.clone(),
        cores.pg_core_state.maildir_root.clone(),
    );

    let rcpts = vec![ALICE.to_string()];
    d.both(
        "file_sent note to self",
        fc.file_sent(ALICE, SELF, &rcpts).await,
        pg.file_sent(ALICE, SELF, &rcpts).await,
        &["blob_ref"],
    );
    intake::Spools::new(&cores)
        .arrive(&cores, &intake::envelope(None), SELF)
        .await;
    assert_eq!(
        (copies(&fc_root), copies(&pg_root)),
        (2, 2),
        "the fixture needs the Sent copy and the inbox copy on both cores"
    );

    d.both(
        "delete_thread note to self",
        fc.delete_thread(ALICE, "self1@example.test").await,
        pg.delete_thread(ALICE, "self1@example.test").await,
        &[],
    );
    d.same(
        "copies on disk after delete_thread",
        &copies(&fc_root),
        &copies(&pg_root),
        &[],
    );
    assert_eq!(copies(&fc_root), 0, "fastcore left a copy on disk");
    assert_eq!(copies(&pg_root), 0, "pg-core left a copy on disk");

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
