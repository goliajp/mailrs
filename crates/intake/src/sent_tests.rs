use std::cell::RefCell;

use super::*;

/// Records what the core was asked to index.
struct Recorder {
    root: String,
    indexed: RefCell<Vec<(String, bool, String)>>,
    arrivals: RefCell<Vec<(String, bool, String)>>,
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
    fn authserv_id(&self) -> Option<&str> {
        Some(OURS)
    }
    fn index_own(&self, _: &str, blob_ref: &str, _: &[u8], a: &Arrival<'_>) -> io::Result<()> {
        self.indexed
            .borrow_mut()
            .push((blob_ref.to_string(), a.is_own, a.thread_id.clone()));
        Ok(())
    }
    fn index_arrival(
        &self,
        _: &str,
        blob_ref: &str,
        _: &[u8],
        a: &Arrival<'_>,
        _: Option<&str>,
    ) -> io::Result<()> {
        self.arrivals
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
        arrivals: RefCell::default(),
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
        arrivals: RefCell::default(),
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
    let key =
        mailrs_core_sidestate::families::contact_cards::autocomplete_key("alice@example.test");
    let flat = conn.hgetall(key.as_bytes()).expect("contacts");
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

const RECEIVED: &[u8] = b"From: Carol <carol@x.test>\r\n\
To: alice@example.test\r\n\
Subject: Re: Plans\r\n\
Message-ID: <theirs@x.test>\r\n\
In-Reply-To: <parent@x.test>\r\n\
\r\n\
See you then.\r\n";

/// An imported message lands in INBOX, out of new/ and unread, and is
/// indexed as one the user received, in the thread it answers.
#[test]
fn an_imported_message_is_filed_in_the_inbox_as_received() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
        arrivals: RefCell::default(),
    };
    let filed = crate::import::file(&core, "alice@example.test", RECEIVED, None).expect("filed");
    assert_eq!(filed.message_id, "theirs@x.test");
    assert_eq!(filed.thread_id, "t-parent");
    let inbox = tmp.path().join("example.test/alice");
    assert!(
        inbox
            .join("cur")
            .join(format!("{}:2,", filed.blob_ref))
            .is_file(),
        "not in cur/ unread"
    );
    assert!(
        core.indexed.borrow().is_empty(),
        "not indexed as the user's own"
    );
    let (blob, own, tid) = core.arrivals.borrow()[0].clone();
    assert_eq!(
        (blob.as_str(), own, tid.as_str()),
        (filed.blob_ref.as_str(), false, "t-parent")
    );
}

/// This server's authserv-id in these tests.
const OURS: &str = "mail.example.test";

/// What a forger puts on top: our name, our trace, and a pass for a
/// domain they do not control.
const FORGED: &str = "Authentication-Results: mail.example.test;\r\n\tdkim=pass header.d=stripe.com header.s=s1\r\n\
Received: from mx.stripe.com (1.2.3.4:5)\r\n\tby mail.example.test with ESMTP\r\n\tfor <alice@example.test>; Mon, 2 Nov 2026 00:00:00 +0000\r\n\
Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n";

/// Every file under `dir`, read whole.
fn files(dir: &std::path::Path) -> Vec<String> {
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(dir.join(leaf)).ok())
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .collect()
}

fn filed_without_our_name(on_disk: &[String]) {
    assert_eq!(on_disk.len(), 1, "one file");
    let head = on_disk[0].split("\r\n\r\n").next().unwrap();
    assert!(
        !head.contains("Authentication-Results: mail.example.test"),
        "a header naming this server was filed:\n{head}"
    );
    assert!(
        head.contains("Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com"),
        "another server's header is not ours to remove:\n{head}"
    );
}

#[test]
fn an_import_loses_a_results_header_naming_this_server() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
        arrivals: RefCell::default(),
    };
    let raw = [FORGED.as_bytes(), RECEIVED].concat();
    crate::import::file(&core, "alice@example.test", &raw, None).expect("filed");
    filed_without_our_name(&files(&tmp.path().join("example.test/alice")));
}

#[test]
fn a_sent_copy_loses_a_results_header_naming_this_server() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
        arrivals: RefCell::default(),
    };
    let raw = [FORGED.as_bytes(), REPLY].concat();
    file(&core, "alice@example.test", &raw, &[]).expect("filed");
    filed_without_our_name(&files(&tmp.path().join("example.test/alice/.Sent")));
}

#[test]
fn a_submission_already_held_files_nothing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let core = Recorder {
        root: tmp.path().to_string_lossy().into_owned(),
        indexed: RefCell::default(),
        arrivals: RefCell::default(),
    };
    let held = b"From: alice@example.test\r\nMessage-ID: <parent@x.test>\r\n\r\nagain\r\n";
    let none = file_submitted(&core, "alice@example.test", held, &[]).expect("checked");
    assert_eq!(none, None);
    assert!(core.indexed.borrow().is_empty());

    let filed = file_submitted(&core, "alice@example.test", REPLY, &[]).expect("filed");
    assert_eq!(
        filed.map(|f| f.message_id).as_deref(),
        Some("mine@example.test")
    );
    assert_eq!(core.indexed.borrow().len(), 1);
}
