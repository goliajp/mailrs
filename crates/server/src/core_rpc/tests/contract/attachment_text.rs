//! Attachment text on both cores: the same mail with a PDF attached is
//! queued on delivery, read by the shared worker over each core's store,
//! and leaves the same text, outcome, number history and search hits.

use std::sync::Arc;

use base64::Engine as _;
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::SearchConversationsRequest;
use mailrs_core_api::types::ConversationFilter;
use mailrs_core_sidestate::families::callback_numbers;
use mailrs_fastcore::attachment_text::KevyAttachmentText;
use mailrs_intake::attachment_text::{AttachmentTextStore, check_attachment_text_store, run_once};

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

use crate::attachment_text::PgAttachmentText;

/// Ids each store mints for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];
const SENDER: &str = "desk@hotel.example";
const WORD: &str = "zebrafinch";

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
    doc.save_to(&mut out).expect("pdf");
    out
}

/// A letter to `to` from the desk, with `attachment` as a PDF when given.
fn letter(mid: &str, to: &str, attachment: Option<&[u8]>) -> Vec<u8> {
    let head = format!(
        "Received: from mx.hotel.example (192.0.2.7:25)\r\n\tby {} with ESMTP\r\n\
         \tfor <{to}>; Mon, 2 Nov 2026 10:00:00 +0000\r\n\
         From: Front Desk <{SENDER}>\r\nTo: {to}\r\nSubject: Your booking {mid}\r\n\
         Message-ID: <{mid}>\r\nDate: Mon, 2 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\n",
        harness::AUTHSERV
    );
    let Some(bytes) = attachment else {
        return format!("{head}Content-Type: text/plain\r\n\r\nno attachment here\r\n")
            .into_bytes();
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64"))
        .collect();
    format!(
        "{head}Content-Type: multipart/mixed; boundary=XB\r\n\r\n\
         --XB\r\nContent-Type: text/plain\r\n\r\nplease see the attached\r\n\
         --XB\r\nContent-Type: application/pdf; name=booking.pdf\r\n\
         Content-Disposition: attachment; filename=booking.pdf\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--XB--\r\n",
        lines.join("\r\n")
    )
    .into_bytes()
}

fn to(rcpts: &[&str]) -> SpoolEnvelope {
    SpoolEnvelope {
        forward_paths: rcpts.iter().map(|r| r.to_string()).collect(),
        ..envelope(None)
    }
}

fn stores(c: &Cores) -> (Arc<KevyAttachmentText>, Arc<PgAttachmentText>) {
    let pg = &c.pg_core_state;
    (
        Arc::new(KevyAttachmentText(c.fastcore_state.clone())),
        Arc::new(PgAttachmentText {
            mailbox: pg.mailbox.clone(),
            maildir_root: pg.maildir_root.clone(),
            net_url: pg.net_url.clone(),
        }),
    )
}

async fn search(c: &Cores, d: &mut Diffs, user: &str) -> usize {
    let req = SearchConversationsRequest {
        query: WORD.into(),
        filter: ConversationFilter {
            limit: 20,
            ..Default::default()
        },
        quarantined: false,
    };
    let found = d.both(
        &format!("search_conversations {WORD} as {user}"),
        c.fastcore.search_conversations(user, &req).await,
        c.pg_core.search_conversations(user, &req).await,
        MINTED,
    );
    found.map(|(fc, _)| fc.items.len()).unwrap_or(0)
}

fn given(url: &str) -> Vec<callback_numbers::Given> {
    let mut conn = kevy_client::Connection::connect(url).expect("kevy");
    callback_numbers::given(&mut conn, ALICE, SENDER).expect("history")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pdf_attachment_is_read_alike_by_both_cores() {
    const FC: &str = "mem://attachment-text-fastcore";
    const PG: &str = "mem://attachment-text-pg-core";
    let c = harness::start_on(FC, PG).await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    let spools = Spools::new(&c);
    let (fc, pg) = stores(&c);
    let mid = "booking-1@hotel.example";
    let body = letter(
        mid,
        ALICE,
        Some(&pdf(&format!(
            "{WORD}: to change it, call us on 03-1234-5678"
        ))),
    );

    spools.arrive(&c, &to(&[ALICE]), &body).await;
    assert!(
        fc.queued(ALICE, mid).await.unwrap(),
        "fastcore did not queue"
    );
    assert!(
        pg.queued(ALICE, mid).await.unwrap(),
        "pg-core did not queue"
    );
    let rounds = (run_once(&fc).await, run_once(&pg).await);
    assert_eq!(rounds.0, rounds.1, "the rounds differ");
    assert_eq!((rounds.0.extracted, rounds.0.stored), (1, 1));

    let texts = (fc.texts(mid).await.unwrap(), pg.texts(mid).await.unwrap());
    assert!(texts.0.len() == 1 && texts.0[0].contains(WORD), "{texts:?}");
    assert_eq!(texts.0, texts.1, "the stored text differs");
    let state = (fc.state(mid).await.unwrap(), pg.state(mid).await.unwrap());
    assert!(state.0.is_some());
    assert_eq!(state.0, state.1, "the stored outcome differs");
    let qr = (
        fc.qr_urls(mid).await.unwrap(),
        pg.qr_urls(mid).await.unwrap(),
    );
    assert_eq!(qr.0, qr.1, "the QR addresses differ");
    let history = given(FC);
    assert_eq!(
        history
            .iter()
            .map(|g| g.number.as_str())
            .collect::<Vec<_>>(),
        ["0312345678"],
        "the number in the attachment was not kept"
    );
    assert_eq!(history, given(PG), "the cores kept different histories");
    assert_eq!(search(&c, &mut d, ALICE).await, 1, "alice's search");

    // the same message for a second reader, after it was read: not read
    // again, and found in the second reader's search too
    let again = letter(mid, BOB, Some(&pdf(&format!("{WORD}: nothing new"))));
    spools.arrive(&c, &to(&[BOB]), &again).await;
    let rounds = (run_once(&fc).await, run_once(&pg).await);
    assert_eq!(rounds.0, rounds.1, "the rounds differ");
    assert_eq!((rounds.0.already_done, rounds.0.extracted), (1, 0));
    assert_eq!(search(&c, &mut d, BOB).await, 1, "bob's search");
    assert!(fc.pending(10).await.unwrap().is_empty());
    assert!(pg.pending(10).await.unwrap().is_empty());

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_stores_keep_attachment_text_as_the_contract_says() {
    let c = harness::start_on(
        "mem://attachment-store-fastcore",
        "mem://attachment-store-pg-core",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    let spools = Spools::new(&c);
    let mid = "plain-1@hotel.example";
    spools
        .arrive(&c, &to(&[ALICE, BOB]), &letter(mid, ALICE, None))
        .await;
    let (fc, pg) = stores(&c);
    check_attachment_text_store(&*fc, ALICE, BOB, mid).await;
    check_attachment_text_store(&*pg, ALICE, BOB, mid).await;
}
