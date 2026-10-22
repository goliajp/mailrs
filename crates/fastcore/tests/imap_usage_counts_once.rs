//! Each IMAP write changes the user's used bytes once: APPEND and COPY add
//! the message, MOVE leaves the total as it was, EXPUNGE takes it off.
//!
//! An integration test because the counter lives in the network kevy that
//! `MAILRS_KEVY_URL` names, and only a process of its own may set that.

use std::sync::Arc;

use mailrs_fastcore::{FastcoreDirectory, FastcoreState};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

const KEVY: &str = "mem://imap-usage-counts-once";
const USER: &str = "alice@example.test";
const PASSWORD: &str = "correct horse";
const RAW: &str =
    "From: bob@example.test\r\nSubject: hi\r\nMessage-ID: <usage@test>\r\n\r\nbody\r\n";

struct Imap {
    r: BufReader<ReadHalf<DuplexStream>>,
    w: WriteHalf<DuplexStream>,
}

impl Imap {
    async fn until(&mut self, tag: &str) -> String {
        let mut out = String::new();
        loop {
            let mut l = String::new();
            assert!(self.r.read_line(&mut l).await.unwrap() > 0, "closed: {out}");
            out.push_str(&l);
            if l.starts_with(&format!("{tag} ")) {
                return out;
            }
        }
    }

    async fn cmd(&mut self, tag: &str, line: &str) -> String {
        let sent = format!("{tag} {line}\r\n");
        self.w.write_all(sent.as_bytes()).await.unwrap();
        let r = self.until(tag).await;
        assert!(r.lines().last().unwrap().contains(" OK "), "{line}: {r}");
        r
    }
}

fn used(net: &mut kevy_client::Connection) -> i64 {
    let key = mailrs_core_sidestate::keys::quota::used_key(USER);
    let v = net.get(key.as_bytes()).unwrap().unwrap_or_default();
    String::from_utf8(v).unwrap().parse().unwrap_or(0)
}

#[tokio::test]
async fn append_copy_move_and_expunge_each_count_once() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let mut net = kevy_client::Connection::connect(KEVY).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("example.test").join("alice");
    for folder in ["", ".Trash", ".Junk"] {
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(home.join(folder).join(leaf)).unwrap();
        }
    }
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let mut state = FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(store));
    state.net_url = Some(KEVY.to_string());
    let hash = mailrs_core::users::UserStore::hash_password(PASSWORD).unwrap();
    let blob = serde_json::json!({ "password_hash": hash }).to_string();
    state.mailbox.upsert_account(USER, &blob).unwrap();
    let dir = Arc::new(FastcoreDirectory::new(
        Arc::new(state),
        tmp.path().to_path_buf(),
    ));

    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir, server));
    let (r, w) = tokio::io::split(client);
    let mut c = Imap {
        r: BufReader::new(r),
        w,
    };
    let mut greeting = String::new();
    c.r.read_line(&mut greeting).await.unwrap();
    c.cmd("a1", &format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    let size = RAW.len() as i64;
    assert_eq!(used(&mut net), 0);

    let append = format!("a2 APPEND INBOX {{{size}}}\r\n");
    c.w.write_all(append.as_bytes()).await.unwrap();
    let mut plus = String::new();
    c.r.read_line(&mut plus).await.unwrap();
    c.w.write_all(format!("{RAW}\r\n").as_bytes())
        .await
        .unwrap();
    c.until("a2").await;
    assert_eq!(used(&mut net), size, "APPEND");

    c.cmd("a3", "SELECT INBOX").await;
    c.cmd("a4", "COPY 1 Trash").await;
    assert_eq!(used(&mut net), 2 * size, "COPY");

    c.cmd("a5", "SELECT Trash").await;
    c.cmd("a6", "MOVE 1 Junk").await;
    assert_eq!(used(&mut net), 2 * size, "MOVE");

    c.cmd("a7", "SELECT Junk").await;
    c.cmd("a8", "STORE 1 +FLAGS.SILENT (\\Deleted)").await;
    c.cmd("a9", "EXPUNGE").await;
    assert_eq!(used(&mut net), size, "EXPUNGE");
}
