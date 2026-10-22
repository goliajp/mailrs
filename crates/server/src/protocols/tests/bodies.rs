//! A message's body is a file in its mailbox's Maildir++ folder, on both
//! cores: APPEND writes it there, COPY writes a second one into the target,
//! MOVE takes it across, EXPUNGE removes it. Each is read back over IMAP
//! and looked for on disk, and a copy or a move keeps the message's flags.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::script::message;

const SUBJECT: &str = "ten";
const BODY: &str = "tenth";

/// How many files in `home`'s `folder` hold the test message.
fn files_in(home: &Path, folder: &str) -> usize {
    let dir = match folder {
        "INBOX" => home.to_path_buf(),
        f => home.join(format!(".{f}")),
    };
    let needle = format!("Subject: {SUBJECT}\r\n");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(dir.join(leaf)).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            std::fs::read(e.path())
                .map(|b| String::from_utf8_lossy(&b).contains(&needle))
                .unwrap_or(false)
        })
        .count()
}

struct Checks {
    home: PathBuf,
    diffs: Vec<String>,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str, reply: &str) {
        if !ok {
            self.diffs
                .push(format!("bodies: {what}\n    got: {reply:?}"));
        }
    }

    fn on_disk(&mut self, folder: &str, want: usize, what: &str) {
        let got = files_in(&self.home, folder);
        self.check(got == want, what, &format!("{got} file(s) in {folder}"));
    }
}

/// The body and the flags of `uid` in the selected mailbox.
async fn read_back(c: &mut Client, tag: &str, uid: &str) -> String {
    c.cmd(tag, &format!("UID FETCH {uid} (FLAGS BODY.PEEK[])"))
        .await
}

fn kept(reply: &str) -> bool {
    reply.contains("\\Seen") && reply.contains("\\Flagged")
}

pub(super) async fn folders<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut c = Client::imap(dir).await;
    let mut t = Checks {
        home,
        diffs: Vec::new(),
    };

    let r = c
        .append("b1", "Sent", "(\\Seen \\Flagged)", &message(SUBJECT, BODY))
        .await;
    let Some(appended) = code(&r, "[APPENDUID ") else {
        t.check(false, "APPEND names the message", &r);
        return t.diffs;
    };
    let uid = appended[1].to_string();
    t.on_disk("Sent", 1, "APPEND to Sent writes the body into .Sent");
    t.on_disk("INBOX", 0, "and not into INBOX");
    c.cmd("b2", "SELECT Sent").await;
    let r = read_back(&mut c, "b3", &uid).await;
    t.check(r.contains(BODY), "the appended body reads back", &r);

    let r = c.cmd("b4", &format!("UID COPY {uid} Trash")).await;
    let copied = code(&r, "[COPYUID ").map(|w| w[2].to_string());
    t.on_disk("Trash", 1, "COPY writes a body into .Trash");
    t.on_disk("Sent", 1, "and leaves the one in .Sent");

    let r = c.cmd("b5", &format!("UID MOVE {uid} Junk")).await;
    let moved = code(&r, "[COPYUID ").map(|w| w[2].to_string());
    t.on_disk("Junk", 1, "MOVE takes the body into .Junk");
    t.on_disk("Sent", 0, "and out of .Sent");

    c.cmd("b6", "SELECT Trash").await;
    let r = read_back(&mut c, "b7", copied.as_deref().unwrap_or("*")).await;
    t.check(r.contains(BODY), "the copy's body reads back in Trash", &r);
    t.check(kept(&r), "the copy keeps the message's flags", &r);

    c.cmd("b8", "SELECT Junk").await;
    let moved = moved.unwrap_or_else(|| "*".into());
    let r = read_back(&mut c, "b9", &moved).await;
    t.check(r.contains(BODY), "the moved body reads back in Junk", &r);
    t.check(kept(&r), "the moved message keeps its flags", &r);

    c.cmd(
        "b10",
        &format!("UID STORE {moved} +FLAGS.SILENT (\\Deleted)"),
    )
    .await;
    let r = c.cmd("b11", &format!("UID EXPUNGE {moved}")).await;
    t.check(r.contains("b11 OK "), "UID EXPUNGE completes", &r);
    t.on_disk("Junk", 0, "EXPUNGE removes the body");
    t.on_disk("Trash", 1, "and only that one");
    t.diffs
}
