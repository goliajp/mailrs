//! A label as IMAP shows it, on both cores: copying a message into the
//! label's mailbox puts the label on and writes no file; the message reads
//! back there; expunging there takes the label off and leaves the message
//! where it is filed.

use std::path::PathBuf;
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::harness::LABEL;
use super::script::message;

const SUBJECT: &str = "labelled";

fn files_in_inbox(home: &std::path::Path) -> usize {
    let needle = format!("Subject: {SUBJECT}\r\n");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(home.join(leaf)).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            std::fs::read(e.path())
                .map(|b| String::from_utf8_lossy(&b).contains(&needle))
                .unwrap_or(false)
        })
        .count()
}

pub(super) async fn label<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut diffs = Vec::new();
    let mut check = |ok: bool, what: &str, got: &str| {
        if !ok {
            diffs.push(format!("labels: {what}\n    got: {got:?}"));
        }
    };
    let mut c = Client::imap(dir).await;
    let r = c
        .append("l1", "INBOX", "(\\Seen)", &message(SUBJECT, "body"))
        .await;
    let Some(appended) = code(&r, "[APPENDUID ") else {
        check(false, "APPEND names the message", &r);
        return diffs;
    };
    let uid = appended[1].to_string();
    c.cmd("l2", "SELECT INBOX").await;
    let r = c.cmd("l3", &format!("UID COPY {uid} {LABEL}")).await;
    let shown = code(&r, "[COPYUID ").map(|w| w[2].to_string());
    check(r.contains("l3 OK "), "COPY into the label completes", &r);
    let n = files_in_inbox(&home);
    check(n == 1, "and writes no second file", &format!("{n} file(s)"));

    let r = c.cmd("l4", &format!("SELECT {LABEL}")).await;
    check(r.contains("* 1 EXISTS"), "the label holds the message", &r);
    let shown = shown.unwrap_or_else(|| "*".into());
    let r = c
        .cmd("l5", &format!("UID FETCH {shown} (FLAGS BODY.PEEK[])"))
        .await;
    check(
        r.contains("body") && r.contains("\\Seen"),
        "and reads it back with its flags",
        &r,
    );

    c.cmd(
        "l6",
        &format!("UID STORE {shown} +FLAGS.SILENT (\\Deleted)"),
    )
    .await;
    let r = c.cmd("l7", "EXPUNGE").await;
    check(r.contains("l7 OK "), "EXPUNGE in the label completes", &r);
    let r = c.cmd("l8", &format!("SELECT {LABEL}")).await;
    check(r.contains("* 0 EXISTS"), "and takes the label off", &r);
    let n = files_in_inbox(&home);
    check(n == 1, "and deletes nothing", &format!("{n} file(s)"));
    c.cmd("l9", "SELECT INBOX").await;
    let r = c.cmd("l10", &format!("UID FETCH {uid} (UID)")).await;
    check(r.contains("* "), "the message is still in INBOX", &r);
    diffs
}
