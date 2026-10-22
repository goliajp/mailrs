//! The two cores hold the same IMAP, POP3 and ManageSieve conversations.
//!
//! One scripted client talks to the shared sessions over fastcore's stores
//! and over the SQL store, from the same seed, and every reply is compared
//! with the one the script expects. Values each store mints on its own
//! (UIDVALIDITY, HIGHESTMODSEQ) are masked where they are read.
//!
//! One test, run in order: the POP3 and ManageSieve parts read what the
//! IMAP part left.

mod folders;
mod harness;
mod script;
mod selection;
mod steps;

use std::sync::Arc;
use std::time::Duration;

use mailrs_mail_protocols::backend::Directory;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// What a client sends, and every line it expects back.
pub(super) struct Step {
    pub send: String,
    pub expect: String,
}

/// Mask the numbers each store chooses for itself: the digits after each
/// of these words.
fn masked(line: &str) -> String {
    let mut out = line.to_string();
    for word in ["UIDVALIDITY ", "HIGHESTMODSEQ "] {
        let mut from = 0;
        while let Some(at) = out[from..].find(word) {
            let start = from + at + word.len();
            let digits = out[start..].bytes().take_while(u8::is_ascii_digit).count();
            out.replace_range(start..start + digits, "*");
            from = start;
        }
    }
    out
}

/// Run `steps` on one connection; each difference from the script, named
/// by the step that showed it.
async fn converse<S>(io: S, protocol: &str, steps: &[Step]) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    let mut diffs = Vec::new();
    for step in steps {
        if !step.send.is_empty() {
            w.write_all(step.send.as_bytes()).await.unwrap();
        }
        let mut got = String::new();
        for _ in 0..step.expect.matches('\n').count() {
            let mut l = String::new();
            match tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut l)).await {
                Ok(Ok(n)) if n > 0 => got.push_str(&masked(&l)),
                _ => {
                    got.push_str("<nothing more>\n");
                    break;
                }
            }
        }
        if got != step.expect {
            let sent = step.send.lines().next().unwrap_or("(connect)");
            diffs.push(format!(
                "{protocol} `{sent}`\n    expected: {:?}\n    got:      {got:?}",
                step.expect
            ));
        }
    }
    diffs
}

/// IMAP conversations on fresh connections, in order, after the first
/// four: each starts from what the one before left.
fn later() -> Vec<Vec<Step>> {
    vec![selection::unselect(), folders::status(), folders::fixed()]
}

async fn conversations<D: Directory>(dir: Arc<D>) -> Vec<String> {
    let mut diffs = Vec::new();
    let (client, server) = tokio::io::duplex(1 << 20);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
    diffs.extend(converse(client, "imap", &script::imap()).await);

    let (client, server) = tokio::io::duplex(1 << 20);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
    diffs.extend(converse(client, "imap", &script::authenticate()).await);

    let (client, server) = tokio::io::duplex(1 << 20);
    let pop3 = mailrs_mail_protocols::pop3::session(dir.clone(), server, None);
    tokio::spawn(pop3);
    diffs.extend(converse(client, "pop3", &script::pop3()).await);

    let (client, server) = tokio::io::duplex(1 << 20);
    let sieve = mailrs_mail_protocols::managesieve::session(dir.clone(), server, None);
    tokio::spawn(sieve);
    diffs.extend(converse(client, "managesieve", &script::managesieve()).await);

    for steps in later() {
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
        diffs.extend(converse(client, "imap", &steps).await);
    }
    diffs
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_cores_hold_the_same_mail_conversations() {
    let backends = harness::start().await;
    let mut report = Vec::new();
    for diff in conversations(backends.fastcore.clone()).await {
        report.push(format!("fastcore: {diff}"));
    }
    for diff in conversations(backends.pg.clone()).await {
        report.push(format!("pg-core: {diff}"));
    }
    assert!(
        report.is_empty(),
        "{} difference(s) from the script:\n\n{}",
        report.len(),
        report.join("\n\n")
    );
}

#[test]
fn the_minted_numbers_are_masked_and_nothing_else() {
    assert_eq!(
        masked("* OK [UIDVALIDITY 1760000000] Version 1\r\n"),
        "* OK [UIDVALIDITY *] Version 1\r\n"
    );
    assert_eq!(
        masked("* OK [UIDNEXT 4] Predicted next UID\r\n"),
        "* OK [UIDNEXT 4] Predicted next UID\r\n"
    );
    assert_eq!(
        masked("* STATUS INBOX (UIDVALIDITY 17 UIDNEXT 4 HIGHESTMODSEQ 9)\r\n"),
        "* STATUS INBOX (UIDVALIDITY * UIDNEXT 4 HIGHESTMODSEQ *)\r\n"
    );
}
