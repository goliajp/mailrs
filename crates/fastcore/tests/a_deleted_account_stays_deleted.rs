//! Disconnecting an account while it is syncing.
//!
//! A full re-read takes as long as a mailbox is big — that is why
//! there is a progress note at all — and disconnecting is one button.
//! The pass that was already running finishes afterwards and writes
//! the row back, unless it checks. It did not check.
//!
//! What came back was worse than a stale row: the sealed credential
//! and the sync markers had been deleted with the account, so the row
//! reappeared as one that could never work, and would re-download the
//! whole mailbox if anybody ever reconnected it. Nothing said so —
//! the account simply seemed to come back broken.
//!
//! Driven against a scripted server, the way `kevy-client`'s own
//! transaction tests are: what matters is the bytes that go out.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mailrs_core_sidestate::families::external_accounts::AccountRow;
use mailrs_fastcore::external_sync::save_if_present;

/// A one-shot server that scripts `(bytes_to_wait_for, reply)` rounds
/// and records everything it was sent.
fn mock(rounds: Vec<(usize, &'static [u8])>) -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = mpsc::channel();
    let (up_tx, up_rx) = mpsc::channel();
    thread::spawn(move || {
        up_tx.send(()).unwrap();
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut all = Vec::new();
        let mut buf = vec![0u8; 4096];
        for (need, reply) in rounds {
            let mut got = 0;
            while got < need {
                match sock.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        all.extend_from_slice(&buf[..n]);
                        got += n;
                    }
                    _ => break,
                }
            }
            let _ = sock.write_all(reply);
        }
        thread::sleep(Duration::from_millis(50));
        let _ = seen_tx.send(all);
    });
    up_rx.recv().unwrap();
    (port, seen_rx)
}

fn a_row() -> AccountRow {
    AccountRow {
        id: "acc_1".into(),
        email: "someone@gmail.com".into(),
        ..AccountRow::default()
    }
}

/// The row is gone: `HGET` answers nil, so the pass lets go.
#[test]
fn a_pass_that_finishes_after_a_delete_writes_nothing() {
    let (port, seen) = mock(vec![
        (20, b"+OK\r\n"), // WATCH
        (20, b"$-1\r\n"), // HGET → nil: somebody disconnected it
        (13, b"+OK\r\n"), // UNWATCH
    ]);
    let mut conn = kevy_client::Connection::connect(&format!("kevy://127.0.0.1:{port}")).unwrap();
    let wrote = save_if_present(&mut conn, "ext:accts:me@golia.jp", &a_row()).unwrap();
    assert!(!wrote, "a deleted account was written back");

    drop(conn);
    let sent = seen.recv_timeout(Duration::from_secs(3)).unwrap();
    let text = String::from_utf8_lossy(&sent).to_uppercase();
    assert!(text.contains("WATCH"), "the field was read without a watch");
    assert!(
        !text.contains("HSET"),
        "the pass wrote the row back anyway:\n{}",
        String::from_utf8_lossy(&sent)
    );
}

/// Still there: the write goes out inside the transaction.
#[test]
fn an_account_that_is_still_there_is_written_back() {
    let (port, seen) = mock(vec![
        (20, b"+OK\r\n"),      // WATCH
        (20, b"$2\r\n{}\r\n"), // HGET → a row is there
        (14, b"+OK\r\n"),      // MULTI
        (40, b"+QUEUED\r\n"),  // HSET
        (13, b"*1\r\n:1\r\n"), // EXEC
    ]);
    let mut conn = kevy_client::Connection::connect(&format!("kevy://127.0.0.1:{port}")).unwrap();
    let wrote = save_if_present(&mut conn, "ext:accts:me@golia.jp", &a_row()).unwrap();
    assert!(wrote, "a live account was not written back");

    drop(conn);
    let sent = seen.recv_timeout(Duration::from_secs(3)).unwrap();
    let text = String::from_utf8_lossy(&sent).to_uppercase();
    assert!(text.contains("HSET"), "nothing was written");
}

/// Somebody else wrote to the hash between the read and the write, so
/// `EXEC` returns nil. Their answer is the newer one.
#[test]
fn a_watch_that_aborted_is_not_retried_over_the_top() {
    let (port, _seen) = mock(vec![
        (20, b"+OK\r\n"),      // WATCH
        (20, b"$2\r\n{}\r\n"), // HGET
        (14, b"+OK\r\n"),      // MULTI
        (40, b"+QUEUED\r\n"),  // HSET
        (13, b"$-1\r\n"),      // EXEC → aborted
    ]);
    let mut conn = kevy_client::Connection::connect(&format!("kevy://127.0.0.1:{port}")).unwrap();
    let wrote = save_if_present(&mut conn, "ext:accts:me@golia.jp", &a_row()).unwrap();
    assert!(!wrote, "an aborted transaction was reported as a write");
}
