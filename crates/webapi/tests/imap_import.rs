//! An IMAP import against a real IMAP server — this deployment's own,
//! serving another account — through the same filing as an mbox upload.
//! The address check is the one part replaced: the server is on loopback,
//! which the import refuses, so the test hands the job its own way in.

mod live;

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::types::ConversationFilter;
use mailrs_webapi::handlers::imap_import::connect::{Connect, Opened};
use mailrs_webapi::handlers::imap_import::job::{self, Refused, Source, State, Status};

const KEVY: &str = "mem://webapi-imap-import";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

/// Straight to the test server, in clear.
struct Plain(SocketAddr);

impl Connect for Plain {
    fn connect<'a>(
        &'a self,
        _: &'a str,
        _: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>> {
        Box::pin(async move {
            let tcp = tokio::net::TcpStream::connect(self.0)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Opened {
                stream: Box::new(tcp),
                greeted: false,
            })
        })
    }
}

/// A server that cannot be reached, after `wait` lets it go.
struct Unreachable(Arc<tokio::sync::Notify>);

impl Connect for Unreachable {
    fn connect<'a>(
        &'a self,
        _: &'a str,
        _: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>> {
        Box::pin(async move {
            self.0.notified().await;
            Err("<b>no route</b> to host".to_string())
        })
    }
}

fn mail(id: &str, subject: &str, from: &str) -> Vec<u8> {
    format!(
        "Date: Mon, 02 Jan 2023 09:00:00 +0000\r\nFrom: {from}\r\nTo: Bob <bob@example.test>\r\n\
Subject: {subject}\r\nMessage-ID: <{id}>\r\nMIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\nbody of {subject}\r\n"
    )
    .into_bytes()
}

fn source(port: u16) -> Source {
    Source {
        host: "imap.old.test".into(),
        port,
        username: BOB.into(),
        password: "pw".into(),
    }
}

async fn finished(user: &str) -> Status {
    for _ in 0..300 {
        let s = job::status(user);
        if !matches!(s.state, State::Queued | State::Running) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "the import of {user} did not finish: {:?}",
        job::status(user)
    );
}

async fn subjects(live: &live::Live, user: &str, folder: &str) -> Vec<String> {
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some(folder.into()),
            limit: 50,
            ..Default::default()
        },
    };
    let mut s: Vec<String> = live
        .core
        .list_conversations(user, &req)
        .await
        .expect("list")
        .items
        .into_iter()
        .map(|c| c.subject)
        .collect();
    s.sort();
    s
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_imap_import_files_folders_resumes_and_is_limited() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;

    // Bob's mailbox, as the other provider holds it
    for (id, subject) in [
        ("r1@old.test", "Read one"),
        ("u1@old.test", "Unread one"),
        ("t1@old.test", "Thrown away"),
    ] {
        live.core
            .import_message(BOB, &mail(id, subject, "Erin <erin@oldmail.net>"), None)
            .await
            .expect("seed");
    }
    live.core
        .file_sent(
            BOB,
            &mail("s1@old.test", "Lunch?", "Bob <bob@example.test>"),
            &["carol@partner.test".into()],
        )
        .await
        .expect("seed sent");
    let read = live
        .core
        .find_by_message_id_for_user(BOB, "r1@old.test")
        .await
        .unwrap();
    live.core
        .set_message_flags(BOB, read.uid, &SetMessageFlagsRequest { flags: FLAG_SEEN })
        .await
        .unwrap();
    live.core.trash_thread(BOB, "t1@old.test").await.unwrap();

    // this deployment's IMAP server, serving Bob
    let mut fc = mailrs_fastcore::FastcoreState::new(live.mailbox.clone());
    fc.net_url = Some(KEVY.to_string());
    let dir = mailrs_fastcore::FastcoreDirectory::new(Arc::new(fc), tmp.path().to_path_buf());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(mailrs_mail_protocols::imap::serve_plain(
        listener,
        Arc::new(dir),
        None,
    ));
    let plain: Arc<dyn Connect> = Arc::new(Plain(addr));

    // 1. the whole mailbox: Inbox and Sent filed, the Trash left behind
    job::start(live.state.clone(), ALICE, source(993), plain.clone()).expect("start");
    let s = finished(ALICE).await;
    assert_eq!(s.state, State::Done, "{s:?}");
    assert_eq!(
        (s.report.imported, s.report.sent, s.report.failed),
        (2, 1, 0),
        "{s:?}"
    );
    let inbox = subjects(&live, ALICE, "Inbox").await;
    assert!(
        inbox.contains(&"Read one".to_string()) && inbox.contains(&"Unread one".to_string()),
        "{inbox:?}"
    );
    assert_eq!(s.report.held, 0, "{s:?}");
    // filed as a sent copy. Written from Bob's old address, it is not
    // Alice's by its From, which is what the Sent view reads
    let sent = live
        .core
        .find_by_message_id_for_user(ALICE, "s1@old.test")
        .await
        .unwrap();
    assert!(sent.blob_ref.starts_with(".Sent/"), "{}", sent.blob_ref);
    let r = live
        .core
        .find_by_message_id_for_user(ALICE, "r1@old.test")
        .await
        .unwrap();
    let u = live
        .core
        .find_by_message_id_for_user(ALICE, "u1@old.test")
        .await
        .unwrap();
    assert_eq!((r.flags & FLAG_SEEN, u.flags & FLAG_SEEN), (FLAG_SEEN, 0));
    assert!(
        live.core
            .find_by_message_id_for_user(ALICE, "t1@old.test")
            .await
            .is_err()
    );
    // finished: nothing to resume
    assert_eq!(job::status(ALICE).state, State::Done);

    // 2. again: everything is already here
    job::start(live.state.clone(), ALICE, source(993), plain.clone()).expect("start");
    let s = finished(ALICE).await;
    assert_eq!(
        (s.report.imported, s.report.sent, s.report.skipped_duplicate),
        (0, 0, 3),
        "{s:?}"
    );

    // 3. resumed: the Inbox is recorded as done, so only Sent is read
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    use mailrs_core_sidestate::families::imap_import as progress;
    progress::begin(&mut kv, ALICE, "imap.old.test", 993, BOB).unwrap();
    progress::record(&mut kv, ALICE, "INBOX", u32::MAX - 1).unwrap();
    job::start(live.state.clone(), ALICE, source(993), plain.clone()).expect("start");
    let s = finished(ALICE).await;
    assert_eq!(s.report.skipped_duplicate, 1, "only Sent was read: {s:?}");

    // 4. a server that cannot be reached: the reason, as plain text, and
    // the start still counts towards the day's five
    let gate = Arc::new(tokio::sync::Notify::new());
    gate.notify_one();
    job::start(
        live.state.clone(),
        ALICE,
        source(993),
        Arc::new(Unreachable(gate.clone())),
    )
    .expect("start");
    let s = finished(ALICE).await;
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.error.as_deref(), Some("<b>no route</b> to host"));
    gate.notify_one();
    job::start(
        live.state.clone(),
        ALICE,
        source(993),
        Arc::new(Unreachable(gate.clone())),
    )
    .expect("start");
    finished(ALICE).await;
    assert_eq!(
        job::start(live.state.clone(), ALICE, source(993), plain.clone()).err(),
        Some(Refused::TooManyToday)
    );

    // 5. two at a time server-wide; a third waits, a second for one user is refused
    let held = Arc::new(tokio::sync::Notify::new());
    let users = ["q1@example.test", "q2@example.test", "q3@example.test"];
    for u in users {
        job::start(
            live.state.clone(),
            u,
            source(993),
            Arc::new(Unreachable(held.clone())),
        )
        .expect("start");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let states: Vec<State> = users.iter().map(|u| job::status(u).state).collect();
    assert_eq!(
        states.iter().filter(|s| **s == State::Running).count(),
        2,
        "{states:?}"
    );
    assert_eq!(
        states.iter().filter(|s| **s == State::Queued).count(),
        1,
        "{states:?}"
    );
    assert_eq!(
        job::start(
            live.state.clone(),
            "q1@example.test",
            source(993),
            plain.clone()
        )
        .err(),
        Some(Refused::AlreadyRunning)
    );
    for _ in 0..3 {
        held.notify_waiters();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    for u in users {
        assert_eq!(finished(u).await.state, State::Failed);
    }
}
