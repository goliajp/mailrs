//! A POP3 unique-id lasts as long as the message: reading or flagging it
//! elsewhere does not change it, or every POP3 client fetches it again.
//! Each store picks its own ids, so only that they hold still is checked.

use std::sync::Arc;
use std::time::Duration;

use mailrs_mail_protocols::backend::Directory;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::checks::Client;
use super::harness::{PASSWORD, USER};

/// The UIDL listing, one `n id` line each.
async fn listing<D: Directory>(dir: Arc<D>) -> Vec<String> {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::pop3::session(dir, server, None));
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    let login = format!("USER {USER}\r\nPASS {PASSWORD}\r\nUIDL\r\nQUIT\r\n");
    w.write_all(login.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    loop {
        let mut l = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut l));
        match read.await {
            Ok(Ok(n)) if n > 0 && l != ".\r\n" => out.push(l),
            _ => break,
        }
    }
    // greeting, USER, PASS and the listing's own +OK
    out.split_off(4.min(out.len()))
}

/// INBOX holds one message, `\Seen`.
pub(super) async fn stable<D: Directory>(dir: Arc<D>) -> Vec<String> {
    let before = listing(dir.clone()).await;
    let mut c = Client::imap(dir.clone()).await;
    c.cmd("v1", "SELECT INBOX").await;
    c.cmd("v2", "STORE 1 -FLAGS.SILENT (\\Seen)").await;
    let after = listing(dir.clone()).await;
    c.cmd("v3", "STORE 1 +FLAGS.SILENT (\\Seen \\Flagged)")
        .await;
    let again = listing(dir).await;
    let mut diffs = Vec::new();
    if before.len() != 1 {
        diffs.push(format!("uidl: one message listed\n    got: {before:?}"));
    }
    if before != after || after != again {
        diffs.push(format!(
            "uidl: the unique-id changed with the flags\n    got: {before:?} {after:?} {again:?}"
        ));
    }
    diffs
}
