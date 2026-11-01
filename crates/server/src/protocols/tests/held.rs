//! A held conversation as IMAP shows it, on both cores: it leaves INBOX,
//! is listed and read in `Review`, and a client cannot move it back out —
//! releasing is the review screen's decision.

use std::future::Future;
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::script::message;

const SUBJECT: &str = "held";

/// `hold` holds the conversation it is given, the way a fraud verdict does
/// on that core.
pub(super) async fn held<D, F, Fut>(dir: Arc<D>, hold: F) -> Vec<String>
where
    D: Directory,
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut diffs = Vec::new();
    let mut check = |ok: bool, what: &str, got: &str| {
        if !ok {
            diffs.push(format!("held: {what}\n    got: {got:?}"));
        }
    };
    let mut c = Client::imap(dir).await;
    let r = c
        .append("h1", "INBOX", "()", &message(SUBJECT, "a prize awaits"))
        .await;
    let Some(appended) = code(&r, "[APPENDUID ") else {
        check(false, "APPEND names the message", &r);
        return diffs;
    };
    let uid = appended[1].to_string();
    hold(format!("{SUBJECT}@example.test")).await;

    c.cmd("h2", "SELECT INBOX").await;
    let r = c.cmd("h3", &format!("UID FETCH {uid} (FLAGS)")).await;
    check(!r.contains(" FETCH ("), "a held message leaves INBOX", &r);
    let r = c.cmd("h4", "LIST \"\" \"Review\"").await;
    check(r.contains("\"/\" Review\r\n"), "Review is listed", &r);
    let r = c.cmd("h5", "SELECT Review").await;
    check(
        r.contains("* 1 EXISTS"),
        "Review holds the held message",
        &r,
    );
    let r = c.cmd("h6", "FETCH 1 (BODY.PEEK[])").await;
    check(
        r.contains(&format!("Subject: {SUBJECT}")),
        "and it reads back there",
        &r,
    );
    let r = c.cmd("h7", "MOVE 1 INBOX").await;
    check(
        r.contains("h7 NO [CANNOT]"),
        "a client cannot move it out",
        &r,
    );
    let r = c.cmd("h8", "STATUS Review (MESSAGES)").await;
    check(
        r.contains("(MESSAGES 1)"),
        "and it is still there after",
        &r,
    );
    diffs
}
