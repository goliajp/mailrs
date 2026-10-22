use std::cell::RefCell;

use super::*;

/// Records what the core was asked to index.
struct Recorder {
    root: String,
    indexed: RefCell<Vec<(String, bool, String)>>,
}

impl Threads for Recorder {
    fn thread_for_message_id(&self, _: &str, mid: &str) -> Option<String> {
        (mid == "parent@x.test").then(|| "t-parent".to_string())
    }
    fn thread_subject(&self, _: &str, _: &str) -> Option<String> {
        Some("Plans".into())
    }
}

impl SentStore for Recorder {
    fn maildir_root(&self) -> &str {
        &self.root
    }
    fn net_conn(&self) -> Option<Connection> {
        None
    }
    fn index_own(&self, _: &str, blob_ref: &str, _: &[u8], a: &Arrival<'_>) -> io::Result<()> {
        self.indexed
            .borrow_mut()
            .push((blob_ref.to_string(), a.is_own, a.thread_id.clone()));
        Ok(())
    }
}

/// Sent as an alias the user may use: `From:` is not their address, and
/// the copy is still theirs.
const REPLY: &[u8] = b"From: Team <team@example.test>\r\n\
To: carol@x.test\r\n\
Subject: Re: Plans\r\n\
Message-ID: <mine@example.test>\r\n\
In-Reply-To: <parent@x.test>\r\n\
\r\n\
Thursday works.\r\n";

#[test]
fn a_sent_copy_is_filed_in_sent_read_and_indexed_as_the_users_own() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
    };
    let filed = file(&core, "alice@example.test", REPLY, &[]).expect("filed");

    assert_eq!(filed.message_id, "mine@example.test");
    assert_eq!(filed.thread_id, "t-parent", "the reply left its thread");
    let name = filed.blob_ref.strip_prefix(".Sent/").expect("in Sent");
    let sent = tmp.path().join("example.test/alice/.Sent");
    assert!(
        sent.join("cur").join(format!("{name}:2,S")).is_file(),
        "not on disk as read"
    );
    assert!(
        std::fs::read_dir(sent.join("new"))
            .unwrap()
            .next()
            .is_none()
    );
    let (blob, own, tid) = core.indexed.borrow()[0].clone();
    assert_eq!(
        (blob.as_str(), own, tid.as_str()),
        (filed.blob_ref.as_str(), true, "t-parent")
    );
}

#[test]
fn a_sent_copy_for_no_address_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
    };
    assert!(file(&core, "nobody", REPLY, &[]).is_err());
    assert!(core.indexed.borrow().is_empty());
}

#[test]
fn every_envelope_recipient_joins_the_contacts_under_its_address() {
    let mut conn = Connection::connect("mem://intake-sent-contacts").expect("kevy");
    let rcpts = [
        "Carol <carol@x.test>".to_string(),
        "hidden@x.test".to_string(),
        " ".to_string(),
    ];
    record_recipients(&mut conn, "alice@example.test", &rcpts).expect("recorded");
    let key = b"mailrs:user:alice@example.test:contacts";
    let flat = conn.hgetall(key).expect("contacts");
    let mut got: Vec<(Vec<u8>, Vec<u8>)> = flat
        .chunks(2)
        .map(|kv| (kv[0].clone(), kv[1].clone()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (b"carol@x.test".to_vec(), b"Carol <carol@x.test>".to_vec()),
            (b"hidden@x.test".to_vec(), b"hidden@x.test".to_vec()),
        ]
    );
}
