//! What a fixed script cannot say, because each store picks the numbers:
//! the uids APPENDUID and COPYUID name are the messages the client then
//! finds there.

use std::sync::Arc;
use std::time::Duration;

use mailrs_mail_protocols::backend::Directory;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

use super::harness::{PASSWORD, USER};
use super::script::message;

pub(super) struct Client {
    r: BufReader<ReadHalf<DuplexStream>>,
    w: WriteHalf<DuplexStream>,
}

impl Client {
    pub(super) async fn imap<D: Directory>(dir: Arc<D>) -> Self {
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(mailrs_mail_protocols::imap::run(dir, server));
        let (r, w) = tokio::io::split(client);
        let mut c = Self {
            r: BufReader::new(r),
            w,
        };
        c.line().await;
        c.send(&format!("l LOGIN {USER} \"{PASSWORD}\"\r\n")).await;
        c.until("l ").await;
        c
    }

    pub(super) async fn send(&mut self, raw: &str) {
        self.w.write_all(raw.as_bytes()).await.unwrap();
    }

    async fn line(&mut self) -> String {
        let mut l = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), self.r.read_line(&mut l));
        match read.await {
            Ok(Ok(n)) if n > 0 => l,
            _ => "<nothing more>\n".into(),
        }
    }

    /// Every line up to and including the one starting with `end`.
    pub(super) async fn until(&mut self, end: &str) -> String {
        let mut out = String::new();
        loop {
            let l = self.line().await;
            out.push_str(&l);
            if l.starts_with(end) || l == "<nothing more>\n" {
                return out;
            }
        }
    }

    pub(super) async fn cmd(&mut self, tag: &str, line: &str) -> String {
        self.send(&format!("{tag} {line}\r\n")).await;
        self.until(&format!("{tag} ")).await
    }

    pub(super) async fn append(&mut self, tag: &str, to: &str, flags: &str, raw: &str) -> String {
        self.send(&format!("{tag} APPEND {to} {flags} {{{}}}\r\n", raw.len()))
            .await;
        self.line().await;
        self.send(&format!("{raw}\r\n")).await;
        self.until(&format!("{tag} ")).await
    }
}

/// The words of the response code `code` in `reply`.
pub(super) fn code<'a>(reply: &'a str, code: &str) -> Option<Vec<&'a str>> {
    let at = reply.find(code)? + code.len();
    let end = reply[at..].find(']')? + at;
    Some(reply[at..end].split(' ').collect())
}

fn uidvalidity(selected: &str) -> Option<&str> {
    code(selected, "[UIDVALIDITY ").map(|w| w[0])
}

/// APPEND with flags, then COPY, then UID EXPUNGE, each read back by uid.
pub(super) async fn uidplus<D: Directory>(dir: Arc<D>) -> Vec<String> {
    let mut c = Client::imap(dir).await;
    let mut diffs = Vec::new();
    let mut check = |ok: bool, what: &str, reply: &str| {
        if !ok {
            diffs.push(format!("uidplus: {what}\n    got: {reply:?}"));
        }
    };

    let raw = message("eight", "eighth");
    let r = c.append("k1", "Sent", "(\\Seen \\Flagged)", &raw).await;
    let Some(appended) = code(&r, "[APPENDUID ") else {
        check(false, "APPEND names the message", &r);
        return diffs;
    };
    let (v, uid) = (appended[0].to_string(), appended[1].to_string());
    let sel = c.cmd("k2", "SELECT Sent").await;
    check(
        uidvalidity(&sel) == Some(&v),
        "APPENDUID's uidvalidity is Sent's",
        &sel,
    );
    let r = c.cmd("k3", &format!("UID FETCH {uid} (FLAGS)")).await;
    let kept =
        r.contains(&format!("UID {uid} ")) && r.contains("\\Seen") && r.contains("\\Flagged");
    check(kept, "the appended message keeps its flags", &r);

    let r = c.cmd("k4", &format!("UID COPY {uid} Drafts")).await;
    let Some(copied) = code(&r, "[COPYUID ") else {
        check(false, "COPY names the copy", &r);
        return diffs;
    };
    check(copied[1] == uid, "COPYUID names the source uid", &r);
    let (cv, cuid) = (copied[0].to_string(), copied[2].to_string());

    let other = message("nine", "ninth");
    c.append("k5", "Drafts", "(\\Deleted)", &other).await;
    let sel = c.cmd("k6", "SELECT Drafts").await;
    check(
        uidvalidity(&sel) == Some(&cv),
        "COPYUID's uidvalidity is Drafts'",
        &sel,
    );
    let r = c.cmd("k7", &format!("UID FETCH {cuid} (FLAGS)")).await;
    check(
        r.contains(&format!("UID {cuid} ")),
        "the copy is where COPYUID says",
        &r,
    );

    c.cmd("k8", &format!("UID STORE {cuid} +FLAGS.SILENT (\\Deleted)"))
        .await;
    let r = c.cmd("k9", &format!("UID EXPUNGE {cuid}")).await;
    check(
        r.starts_with("* 1 EXPUNGE\r\nk9 OK "),
        "UID EXPUNGE takes only the uid named",
        &r,
    );
    let r = c.cmd("k10", "FETCH 1:* (FLAGS)").await;
    let left = r.lines().filter(|l| l.starts_with("* ")).count() == 1 && r.contains("\\Deleted");
    check(left, "the other deleted message stays", &r);
    diffs
}
