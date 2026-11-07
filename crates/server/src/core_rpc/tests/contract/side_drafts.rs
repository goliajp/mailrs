//! Drafts with files, on both cores: a save that does not name the files
//! keeps them and the first save's time, `[]` clears them, and a file that
//! is not the account's upload is refused by both alike.

use mailrs_core_api::method::admin::{DraftAttachment, SaveDraftRequest};
use mailrs_core_sidestate::families::uploads;

use super::ALICE;
use super::harness::{Cores, Diffs};

const MINTED: &[&str] = &["id", "created_at", "updated_at"];

/// The upload store both cores read in this test: one directory, as in
/// prod, where every process mounts the same volume.
pub(super) fn uploads_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("mailrs-contract-uploads");
    // SAFETY: always the same value, set before either core reads it
    unsafe { std::env::set_var("MAILRS_JMAP_UPLOAD_DIR", &root) };
    root
}

pub(super) async fn drafts_with_files(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let root = uploads_root();
    let uploads::Put::Stored(sha) =
        uploads::put(&root, ALICE, b"the agenda", std::time::SystemTime::now()).unwrap()
    else {
        panic!("upload store full");
    };
    let file = DraftAttachment {
        blob_id: uploads::blob_id(&sha),
        name: "agenda.txt".into(),
        content_type: "text/plain".into(),
        size: 0,
    };
    let first = SaveDraftRequest {
        subject: "With a file".into(),
        attachments: Some(vec![file.clone()]),
        ..Default::default()
    };
    let Some((a, b)) = d.both(
        "save_draft with a file",
        fc.save_draft(ALICE, &first).await,
        pg.save_draft(ALICE, &first).await,
        MINTED,
    ) else {
        return;
    };
    for (core, client, id) in [("fastcore", fc, a.id), ("pg-core", pg, b.id)] {
        let later = SaveDraftRequest {
            id: Some(id),
            subject: "Edited by an older client".into(),
            ..Default::default()
        };
        client.save_draft(ALICE, &later).await.unwrap();
        let list = client.list_drafts(ALICE).await.unwrap();
        let draft = list.items.iter().find(|x| x.id == id).unwrap();
        assert_eq!(draft.subject, "Edited by an older client", "{core}");
        assert_eq!(draft.attachments.len(), 1, "{core}: kept");
        assert_eq!(draft.attachments[0].size, 10, "{core}: sized from the file");
    }
    d.both(
        "list_drafts with a file",
        fc.list_drafts(ALICE).await,
        pg.list_drafts(ALICE).await,
        MINTED,
    );
    let foreign = SaveDraftRequest {
        attachments: Some(vec![DraftAttachment {
            blob_id: uploads::blob_id(&"0".repeat(64)),
            ..file
        }]),
        ..Default::default()
    };
    let (fe, pe) = (
        fc.save_draft(ALICE, &foreign).await,
        pg.save_draft(ALICE, &foreign).await,
    );
    assert!(
        fe.is_err() && pe.is_err(),
        "a file that is not an upload is refused"
    );
    d.both("save_draft naming no upload", fe, pe, &[]);
    for (client, id) in [(fc, a.id), (pg, b.id)] {
        let clear = SaveDraftRequest {
            id: Some(id),
            attachments: Some(Vec::new()),
            ..Default::default()
        };
        client.save_draft(ALICE, &clear).await.unwrap();
        let list = client.list_drafts(ALICE).await.unwrap();
        let draft = list.items.iter().find(|x| x.id == id).unwrap();
        assert!(draft.attachments.is_empty());
        client.delete_draft(ALICE, id).await.unwrap();
    }
}
