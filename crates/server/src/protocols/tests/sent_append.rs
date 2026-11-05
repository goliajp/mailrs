//! A client that submits over SMTP and then APPENDs its copy to Sent, as
//! most do, finds the copy the server already filed: the APPEND names that
//! message and writes no second file. Another mailbox still takes a second
//! copy of the same message.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::script::message;

const SUBJECT: &str = "submitted";

fn files_in(home: &Path, folder: &str) -> usize {
    let needle = format!("Message-ID: <{SUBJECT}@example.test>");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(home.join(folder).join(leaf)).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            std::fs::read(e.path())
                .map(|b| String::from_utf8_lossy(&b).contains(&needle))
                .unwrap_or(false)
        })
        .count()
}

pub(super) async fn append_twice<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut diffs = Vec::new();
    let mut check = |ok: bool, what: &str, got: &str| {
        if !ok {
            diffs.push(format!("sent APPEND: {what}\n    got: {got:?}"));
        }
    };
    let mut c = Client::imap(dir).await;
    let raw = message(SUBJECT, "filed by the submission");
    let first = c.append("s1", "Sent", "(\\Seen)", &raw).await;
    let second = c.append("s2", "Sent", "(\\Seen)", &raw).await;
    let (a, b) = (code(&first, "[APPENDUID "), code(&second, "[APPENDUID "));
    check(a.is_some(), "the first APPEND is filed", &first);
    check(
        a.is_some() && a == b,
        "the second APPEND names the first message",
        &second,
    );
    let n = files_in(&home, ".Sent");
    check(n == 1, "one file in .Sent", &format!("{n} file(s)"));

    let other = c.append("s3", "Drafts", "()", &raw).await;
    check(
        code(&other, "[APPENDUID ").is_some(),
        "another mailbox takes it",
        &other,
    );
    let n = files_in(&home, ".Drafts");
    check(n == 1, "one file in .Drafts", &format!("{n} file(s)"));
    diffs
}
