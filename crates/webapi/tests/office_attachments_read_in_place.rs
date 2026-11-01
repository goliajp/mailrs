//! A Word attachment reads in place: the attachment-content route answers
//! with the document's paragraphs instead of an empty "download it".

mod live;

use std::io::Write;

use base64::Engine as _;
use reqwest::Method;

const KEVY: &str = "mem://webapi-office-attachments-read-in-place";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-office@example.test";
const FILE: &str = "1786000100.M1P1.host";

fn docx(paragraphs: &[&str]) -> Vec<u8> {
    let body: String = paragraphs
        .iter()
        .map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>"))
        .collect();
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    write!(zip, "<w:document><w:body>{body}</w:body></w:document>").unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_word_attachment_reads_as_its_paragraphs() {
    let tmp = tempfile::tempdir().expect("tmp");
    let inbox = tmp.path().join("example.test/alice");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(inbox.join(leaf)).expect("maildir");
    }
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(docx(&["Quarterly report", "Revenue rose 12%"]));
    std::fs::write(
        inbox.join("cur").join(format!("{FILE}:2,S")),
        format!(
            "From: bob@partner.example\r\nTo: {ALICE}\r\nSubject: report\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nattached\r\n\
--b\r\nContent-Type: application/octet-stream; name=report.docx\r\n\
Content-Disposition: attachment; filename=report.docx\r\n\
Content-Transfer-Encoding: base64\r\n\r\n{encoded}\r\n--b--\r\n"
        ),
    )
    .expect("message");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_100,
            latest_preview: "attached",
            senders_csv: "bob@partner.example",
            subject: "report",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let mid = "m-office@example.test";
    let payload = serde_json::json!({
        "uid": 9, "sender": "bob@partner.example", "recipients": ALICE,
        "subject": "report", "date": 1_786_000_100, "internal_date": 1_786_000_100,
        "size": 900, "flags": 1, "message_id": mid, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    live.mailbox
        .upsert_user_message(
            ALICE,
            TID,
            mid,
            1_786_000_100,
            payload.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: FILE,
                uid: 9,
                flags: 1,
                modseq: 1,
            },
        )
        .expect("message row");
    live.mailbox.register_uid(ALICE, 9, mid).expect("uid");

    let (status, got) = live
        .call(
            Method::GET,
            "/api/mail/messages/9/attachments/0/content",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(
        got["extracted_text"], "Quarterly report\nRevenue rose 12%",
        "{got}"
    );
    assert_eq!(got["sections"][0]["lines"][1], "Revenue rose 12%", "{got}");
}
