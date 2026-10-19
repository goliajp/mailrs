//! Text inside a PDF attachment becomes searchable, end to end: a real
//! receiver takes the message over SMTP, fastcore drains the spool, and
//! the text worker reads the PDF.
//!
//! One test for the receiver path, because the receiver reads its
//! configuration from the environment once per process.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tower::ServiceExt;

const KEVY: &str = "mem://pdf-attachment-text";
const BOB: &str = "bob@golia.example";
const CAROL: &str = "carol@golia.example";
const WORD: &str = "zebrafinch";
const PROBE: &str = "probe-attachment-offer";
const OFFER: &str = "the sum of US$10,200,000.00 split equally, 50% apiece";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A one-page PDF whose only text is `text`, in Helvetica.
fn pdf(text: &str) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let content = format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET");
    let contents = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Contents" => contents,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// A message with `attachment` as a PDF part named `name`, or a plain
/// letter when there is none.
fn message(mid: &str, attachment: Option<(&str, &str, &[u8])>) -> String {
    let head = format!(
        "From: Sender <a@example.org>\r\nTo: {BOB}\r\nSubject: papers {mid}\r\n\
         Message-ID: <{mid}>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0900\r\nMIME-Version: 1.0\r\n"
    );
    let Some((ctype, name, bytes)) = attachment else {
        return format!("{head}Content-Type: text/plain\r\n\r\nno attachment here\r\n");
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "{head}Content-Type: multipart/mixed; boundary=XB\r\n\r\n\
         --XB\r\nContent-Type: text/plain\r\n\r\nplease see the attached\r\n\
         --XB\r\nContent-Type: {ctype}; name={name}\r\n\
         Content-Disposition: attachment; filename={name}\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--XB--\r\n",
        lines.join("\r\n")
    )
}

struct Smtp {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl Smtp {
    async fn connect(port: u16) -> Self {
        for _ in 0..200 {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)).await {
                let (r, w) = s.into_split();
                let mut smtp = Self {
                    reader: BufReader::new(r),
                    writer: w,
                };
                smtp.reply().await;
                return smtp;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("receiver never listened");
    }

    async fn reply(&mut self) -> String {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            assert!(!line.is_empty(), "connection closed");
            if line.as_bytes().get(3) != Some(&b'-') {
                return line.trim_end().to_string();
            }
        }
    }

    async fn cmd(&mut self, line: &str) -> String {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }

    async fn send(&mut self, to: &str, body: &str) {
        assert!(
            self.cmd("MAIL FROM:<a@example.org>")
                .await
                .starts_with("250")
        );
        assert!(
            self.cmd(&format!("RCPT TO:<{to}>"))
                .await
                .starts_with("250")
        );
        assert!(self.cmd("DATA").await.starts_with("354"));
        let r = self.cmd(&format!("{body}.")).await;
        assert!(r.starts_with("250"), "{r}");
    }
}

async fn call(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    uri: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// What a judgement could have changed about the PDF conversation.
fn judged(m: &mailrs_mailbox_kevy::KevyMailboxStore) -> (Option<String>, bool, String) {
    let row = m
        .get_thread_for_user(BOB, "pdf1@example.org")
        .unwrap()
        .unwrap();
    (
        m.fraud_verdict("pdf1@example.org").unwrap(),
        row.quarantined,
        row.category,
    )
}

async fn search(state: &Arc<mailrs_fastcore::FastcoreState>, user: &str) -> Vec<String> {
    let found = call(
        state,
        &format!("/v1/users/{user}/conversations:search"),
        serde_json::json!({ "query": WORD }),
    )
    .await;
    found["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["thread_id"].as_str().unwrap().to_string())
        .collect()
}

async fn rescan_dry(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    call(
        state,
        "/v1/admin/maintenance:fraud-rescan?dry_run=true",
        serde_json::json!({}),
    )
    .await
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>, dry: bool) -> serde_json::Value {
    call(
        state,
        &format!("/v1/admin/maintenance:backfill-attachment-text?dry_run={dry}"),
        serde_json::json!({}),
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_word_inside_a_pdf_finds_the_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let incoming = tmp.path().join("spool/incoming/new");
    let smtp_port = free_port();

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    for user in [BOB, CAROL] {
        mailbox.upsert_account(user, "{}").unwrap();
        let local = user.split('@').next().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(maildir.join("golia.example").join(local).join(sub)).unwrap();
        }
    }
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    let router = mailrs_fastcore::build_router(state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await });

    // The shipped rules plus one that reads the new fact and would move the
    // conversation to Junk: a re-judge after extraction would show, and the
    // sweep's reading of the stored text can be seen in its report.
    let rules = tmp.path().join("rules.lua");
    std::fs::write(
        &rules,
        format!(
            "{}\nrule('{PROBE}', 'content', 5, false, function(m)\n\
             if m.attachment_offers_the_reader_a_sum then return 'offer in a pdf' end\nend)\n",
            mailrs_fraud_lua::DEFAULT_SOURCE
        ),
    )
    .unwrap();

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_CORE_RPC_BASE", &core);
        std::env::set_var("MAILRS_CORE_API_SECRET", "secret");
        std::env::set_var("MAILRS_SPOOL_ROOT", tmp.path().join("spool"));
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_PORT", smtp_port.to_string());
        std::env::set_var("MAILRS_SUBMISSION_PORT", free_port().to_string());
        std::env::set_var("MAILRS_SMTPS_PORT", free_port().to_string());
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
        std::env::set_var("MAILRS_FRAUD_RULES_FILE", &rules);
        // OCR off, so the corrupt PDF fails here rather than going to a
        // renderer; the OCR path has its own test
        std::env::set_var("MAILRS_OCR_BIN", "/nonexistent/mailrs-fastcore-ocr");
    }
    tokio::spawn(mailrs_receiver::run());

    let good = pdf(&format!("contract {WORD}: {OFFER}"));
    let mut smtp = Smtp::connect(smtp_port).await;
    assert!(smtp.cmd("EHLO sender.example.org").await.starts_with("250"));
    smtp.send(
        BOB,
        &message(
            "pdf1@example.org",
            Some(("application/pdf", "contract.pdf", &good)),
        ),
    )
    .await;
    smtp.send(BOB, &message("plain1@example.org", None)).await;
    // octet-stream is what some mailers call every attachment
    smtp.send(
        BOB,
        &message(
            "bad1@example.org",
            Some((
                "application/octet-stream",
                "scan.PDF",
                b"%PDF-1.4 not really",
            )),
        ),
    )
    .await;

    let drained =
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state).0;
    assert_eq!(drained, 3);

    // delivery queued the two PDFs and not the letter
    let m = &state.mailbox;
    assert!(m.attachment_text_queued(BOB, "pdf1@example.org").unwrap());
    assert!(m.attachment_text_queued(BOB, "bad1@example.org").unwrap());
    assert!(!m.attachment_text_queued(BOB, "plain1@example.org").unwrap());

    // the word is only inside the PDF, so nothing finds it yet
    assert!(search(&state, BOB).await.is_empty());
    let before = rescan_dry(&state).await;
    assert!(before["scored_by_reason"].get(PROBE).is_none(), "{before}");
    let judged_before = judged(m);

    let round = mailrs_fastcore::attachment_text::run_once(&state).await;
    assert_eq!(round.taken, 2, "{round:?}");
    assert_eq!(round.extracted, 2, "{round:?}");
    assert_eq!(round.stored, 1, "{round:?}");
    assert_eq!(
        round.failed, 1,
        "the corrupt PDF was not recorded as failed: {round:?}"
    );
    assert!(
        m.pending_attachment_texts(10).unwrap().is_empty(),
        "items stayed queued"
    );
    let bad = m
        .attachment_text_state("bad1@example.org")
        .unwrap()
        .unwrap();
    assert_eq!((bad.pdfs, bad.failed, bad.stored), (1, 1, 0));

    // found for the recipient, and for nobody else
    let pdf_thread = vec!["pdf1@example.org".to_string()];
    assert_eq!(search(&state, BOB).await, pdf_thread);
    assert!(search(&state, CAROL).await.is_empty());

    // extraction judged nothing: verdict, hold and folder are as they were,
    // though the probe rule would have junked the conversation
    assert_eq!(judged(m), judged_before);
    // and the sweep, which a rule change runs, reads the stored text
    let after = rescan_dry(&state).await;
    assert_eq!(after["scored_by_reason"][PROBE], 1, "{after}");

    // an empty queue costs nothing, and the wait grows to its cap
    let idle = mailrs_fastcore::attachment_text::run_once(&state).await;
    assert_eq!(idle, Default::default());
    let wait = mailrs_fastcore::attachment_text::wait_after;
    assert!(wait(1) > wait(0) && wait(2) > wait(1));
    assert_eq!(wait(50), wait(5));

    // mail from before the worker existed: delivered, never queued
    let old = pdf(&format!("old minutes {WORD}"));
    smtp.send(
        BOB,
        &message(
            "old1@example.org",
            Some(("application/pdf", "minutes.pdf", &old)),
        ),
    )
    .await;
    assert_eq!(
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state).0,
        1
    );
    m.dequeue_attachment_text(BOB, "old1@example.org").unwrap();

    let dry = backfill(&state, true).await;
    assert_eq!(dry["messages_walked"], 4, "{dry}");
    assert_eq!(dry["files_read"], 4, "{dry}");
    assert_eq!(dry["with_readable"], 3, "{dry}");
    assert_eq!(dry["already_done"], 2, "{dry}");
    assert_eq!(dry["already_done_failed"], 1, "{dry}");
    assert_eq!(dry["queued"], 0, "{dry}");
    assert!(!m.attachment_text_queued(BOB, "old1@example.org").unwrap());
    let wet = backfill(&state, false).await;
    assert_eq!(wet["queued"], 1, "{wet}");
    assert_eq!(backfill(&state, false).await["already_queued"], 1);
    assert_eq!(
        mailrs_fastcore::attachment_text::run_once(&state)
            .await
            .stored,
        1
    );
    let mut both = search(&state, BOB).await;
    both.sort();
    assert_eq!(
        both,
        vec![
            "old1@example.org".to_string(),
            "pdf1@example.org".to_string()
        ]
    );

    // deleting the conversation takes its attachment text with it
    assert!(m.delete_thread(BOB, "pdf1@example.org").unwrap().0);
    assert!(m.attachment_texts("pdf1@example.org").unwrap().is_empty());
    assert_eq!(m.attachment_text_state("pdf1@example.org").unwrap(), None);
    assert_eq!(
        search(&state, BOB).await,
        vec!["old1@example.org".to_string()]
    );
}
