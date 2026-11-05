//! A client that submits over SMTP and then APPENDs its copy to Sent, as
//! most do, ends with one copy there, and it is the client's: the copy the
//! drain filed from the submission has no Bcc, the client's does. A
//! second APPEND of the same message, or one the client APPENDed with no
//! submission before it, writes no second file. Another mailbox still takes
//! a second copy of the same message.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::harness::{Backends, USER};
use super::passwordless::BLANK;
use super::script::message;

const SUBJECT: &str = "submitted";
const SUBMITTED_ID: &str = "submitted-with-bcc@example.test";

fn files_in(home: &Path, folder: &str, message_id: &str) -> Vec<String> {
    let needle = format!("Message-ID: <{message_id}>");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(home.join(folder).join(leaf)).ok())
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .filter(|text| text.contains(&needle))
        .collect()
}

struct Checks(Vec<String>);

impl Checks {
    fn check(&mut self, ok: bool, what: &str, got: &str) {
        if !ok {
            self.0
                .push(format!("sent APPEND: {what}\n    got: {got:?}"));
        }
    }
}

pub(super) async fn append_twice<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut t = Checks(Vec::new());
    let mut c = Client::imap(dir).await;
    let raw = message(SUBJECT, "filed by the client alone");
    let id = format!("{SUBJECT}@example.test");
    let first = c.append("s1", "Sent", "(\\Seen)", &raw).await;
    let second = c.append("s2", "Sent", "(\\Seen)", &raw).await;
    let (a, b) = (code(&first, "[APPENDUID "), code(&second, "[APPENDUID "));
    t.check(a.is_some(), "the first APPEND is filed", &first);
    t.check(
        a.is_some() && a == b,
        "the second APPEND names the first message",
        &second,
    );
    let n = files_in(&home, ".Sent", &id).len();
    t.check(n == 1, "one file in .Sent", &format!("{n} file(s)"));

    let other = c.append("s3", "Drafts", "()", &raw).await;
    t.check(
        code(&other, "[APPENDUID ").is_some(),
        "another mailbox takes it",
        &other,
    );
    let n = files_in(&home, ".Drafts", &id).len();
    t.check(n == 1, "one file in .Drafts", &format!("{n} file(s)"));
    t.0
}

/// What the client submitted, as the receiver spools it: no Bcc.
fn submitted() -> String {
    format!(
        "Received: from app.example.org by mail.example.test\r\n\
From: {USER}\r\nTo: {BLANK}\r\nSubject: with bcc\r\nMessage-ID: <{SUBMITTED_ID}>\r\n\r\nhello\r\n"
    )
}

/// What the client files in Sent: the same message, with its Bcc.
fn clients_copy() -> String {
    format!(
        "From: {USER}\r\nTo: {BLANK}\r\nBcc: hidden@partner.test\r\nSubject: with bcc\r\n\
Message-ID: <{SUBMITTED_ID}>\r\n\r\nhello\r\n"
    )
}

/// The submission in a fresh spool, for `drain` to take.
fn spooled() -> tempfile::TempDir {
    let spool = tempfile::tempdir().unwrap();
    let env = SpoolEnvelope {
        reverse_path: USER.into(),
        forward_paths: vec![BLANK.into()],
        is_authenticated: true,
        auth_user: Some(USER.into()),
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    let new = spool.path().join("incoming/new");
    std::fs::create_dir_all(&new).unwrap();
    let blob = encode_spool_blob(&env, submitted().as_bytes());
    std::fs::write(new.join("1790000000.M1P1.sub"), blob).unwrap();
    spool
}

/// Drain the submission on the fastcore side.
pub(super) async fn submit_on_fastcore(b: &Backends) {
    let spool = spooled();
    let state = b.fastcore_state.clone();
    let drained = tokio::task::spawn_blocking(move || {
        let dir = spool.path().join("incoming/new");
        mailrs_fastcore::spool_drain::drain_once(&dir, &state.maildir_root, &state).0
    });
    assert_eq!(drained.await.unwrap(), 1, "fastcore took nothing");
}

/// Drain the submission on the SQL side.
pub(super) async fn submit_on_pg(b: &Backends) {
    let spool = spooled();
    let core = crate::intake::PgCore {
        dir: crate::intake::PgDirectory::new(
            b.pg.domain.clone().expect("domain store"),
            b.pg.maildir_root.clone(),
            b.pg.net_url.clone(),
        ),
        mailbox: b.pg.mailbox.clone(),
        event_bus: crate::event_bus::EventBus::new(16),
    };
    let drained = tokio::task::spawn_blocking(move || {
        let dirs = mailrs_intake::spool::SpoolDirs::under(spool.path());
        mailrs_intake::spool::tick(&dirs, &core)
    });
    assert_eq!(drained.await.unwrap(), 1, "pg-core took nothing");
}

/// After the submission was drained: the client's APPEND replaces the
/// drain's copy, and the same APPEND again changes nothing.
pub(super) async fn after_submission<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut t = Checks(Vec::new());
    let filed = files_in(&home, ".Sent", SUBMITTED_ID);
    t.check(
        filed.len() == 1 && !filed[0].contains("Bcc:"),
        "the drain filed the submission",
        &format!("{filed:?}"),
    );
    let mut c = Client::imap(dir).await;
    let raw = clients_copy();
    let r = c.append("b1", "Sent", "(\\Seen)", &raw).await;
    let Some(appended) = code(&r, "[APPENDUID ") else {
        t.check(false, "the client's copy is filed", &r);
        return t.0;
    };
    let uid = appended[1].to_string();
    let filed = files_in(&home, ".Sent", SUBMITTED_ID);
    t.check(
        filed.len() == 1 && filed[0].contains("Bcc: hidden@partner.test"),
        "the client's copy, with its Bcc, is the one in .Sent",
        &format!("{filed:?}"),
    );
    c.cmd("b2", "SELECT Sent").await;
    let r = c
        .cmd(
            "b3",
            &format!("UID FETCH {uid} (BODY.PEEK[HEADER.FIELDS (BCC)])"),
        )
        .await;
    t.check(
        r.contains("hidden@partner.test"),
        "the uid APPENDUID gave reads the client's copy",
        &r,
    );
    let again = c.append("b4", "Sent", "(\\Seen)", &raw).await;
    t.check(
        code(&again, "[APPENDUID ").is_some_and(|w| w[1] == uid),
        "the same APPEND again names the client's copy",
        &again,
    );
    let n = files_in(&home, ".Sent", SUBMITTED_ID).len();
    t.check(n == 1, "still one file in .Sent", &format!("{n} file(s)"));
    t.0
}
