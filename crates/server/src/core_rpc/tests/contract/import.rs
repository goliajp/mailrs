//! A message brought over from another mailbox, filed by each core from
//! the same bytes, then read and archived the way the import route does:
//! where it threads, how it lists, and that nothing counts it twice.

use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

/// Ids, counters and file names each store mints for itself.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "uid",
    "modseq",
    "new_modseq",
    "affected",
    "created_at",
    "blob_ref",
];

const OLD: &[u8] = b"Date: Mon, 02 Jan 2023 09:00:00 +0000\r\n\
From: Erin <erin@old.test>\r\n\
To: Alice <alice@example.test>\r\n\
Subject: From the old mailbox\r\n\
Message-ID: <imported1@old.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Kept for years.\r\n";

const FILED: &[u8] = b"Date: Tue, 03 Jan 2023 09:00:00 +0000\r\n\
From: Frank <frank@old.test>\r\n\
To: Alice <alice@example.test>\r\n\
Subject: Receipt\r\n\
Message-ID: <imported2@old.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Thanks for your order.\r\n";

fn list(folder: &str) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some(folder.into()),
            limit: 50,
            ..Default::default()
        },
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    for (name, raw) in [("unread", OLD), ("to archive", FILED)] {
        let filed = d.both(
            &format!("import_message {name}"),
            fc.import_message(ALICE, raw).await,
            pg.import_message(ALICE, raw).await,
            &["blob_ref"],
        );
        if let Some((a, _)) = &filed {
            assert!(!a.blob_ref.contains('/'), "not in INBOX: {}", a.blob_ref);
        }
    }

    // read, then archived: what the web import does after filing
    for core in [fc, pg] {
        let m = core
            .find_by_message_id_for_user(ALICE, "imported2@old.test")
            .await
            .expect("the imported message is found by its Message-ID");
        core.set_message_flags(ALICE, m.uid, &SetMessageFlagsRequest { flags: FLAG_SEEN })
            .await
            .expect("flags");
        core.archive_thread(ALICE, &m.thread_id)
            .await
            .expect("archive");
    }

    for mid in ["imported1@old.test", "imported2@old.test"] {
        d.both(
            &format!("find_by_message_id {mid} after import"),
            fc.find_by_message_id_for_user(ALICE, mid).await,
            pg.find_by_message_id_for_user(ALICE, mid).await,
            MINTED,
        );
    }
    for folder in ["Inbox", "all"] {
        let rows = d.both(
            &format!("list_conversations [{folder}] after import"),
            fc.list_conversations(ALICE, &list(folder)).await,
            pg.list_conversations(ALICE, &list(folder)).await,
            MINTED,
        );
        if let Some((rows, _)) = rows {
            let has = |tid: &str| rows.items.iter().any(|r| r.thread_id == tid);
            assert!(has("imported1@old.test"), "[{folder}] lost the import");
            assert_eq!(
                has("imported2@old.test"),
                folder == "all",
                "[{folder}] the archived import"
            );
        }
    }
    d.both(
        "unseen_count after import",
        fc.unseen_count(ALICE).await,
        pg.unseen_count(ALICE).await,
        &[],
    );
}
