use std::path::Path;
use std::time::{Duration, SystemTime};

use kevy_client::Connection;
use mailrs_core_api::method::admin::{DraftAttachment, SaveDraftRequest};

use super::*;
use crate::families::uploads::{self, KEEP_FOR, Put, sweep};

const ALICE: &str = "alice@x.test";
const BOB: &str = "bob@x.test";
const NOW: i64 = 1_800_000_000;

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://draft-store-{name}")).unwrap()
}

fn upload(root: &Path, user: &str, bytes: &[u8]) -> DraftAttachment {
    let Put::Stored(sha) = uploads::put(root, user, bytes, SystemTime::now()).unwrap() else {
        panic!("not stored");
    };
    DraftAttachment {
        blob_id: uploads::blob_id(&sha),
        name: format!("{}.bin", bytes.len()),
        content_type: "application/octet-stream".into(),
        // the store's size wins over what a client says
        size: 1,
    }
}

fn req(id: Option<i64>, body: &str, attachments: Option<Vec<DraftAttachment>>) -> SaveDraftRequest {
    SaveDraftRequest {
        id,
        body: body.into(),
        attachments,
        ..Default::default()
    }
}

#[test]
fn a_save_without_attachments_keeps_them_and_an_empty_list_clears_them() {
    let mut c = conn("keep");
    let root = tempfile::tempdir().unwrap();
    let a = upload(root.path(), ALICE, b"first");
    let b = upload(root.path(), ALICE, b"second file");
    let saved = save(
        &mut c,
        root.path(),
        ALICE,
        req(None, "v1", Some(vec![a, b])),
        NOW,
    )
    .unwrap();
    let first = get(&mut c, ALICE, saved.id).unwrap().unwrap();
    assert_eq!(
        first.attachments.iter().map(|a| a.size).collect::<Vec<_>>(),
        vec![5, 11],
        "in order, sized from the files"
    );
    assert_eq!(first.created_at, NOW);

    // an older client's autosave: no `attachments` at all
    save(
        &mut c,
        root.path(),
        ALICE,
        req(Some(saved.id), "v2", None),
        NOW + 60,
    )
    .unwrap();
    let kept = get(&mut c, ALICE, saved.id).unwrap().unwrap();
    assert_eq!(kept.body, "v2");
    assert_eq!(kept.attachments, first.attachments);
    assert_eq!(kept.created_at, NOW, "the first save's time");
    assert_eq!(kept.updated_at, NOW + 60);

    save(
        &mut c,
        root.path(),
        ALICE,
        req(Some(saved.id), "v3", Some(vec![])),
        NOW + 120,
    )
    .unwrap();
    assert!(
        get(&mut c, ALICE, saved.id)
            .unwrap()
            .unwrap()
            .attachments
            .is_empty()
    );
}

#[test]
fn a_file_that_is_not_the_accounts_upload_is_refused_and_nothing_written() {
    let mut c = conn("missing");
    let root = tempfile::tempdir().unwrap();
    let saved = save(&mut c, root.path(), ALICE, req(None, "kept", None), NOW).unwrap();
    let bobs = upload(root.path(), BOB, b"bob's file");
    let nothing = DraftAttachment {
        blob_id: uploads::blob_id(&"0".repeat(64)),
        name: "gone.pdf".into(),
        content_type: "application/pdf".into(),
        size: 3,
    };
    let not_an_upload = DraftAttachment {
        blob_id: "B12".into(),
        ..nothing.clone()
    };
    for (list, named) in [
        (vec![bobs.clone()], bobs.blob_id.clone()),
        (vec![nothing.clone()], nothing.blob_id.clone()),
        (vec![not_an_upload], "B12".into()),
    ] {
        let err = save(
            &mut c,
            root.path(),
            ALICE,
            req(Some(saved.id), "changed", Some(list)),
            NOW,
        )
        .unwrap_err();
        let (status, body) = err.into_parts();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body.0["missing"][0].as_str().unwrap().contains(&named),
            "{body:?}"
        );
    }
    assert_eq!(get(&mut c, ALICE, saved.id).unwrap().unwrap().body, "kept");
    // a new draft refused does not spend an id either
    let err = save(
        &mut c,
        root.path(),
        ALICE,
        req(None, "x", Some(vec![nothing])),
        NOW,
    );
    assert!(err.is_err());
    let next = save(&mut c, root.path(), ALICE, req(None, "y", None), NOW).unwrap();
    assert_eq!(next.id, saved.id + 1);
}

#[test]
fn files_over_the_limit_are_refused_naming_it() {
    let mut c = conn("large");
    let root = tempfile::tempdir().unwrap();
    let half = vec![7u8; (MAX_ATTACHMENT_BYTES / 2) as usize];
    let mut more = half.clone();
    more.push(1);
    let a = upload(root.path(), ALICE, &half);
    let b = upload(root.path(), ALICE, &more);
    let err = save(
        &mut c,
        root.path(),
        ALICE,
        req(None, "", Some(vec![a.clone(), b])),
        NOW,
    )
    .unwrap_err();
    let (status, body) = err.into_parts();
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body.0["limit_bytes"], MAX_ATTACHMENT_BYTES);
    assert_eq!(body.0["total_bytes"], MAX_ATTACHMENT_BYTES + 1);
    let c2 = c.dbsize().unwrap();
    assert_eq!(c2, 0, "nothing written");
    save(
        &mut c,
        root.path(),
        ALICE,
        req(None, "", Some(vec![a.clone(), a])),
        NOW,
    )
    .unwrap();
}

#[test]
fn saving_a_draft_keeps_a_file_that_had_expired() {
    let mut c = conn("touch");
    let root = tempfile::tempdir().unwrap();
    let a = upload(root.path(), ALICE, b"old upload");
    let path = uploads::path_of(root.path(), ALICE, &a.blob_id).unwrap();
    let now = SystemTime::now();
    uploads::touch(&path, now - KEEP_FOR - Duration::from_secs(60)).unwrap();
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let saved = save(
        &mut c,
        root.path(),
        ALICE,
        req(None, "", Some(vec![a])),
        secs,
    )
    .unwrap();
    // the draft goes, so only the fresh time can keep the file
    remove(&mut c, ALICE, saved.id).unwrap();
    let pass = sweep::sweep(&mut c, root.path(), now, false).unwrap();
    assert_eq!(pass.deleted, 0);
    assert!(path.exists());
}

#[test]
fn saving_a_draft_keeps_its_pasted_images() {
    let mut c = conn("images");
    let root = tempfile::tempdir().unwrap();
    let id = "0123456789abcdef0123456789abcdef";
    let key = inline_images::key(id);
    c.hset(key.as_bytes(), &[(b"body".as_slice(), b"png".as_slice())])
        .unwrap();
    c.expire(key.as_bytes(), Duration::from_secs(3600)).unwrap();
    let body = format!("<img src=\"/api/mail/inline/{id}\">");
    save(&mut c, root.path(), ALICE, req(None, &body, None), NOW).unwrap();
    let left = c.ttl_ms(key.as_bytes()).unwrap() as u128;
    assert!(left > inline_images::KEEP.as_millis() / 2, "{left}");
}

#[test]
fn what_a_restore_noted_on_a_draft_outlives_its_saves() {
    let mut c = conn("restore-notes");
    let root = tempfile::tempdir().unwrap();
    let saved = save(&mut c, root.path(), ALICE, req(None, "v1", None), NOW).unwrap();
    let mut d = get(&mut c, ALICE, saved.id).unwrap().unwrap();
    d.withheld_send_at = Some(NOW - 600);
    d.attachments_not_included = vec!["contract.pdf".into()];
    put(&mut c, ALICE, &d).unwrap();
    let a = upload(root.path(), ALICE, b"a file added since");
    let later = req(Some(saved.id), "v2", Some(vec![a]));
    save(&mut c, root.path(), ALICE, later, NOW + 60).unwrap();
    let after = get(&mut c, ALICE, saved.id).unwrap().unwrap();
    assert_eq!(after.body, "v2");
    assert_eq!(after.withheld_send_at, Some(NOW - 600));
    assert_eq!(
        after.attachments_not_included,
        vec!["contract.pdf".to_string()]
    );
    assert_eq!(after.attachments.len(), 1);
}
