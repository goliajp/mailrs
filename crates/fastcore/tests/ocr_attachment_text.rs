//! Text in images and scanned PDFs becomes searchable: delivery queues the
//! message, the worker hands each image or scan to the real
//! `mailrs-fastcore-ocr` child, and search finds the words.
//!
//! Needs the OCR models: run `scripts/fetch-ocr-models.sh` first, or point
//! `MAILRS_OCR_MODEL_DIR` at them. One test, because the worker reads its
//! OCR setup from the environment once per process.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use lopdf::{Document, Object, Stream, dictionary};
use tower::ServiceExt;

const DOMAIN: &str = "golia.example";
const BOB: &str = "bob@golia.example";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../ocr/tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn models() -> PathBuf {
    let dir = std::env::var_os("MAILRS_OCR_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ocr-models")
        });
    assert!(
        dir.join("rec.onnx").is_file(),
        "no OCR models in {}: run scripts/fetch-ocr-models.sh",
        dir.display()
    );
    dir
}

fn b64(bytes: &[u8]) -> String {
    let s = base64::engine::general_purpose::STANDARD.encode(bytes);
    s.as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn head(mid: &str, ctype: &str) -> String {
    format!(
        "From: a@example.org\r\nTo: {BOB}\r\nSubject: papers {mid}\r\nMessage-ID: <{mid}>\r\n\
         Date: Mon, 19 Oct 2026 09:00:00 +0900\r\nMIME-Version: 1.0\r\nContent-Type: {ctype}\r\n\r\n"
    )
}

/// A letter with one attachment.
fn attached(mid: &str, ctype: &str, name: &str, bytes: &[u8]) -> String {
    format!(
        "{}--XB\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
         --XB\r\nContent-Type: {ctype}; name={name}\r\nContent-Disposition: attachment; filename={name}\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--XB--\r\n",
        head(mid, "multipart/mixed; boundary=XB"),
        b64(bytes)
    )
}

/// An HTML letter showing `png` inline through `cid:`.
fn inline(mid: &str, png: &[u8]) -> String {
    format!(
        "{}--XB\r\nContent-Type: text/html\r\n\r\n<p>hello</p><img src=\"cid:pic1\">\r\n\
         --XB\r\nContent-Type: image/png\r\nContent-ID: <pic1>\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--XB--\r\n",
        head(mid, "multipart/related; boundary=XB"),
        b64(png)
    )
}

/// Two pages: one with real text, one that is only a JPEG of `png`.
fn half_scanned_pdf(png: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png).unwrap().to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&img)
        .unwrap();
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let text = "BT /F1 12 Tf 72 700 Td (cover letter with enough embedded words to count as a real page of text) Tj ET";
    let p1 = doc.add_object(Stream::new(dictionary! {}, text.as_bytes().to_vec()));
    let mut xobj = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        },
        jpeg,
    );
    xobj.allows_compression = false;
    let im = doc.add_object(xobj);
    let (pw, ph) = (w * 72 / 150, h * 72 / 150);
    let p2 = doc.add_object(Stream::new(
        dictionary! {},
        format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q").into_bytes(),
    ));
    let page = |contents, res: lopdf::Dictionary, doc: &mut Document| {
        doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => contents, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
        })
    };
    let a = page(
        p1,
        dictionary! { "Font" => dictionary! { "F1" => font } },
        &mut doc,
    );
    let b = page(
        p2,
        dictionary! { "XObject" => dictionary! { "Im0" => im } },
        &mut doc,
    );
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![a.into(), b.into()], "Count" => 2 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// A PNG whose header claims 20000 x 20000 pixels: a decompression bomb
/// as far as anything that trusts the header is concerned.
fn bomb_png() -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbImage::new(1, 1)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    // IHDR data starts at byte 16: width, height, then the chunk's CRC
    png[16..20].copy_from_slice(&20000u32.to_be_bytes());
    png[20..24].copy_from_slice(&20000u32.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png.extend(std::iter::repeat_n(0u8, 4096));
    png
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
        }
    }
    !c
}

async fn search(state: &Arc<mailrs_fastcore::FastcoreState>, word: &str) -> Vec<String> {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/users/{BOB}/conversations:search"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "query": word }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let mut ids: Vec<String> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["thread_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn words_in_images_and_scans_find_the_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let incoming = tmp.path().join("spool/incoming/new");
    std::fs::create_dir_all(&incoming).unwrap();
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir.join(DOMAIN).join("bob").join(sub)).unwrap();
    }
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_LOCAL_DOMAINS", DOMAIN);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_OCR_BIN", env!("CARGO_BIN_EXE_mailrs-fastcore-ocr"));
        std::env::set_var("MAILRS_OCR_MODEL_DIR", models());
    }
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.upsert_account(BOB, "{}").unwrap();
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let mixed = fixture("mixed.png");
    let vertical = fixture("vertical.png");
    let mails = [
        (
            "img1@example.org",
            attached("img1@example.org", "image/png", "invoice.png", &mixed),
        ),
        ("cid1@example.org", inline("cid1@example.org", &vertical)),
        (
            "scan1@example.org",
            attached(
                "scan1@example.org",
                "application/pdf",
                "scan.pdf",
                &half_scanned_pdf(&mixed),
            ),
        ),
        (
            "bad1@example.org",
            attached(
                "bad1@example.org",
                "image/jpeg",
                "broken.jpg",
                &[0xAB; 8192],
            ),
        ),
        (
            "bomb1@example.org",
            attached("bomb1@example.org", "image/png", "huge.png", &bomb_png()),
        ),
    ];
    for (i, (_, raw)) in mails.iter().enumerate() {
        let env = mailrs_core::spool::SpoolEnvelope {
            reverse_path: "a@example.org".into(),
            forward_paths: vec![BOB.into()],
            is_authenticated: false,
            auth_user: None,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 1,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
        };
        std::fs::write(
            incoming.join(format!("1790000000.M{i}P1.ocr")),
            mailrs_core::spool::encode_spool_blob(&env, raw.as_bytes()),
        )
        .unwrap();
    }
    assert_eq!(
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state).0,
        5
    );
    let m = &state.mailbox;
    for (mid, _) in &mails {
        assert!(
            m.attachment_text_queued(BOB, mid).unwrap(),
            "{mid} was not queued"
        );
    }
    assert!(search(&state, "增值税").await.is_empty());

    let round = mailrs_fastcore::attachment_text::run_once(&state).await;
    assert_eq!(round.taken, 5, "{round:?}");
    assert_eq!((round.stored, round.ocr), (3, 3), "{round:?}");
    assert_eq!(round.failed, 2, "the broken image and the bomb: {round:?}");

    // Chinese, English, vertical Japanese, and text from a scanned page
    assert_eq!(
        search(&state, "增值税").await,
        ["img1@example.org", "scan1@example.org"]
    );
    assert_eq!(
        search(&state, "remit").await,
        ["img1@example.org", "scan1@example.org"]
    );
    assert_eq!(search(&state, "ゴリア").await, ["cid1@example.org"]);
    // the scan's embedded page is kept beside what OCR read
    assert_eq!(search(&state, "cover").await, ["scan1@example.org"]);

    let scan = m
        .attachment_text_state("scan1@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(
        (scan.version, scan.pdfs, scan.stored, scan.ocr),
        (2, 1, 1, 1)
    );
    let bomb = m
        .attachment_text_state("bomb1@example.org")
        .unwrap()
        .unwrap();
    assert_eq!((bomb.images, bomb.failed, bomb.stored), (1, 1, 0));

    // A message read before OCR existed is read again by the backfill
    m.store_attachment_texts(
        "img1@example.org",
        &[],
        &mailrs_mailbox_kevy::AttachmentTextState {
            version: 1,
            parts: 1,
            images: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(search(&state, "remit").await == ["scan1@example.org"]);
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:backfill-attachment-text?dry_run=false")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        (report["queued"].as_u64(), report["outdated"].as_u64()),
        (Some(1), Some(1)),
        "{report}"
    );
    assert_eq!(report["version"], 2, "{report}");
    assert_eq!(
        mailrs_fastcore::attachment_text::run_once(&state).await.ocr,
        1
    );
    assert_eq!(
        search(&state, "remit").await,
        ["img1@example.org", "scan1@example.org"]
    );
}
