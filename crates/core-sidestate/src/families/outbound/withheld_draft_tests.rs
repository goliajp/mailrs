use std::path::Path;

use kevy_client::Connection;
use mailrs_core_api::method::admin::DraftWire;

use super::super::{COUNTER, Lapsed, SCHEDULED_IDX, job_key, release, withhold};
use crate::families::prefs::drafts_key;
use crate::families::uploads;

const DAVE: &str = "dave@x.test";

/// Two files, then an image the HTML draws in place by `cid:`.
const WITH_FILES: &[u8] = b"From: dave@x.test\r\nTo: erin@y.test\r\nSubject: files\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n\
--r\r\nContent-Type: multipart/alternative; boundary=\"a\"\r\n\r\n\
--a\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
--a\r\nContent-Type: text/html\r\n\r\n<p>see attached</p><img src=\"cid:logo@x\">\r\n\
--a--\r\n\
--r\r\nContent-Type: image/png; name=\"logo.png\"\r\nContent-ID: <logo@x>\r\n\
Content-Disposition: inline; filename=\"logo.png\"\r\n\r\nLOGO\r\n\
--r--\r\n\
--b\r\nContent-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"agenda.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n\
--b\r\nContent-Type: image/png; name=\"map.png\"\r\n\r\npng bytes\r\n--b--\r\n";

/// Queue `raw` for `DAVE`, set to go at 5 000, withhold it as the delete
/// does, and give it back at 10 000 as the restore does.
fn return_once(c: &mut Connection, root: &Path, raw: &[u8], send: &str) -> DraftWire {
    use base64::Engine as _;
    let id = c.incr(COUNTER).unwrap();
    let blob = serde_json::json!({ "id": id, "sender": DAVE, "recipient": "erin@y.test",
        "send_id": send,
        "message_data_b64": base64::engine::general_purpose::STANDARD.encode(raw) })
    .to_string();
    c.hset(
        job_key(id).as_bytes(),
        &[
            (b"state" as &[u8], b"pending" as &[u8]),
            (b"blob", blob.as_bytes()),
        ],
    )
    .unwrap();
    c.zadd(SCHEDULED_IDX, &[(5_000.0, id.to_string().as_bytes())])
        .unwrap();
    withhold(c, DAVE).unwrap();
    let done = release(c, DAVE, 10_000, Lapsed::Draft(root)).unwrap();
    assert_eq!(done.drafts, 1);
    let drafts = c.hgetall(drafts_key(DAVE).as_bytes()).unwrap();
    drafts
        .chunks(2)
        .map(|p| serde_json::from_slice::<DraftWire>(&p[1]).unwrap())
        .max_by_key(|d| d.id)
        .unwrap()
}

fn files_in(root: &Path) -> usize {
    std::fs::read_dir(uploads::account_dir(root, DAVE))
        .map(|it| it.count())
        .unwrap_or(0)
}

#[test]
fn a_returned_draft_carries_the_message_files_in_order() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Connection::connect("mem://withheld-draft-files").unwrap();
    let d = return_once(&mut c, root.path(), WITH_FILES, "f@x.test");
    let listed: Vec<(&str, &str, u64)> = d
        .attachments
        .iter()
        .map(|a| (a.name.as_str(), a.content_type.as_str(), a.size))
        .collect();
    assert_eq!(
        listed,
        [
            ("agenda.pdf", "application/pdf", 9),
            ("map.png", "image/png", 9)
        ]
    );
    let bytes: Vec<Vec<u8>> = d
        .attachments
        .iter()
        .map(|a| std::fs::read(uploads::path_of(root.path(), DAVE, &a.blob_id).unwrap()).unwrap())
        .collect();
    assert_eq!(bytes, [b"%PDF-1.4\n".to_vec(), b"png bytes".to_vec()]);
    assert!(d.attachments_not_included.is_empty(), "{d:?}");
    assert_eq!(d.body.trim(), "see attached");
    assert_eq!(files_in(root.path()), 2, "the inline image is not a file");
}

#[test]
fn a_message_without_files_is_returned_as_before() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Connection::connect("mem://withheld-draft-plain").unwrap();
    let raw = b"From: dave@x.test\r\nTo: erin@y.test\r\nSubject: hi\r\n\r\nbody\r\n";
    let d = return_once(&mut c, root.path(), raw, "p@x.test");
    assert!(d.attachments.is_empty());
    assert!(d.attachments_not_included.is_empty());
    let json = serde_json::to_string(&d).unwrap();
    assert!(!json.contains("attachments"), "{json}");
    assert!(!uploads::account_dir(root.path(), DAVE).exists());
}

#[test]
fn returning_the_same_files_twice_stores_each_once() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Connection::connect("mem://withheld-draft-twice").unwrap();
    let first = return_once(&mut c, root.path(), WITH_FILES, "t1@x.test");
    let second = return_once(&mut c, root.path(), WITH_FILES, "t2@x.test");
    assert_ne!(first.id, second.id);
    let ids = |d: &DraftWire| {
        d.attachments
            .iter()
            .map(|a| a.blob_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(ids(&first).len(), 2);
    assert_eq!(files_in(root.path()), 2);
}
