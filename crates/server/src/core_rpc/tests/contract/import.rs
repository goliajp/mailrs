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

const SUSPECT: &[u8] = b"Date: Wed, 04 Jan 2023 09:00:00 +0000\r\n\
From: Bank <security@bank.test>\r\n\
To: Alice <alice@example.test>\r\n\
Subject: Verify your account\r\n\
Message-ID: <imported3@old.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Click here.\r\n";

/// What the importer hands the core when the rules hold a message, as the
/// receiver hands one to a delivery.
const HOLDS: &str = r#"{"rules_version":"contract","score":9.0,"quarantined":true,"layers":[]}"#;

const FILED: &[u8] = b"Date: Tue, 03 Jan 2023 09:00:00 +0000\r\n\
From: Frank <frank@old.test>\r\n\
To: Alice <alice@example.test>\r\n\
Subject: Receipt\r\n\
Message-ID: <imported2@old.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Thanks for your order.\r\n";

/// Alice's own mail, from an mbox without a Sent label: hers all the same.
const OWN: &[u8] = b"Date: Thu, 05 Jan 2023 09:00:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Carol <carol@old.test>\r\n\
Subject: Note\r\n\
Message-ID: <imported-own@old.test>\r\n\
\r\n\
Mine.\r\n";

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
            fc.import_message(ALICE, raw, None).await,
            pg.import_message(ALICE, raw, None).await,
            &["blob_ref"],
        );
        if let Some((a, _)) = &filed {
            assert!(!a.blob_ref.contains('/'), "not in INBOX: {}", a.blob_ref);
        }
    }

    let own = d.both(
        "import_message from the user",
        fc.import_message(ALICE, OWN, None).await,
        pg.import_message(ALICE, OWN, None).await,
        &["blob_ref"],
    );
    if let Some((a, _)) = &own {
        assert!(a.blob_ref.starts_with(".Sent/"), "{}", a.blob_ref);
    }
    for folder in ["Inbox", "Sent"] {
        if let Some((rows, _)) = d.both(
            &format!("list_conversations [{folder}] after importing the user's own mail"),
            fc.list_conversations(ALICE, &list(folder)).await,
            pg.list_conversations(ALICE, &list(folder)).await,
            MINTED,
        ) {
            let listed = rows
                .items
                .iter()
                .any(|r| r.thread_id == "imported-own@old.test");
            assert_eq!(listed, folder == "Sent", "[{folder}]");
        }
    }

    let held = d.both(
        "import_message held as fraud",
        fc.import_message(ALICE, SUSPECT, Some(HOLDS.into())).await,
        pg.import_message(ALICE, SUSPECT, Some(HOLDS.into())).await,
        &["blob_ref"],
    );
    let held_tid = held.map(|(a, _)| a.thread_id).unwrap_or_default();

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
            assert!(!has(&held_tid), "[{folder}] lists the held import");
        }
    }
    // starred and pinned while held: the flag lists leave it out unless
    // asked for, in both cores
    for core in [fc, pg] {
        core.star_thread(ALICE, &held_tid).await.expect("star");
        core.pin_thread(ALICE, &held_tid).await.expect("pin");
    }
    for (name, include_quarantined) in [("held left out", false), ("held included", true)] {
        let req = |starred, pinned| ListConversationsRequest {
            filter: ConversationFilter {
                starred,
                pinned,
                include_quarantined,
                limit: 50,
                ..Default::default()
            },
        };
        for (view, r) in [
            ("starred", req(Some(true), None)),
            ("pinned", req(None, Some(true))),
        ] {
            if let Some((rows, _)) = d.both(
                &format!("list_conversations [{view}] {name} with a held conversation"),
                fc.list_conversations(ALICE, &r).await,
                pg.list_conversations(ALICE, &r).await,
                MINTED,
            ) {
                let listed = rows.items.iter().any(|row| row.thread_id == held_tid);
                assert_eq!(listed, include_quarantined, "[{view}] {name}");
            }
        }
    }
    let with_held = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("all".into()),
            include_quarantined: true,
            limit: 50,
            ..Default::default()
        },
    };
    let rows = d.both(
        "list_conversations [all, held included] after import",
        fc.list_conversations(ALICE, &with_held).await,
        pg.list_conversations(ALICE, &with_held).await,
        MINTED,
    );
    if let Some((rows, _)) = rows {
        let row = rows
            .items
            .iter()
            .find(|r| r.thread_id == held_tid)
            .expect("the held import is there when held mail is asked for");
        assert!(row.quarantined, "the import was not held");
    }
    d.both(
        "unseen_count after import",
        fc.unseen_count(ALICE).await,
        pg.unseen_count(ALICE).await,
        &[],
    );
}
