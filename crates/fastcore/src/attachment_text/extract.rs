//! Which parts carry readable text, and reading it: Office files and
//! embedded PDF text in a parse worker of their own, everything that
//! needs OCR in the OCR child.

use mailrs_mailbox_kevy::AttachmentTextState;
use mailrs_mailbox_kevy::keys::{ATTACHMENT_TEXT_CAP, cap_text};

use super::ocr::{Job, OcrSetup};

/// A PDF over this many bytes is not read for its embedded text. The
/// extractor keeps the whole document in memory, more than once over.
pub const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
/// Largest PDF handed to OCR, which renders in its own process.
pub const MAX_OCR_PDF_BYTES: usize = 40 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
/// Below this an image is an icon or a spacer.
const MIN_IMAGE_BYTES: usize = 2 * 1024;
/// A page with fewer visible characters than this is a scan.
const THIN_PAGE: usize = 50;
/// Pages of one PDF that go through OCR.
const MAX_OCR_PAGES: usize = 20;

/// Extractor generations, stored with each message's outcome. Independent
/// things: how far PDFs and images were read (the low bits: 1 embedded PDF
/// text only, 2 with OCR), whether Office files were read (the
/// `WITH_OFFICE` bit), and whether the QR codes' addresses were stored
/// (the `WITH_QR` bit; the OCR child reads codes, so only a process with
/// OCR sets it). A message is read again when any falls short of what
/// this process does; see `covers`.
pub const VERSION_PDF_ONLY: u32 = 1;
pub const VERSION_WITH_OCR: u32 = 2;
pub const WITH_OFFICE: u32 = 4;
pub const WITH_QR: u32 = 8;

const FLAGS: [u32; 2] = [WITH_OFFICE, WITH_QR];

/// Whether a message read at `done` already has everything a read at `now`
/// would give. Not `done >= now`: a process without OCR would then read a
/// message again that was read with OCR, and replace its text with less.
pub(crate) fn covers(done: u32, now: u32) -> bool {
    let all = FLAGS.iter().fold(0, |acc, f| acc | f);
    FLAGS.iter().all(|f| done & f >= now & f) && done & !all >= now & !all
}

/// Whether a message read at `done` lacks only its QR codes: its text is
/// current, and reading the codes alone brings it up to `now`.
pub(crate) fn lacks_only_qr(done: u32, now: u32) -> bool {
    !covers(done, now) && covers(done | WITH_QR, now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Pdf,
    Image,
    Office(mailrs_attachment_extract::office::OfficeKind),
}

const IMAGE_TYPES: [&str; 8] = [
    "png", "jpeg", "jpg", "gif", "webp", "tiff", "bmp", "x-ms-bmp",
];
const IMAGE_NAMES: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".tif", ".tiff", ".bmp",
];

/// The message's PDFs, images and Office files, in MIME order. Images shown inline
/// through `cid:` count as much as attached ones.
///
/// `application/octet-stream` counts when the file name says what it is:
/// some mailers give every attachment that type.
pub(crate) fn readable_parts<'p, 'a>(
    parsed: &'p mailrs_mime::Part<'a>,
) -> impl Iterator<Item = (Kind, &'p mailrs_mime::Part<'a>)> {
    parsed
        .walk()
        .filter(|p| p.children.is_empty())
        .filter_map(|p| {
            let ct = &p.content_type;
            let name = p
                .attachment_filename()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if let Some(office) =
                mailrs_attachment_extract::office::office_kind(&ct.mime_type(), &name)
            {
                return Some((Kind::Office(office), p));
            }
            match (ct.type_.as_str(), ct.subtype.as_str()) {
                ("application", "pdf") => Some((Kind::Pdf, p)),
                ("image", sub) if IMAGE_TYPES.contains(&sub) => Some((Kind::Image, p)),
                ("application", "octet-stream") if name.ends_with(".pdf") => Some((Kind::Pdf, p)),
                ("application", "octet-stream")
                    if IMAGE_NAMES.iter().any(|e| name.ends_with(e)) =>
                {
                    Some((Kind::Image, p))
                }
                _ => None,
            }
        })
}

/// Whether `raw` can hold a readable part at all, checked on the bytes
/// before a full MIME parse. An octet-stream part counts: its file name
/// may be an encoded word that hides the extension.
pub(crate) fn may_carry_readable(raw: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 8] = [
        b"application/pdf",
        b"application/octet-stream",
        b".pdf",
        b"image/",
        b"officedocument",
        b".docx",
        b".xlsx",
        b".pptx",
    ];
    NEEDLES.iter().any(|n| contains_ignore_case(raw, n))
}

fn contains_ignore_case(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// What reading one part came to.
enum Got {
    Text { text: String, by_ocr: bool },
    Nothing,
    TooLarge,
    TooSmall,
    Failed,
}

/// Read every readable part of `raw`. Blocking: CPU-bound and unbounded
/// in time on a hostile document, so callers run it off the async runtime.
/// Without `ocr`, images are passed over and scans keep only what text
/// they embed; the outcome then says [`VERSION_PDF_ONLY`] (with [`WITH_OFFICE`]).
///
/// Returns each stored text with its position among the readable parts,
/// the counts that say what happened to the rest, and the web addresses
/// of the QR codes OCR found.
pub(crate) fn extract_all(
    raw: &[u8],
    ocr: Option<&OcrSetup>,
) -> (Vec<(usize, String)>, AttachmentTextState, Vec<String>) {
    let parsed = mailrs_mime::parse(raw);
    let mut texts = Vec::new();
    let mut qr = Vec::new();
    let mut state = AttachmentTextState {
        version: VERSION_PDF_ONLY | WITH_OFFICE,
        ..Default::default()
    };
    if ocr.is_some() {
        state.version = VERSION_WITH_OCR | WITH_OFFICE | WITH_QR;
    }
    for (n, (kind, part)) in readable_parts(&parsed).enumerate() {
        state.parts += 1;
        let got = match kind {
            Kind::Pdf => {
                state.pdfs += 1;
                read_pdf(&part.body, ocr, n, &mut qr)
            }
            Kind::Image => {
                state.images += 1;
                read_image(&part.body, ocr, n, &mut qr)
            }
            Kind::Office(kind) => read_office(kind, &part.body),
        };
        match got {
            Got::Text { text, by_ocr } => {
                let capped = cap_text(text.trim(), ATTACHMENT_TEXT_CAP);
                if capped.len() < text.trim().len() {
                    state.truncated += 1;
                }
                state.stored += 1;
                state.ocr += u32::from(by_ocr);
                texts.push((n, capped.to_string()));
            }
            Got::Nothing => {}
            Got::TooLarge => state.too_large += 1,
            Got::TooSmall => state.too_small += 1,
            Got::Failed => state.failed += 1,
        }
    }
    (texts, state, qr)
}

/// An Office file's text, in a parse worker of its own.
fn read_office(kind: mailrs_attachment_extract::office::OfficeKind, body: &[u8]) -> Got {
    match mailrs_parse_worker::office_text(kind.into(), body) {
        mailrs_parse_worker::Outcome::Read(Ok(lines)) => match lines.is_empty() {
            true => Got::Nothing,
            false => Got::Text {
                text: lines.join("\n"),
                by_ocr: false,
            },
        },
        mailrs_parse_worker::Outcome::Read(Err(_)) => Got::Failed,
        other => {
            tracing::warn!(outcome = ?other, "attachment text: the office reader did not answer");
            Got::Failed
        }
    }
}

fn read_pdf(body: &[u8], ocr: Option<&OcrSetup>, n: usize, qr: &mut Vec<String>) -> Got {
    if body.len() > MAX_OCR_PDF_BYTES {
        return Got::TooLarge;
    }
    let Some(pages) = embedded_pages(body, n) else {
        return unreadable_pdf(body, ocr, n, qr);
    };
    let mut parts: Vec<String> = pages
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let thin = thin_pages(&pages);
    let mut by_ocr = false;
    if let Some(setup) = ocr
        && !thin.is_empty()
        && let Some(read) = ocr_text(setup, Job::Pdf(&thin), body, n, qr)
        && !read.trim().is_empty()
    {
        parts.push(read.trim().to_string());
        by_ocr = true;
    }
    if parts.is_empty() {
        return Got::Nothing;
    }
    Got::Text {
        text: parts.join("\n"),
        by_ocr,
    }
}

/// The pages too thin on embedded text to be anything but scans, which OCR
/// reads; at most [`MAX_OCR_PAGES`].
fn thin_pages(pages: &[String]) -> Vec<usize> {
    (0..pages.len())
        .filter(|&i| pages[i].chars().filter(|c| !c.is_whitespace()).count() < THIN_PAGE)
        .take(MAX_OCR_PAGES)
        .collect()
}

/// Embedded text of each page, or `None` when the file is too large to
/// read this way or the extractor cannot read it. Read in a parse worker
/// of its own.
fn embedded_pages(body: &[u8], n: usize) -> Option<Vec<String>> {
    if body.len() > MAX_PDF_BYTES {
        return None;
    }
    match mailrs_parse_worker::pdf_pages(body) {
        mailrs_parse_worker::Outcome::Read(Ok(pages)) => Some(pages),
        mailrs_parse_worker::Outcome::Read(Err(error)) => {
            tracing::info!(%error, attachment = n, "attachment text: unreadable pdf");
            None
        }
        other => {
            tracing::warn!(outcome = ?other, attachment = n, "attachment text: the pdf reader did not answer");
            None
        }
    }
}

/// A PDF the embedded-text extractor could not open: rendering it may still
/// get at the pages.
fn unreadable_pdf(body: &[u8], ocr: Option<&OcrSetup>, n: usize, qr: &mut Vec<String>) -> Got {
    let Some(setup) = ocr else {
        if body.len() > MAX_PDF_BYTES {
            return Got::TooLarge;
        }
        return Got::Failed;
    };
    let first: Vec<usize> = (0..MAX_OCR_PAGES).collect();
    let Some(read) = ocr_text(setup, Job::Pdf(&first), body, n, qr) else {
        return Got::Failed;
    };
    if read.trim().is_empty() {
        return Got::Nothing;
    }
    Got::Text {
        text: read,
        by_ocr: true,
    }
}

fn read_image(body: &[u8], ocr: Option<&OcrSetup>, n: usize, qr: &mut Vec<String>) -> Got {
    if body.len() < MIN_IMAGE_BYTES {
        return Got::TooSmall;
    }
    if body.len() > MAX_IMAGE_BYTES {
        return Got::TooLarge;
    }
    let Some(setup) = ocr else {
        return Got::Nothing;
    };
    let answer = super::ocr::run(setup, Job::Image, body);
    if let Ok(a) = &answer {
        qr.extend(a.qr.iter().cloned());
    }
    match answer {
        Ok(a) if a.too_small => Got::TooSmall,
        Ok(a) if a.text.trim().is_empty() => Got::Nothing,
        Ok(a) => Got::Text {
            text: a.text,
            by_ocr: true,
        },
        Err(error) => {
            tracing::info!(%error, attachment = n, "attachment text: ocr failed on an image");
            Got::Failed
        }
    }
}

/// The web addresses of the QR codes in `raw`'s images and in the pages of
/// its PDFs that a full read sends to OCR, without reading any text: the
/// same parts and pages as [`extract_all`], the codes only.
pub(crate) fn qr_only(raw: &[u8], setup: &OcrSetup) -> Vec<String> {
    let parsed = mailrs_mime::parse(raw);
    let mut qr = Vec::new();
    for (n, (kind, part)) in readable_parts(&parsed).enumerate() {
        let body = &part.body[..];
        let pages: Vec<usize>;
        let job = match kind {
            Kind::Image if (MIN_IMAGE_BYTES..=MAX_IMAGE_BYTES).contains(&body.len()) => {
                Job::QrImage
            }
            Kind::Pdf if body.len() <= MAX_OCR_PDF_BYTES => {
                pages = match embedded_pages(body, n) {
                    Some(text) => thin_pages(&text),
                    None => (0..MAX_OCR_PAGES).collect(),
                };
                if pages.is_empty() {
                    continue;
                }
                Job::QrPdf(&pages)
            }
            _ => continue,
        };
        match super::ocr::run(setup, job, body) {
            Ok(a) => qr.extend(a.qr),
            Err(error) => {
                tracing::info!(%error, attachment = n, "attachment text: reading qr codes failed")
            }
        }
    }
    qr
}

fn ocr_text(
    setup: &OcrSetup,
    job: Job<'_>,
    body: &[u8],
    n: usize,
    qr: &mut Vec<String>,
) -> Option<String> {
    match super::ocr::run(setup, job, body) {
        Ok(a) => {
            qr.extend(a.qr);
            Some(a.text)
        }
        Err(error) => {
            tracing::info!(%error, attachment = n, "attachment text: ocr failed on a pdf");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_part(ctype: &str, name: &str, body: &str) -> String {
        format!(
            "Subject: x\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n\
             --B\r\nContent-Type: text/plain\r\n\r\nletter\r\n\
             --B\r\nContent-Type: {ctype}; name={name}\r\n\
             Content-Disposition: attachment; filename={name}\r\n\r\n{body}\r\n--B--\r\n"
        )
    }

    fn kinds(raw: &str) -> Vec<Kind> {
        readable_parts(&mailrs_mime::parse(raw.as_bytes()))
            .map(|(k, _)| k)
            .collect()
    }

    #[test]
    fn a_read_without_its_qr_codes_lacks_only_those() {
        let before_qr = VERSION_WITH_OCR | WITH_OFFICE;
        let now = VERSION_WITH_OCR | WITH_OFFICE | WITH_QR;
        assert!(!covers(before_qr, now));
        assert!(lacks_only_qr(before_qr, now));
        assert!(covers(now, now) && !lacks_only_qr(now, now));
        // read without OCR: the text is behind too, so it is read in full
        assert!(!lacks_only_qr(VERSION_PDF_ONLY | WITH_OFFICE, now));
        // a process without OCR reads no codes and asks for none
        assert!(covers(before_qr, VERSION_PDF_ONLY | WITH_OFFICE));
    }

    #[test]
    fn a_read_covers_another_only_when_it_did_everything_that_one_would() {
        let ocr_office = VERSION_WITH_OCR | WITH_OFFICE;
        let pdf_office = VERSION_PDF_ONLY | WITH_OFFICE;
        // read before Office files were: read again
        assert!(!covers(VERSION_WITH_OCR, ocr_office));
        assert!(!covers(VERSION_PDF_ONLY, pdf_office));
        // read with OCR: a process without OCR leaves it alone
        assert!(covers(ocr_office, pdf_office));
        // read without OCR: a process with OCR reads it again
        assert!(!covers(pdf_office, ocr_office));
        assert!(covers(ocr_office, ocr_office));
    }

    #[test]
    fn office_files_are_known_by_their_type_or_their_name() {
        use mailrs_attachment_extract::office::OfficeKind;
        let docx = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        assert_eq!(
            kinds(&with_part(docx, "a.bin", "x")),
            vec![Kind::Office(OfficeKind::Word)]
        );
        assert_eq!(
            kinds(&with_part("application/octet-stream", "Q3.XLSX", "x")),
            vec![Kind::Office(OfficeKind::Excel)]
        );
        assert!(may_carry_readable(with_part(docx, "a.bin", "x").as_bytes()));
    }

    #[test]
    fn an_office_file_reads_as_its_text() {
        use std::io::Write as _;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"<w:document><w:body><w:p><w:r><w:t>Wire the deposit</w:t></w:r></w:p></w:body></w:document>")
            .unwrap();
        zip.finish().unwrap();
        let got = read_office(
            mailrs_attachment_extract::office::OfficeKind::Word,
            &out.into_inner(),
        );
        assert!(matches!(got, Got::Text { ref text, by_ocr: false } if text == "Wire the deposit"));
        assert!(matches!(
            read_office(
                mailrs_attachment_extract::office::OfficeKind::Word,
                b"not a zip"
            ),
            Got::Failed
        ));
    }

    #[test]
    fn parts_are_known_by_their_type_or_their_name() {
        assert_eq!(
            kinds(&with_part("application/pdf", "a.bin", "x")),
            vec![Kind::Pdf]
        );
        assert_eq!(
            kinds(&with_part("application/octet-stream", "A.PDF", "x")),
            vec![Kind::Pdf]
        );
        assert_eq!(
            kinds(&with_part("image/jpeg", "a.bin", "x")),
            vec![Kind::Image]
        );
        assert_eq!(
            kinds(&with_part("application/octet-stream", "scan.JPG", "x")),
            vec![Kind::Image]
        );
        assert!(kinds(&with_part("application/octet-stream", "a.zip", "x")).is_empty());
        assert!(kinds(&with_part("image/svg+xml", "a.svg", "x")).is_empty());
        assert!(kinds(&with_part("text/plain", "notes.pdf", "x")).is_empty());
        assert!(kinds("Subject: x\r\n\r\njust a letter\r\n").is_empty());
    }

    /// An image shown inline through `cid:` has no file name and no
    /// attachment disposition, and is read all the same.
    #[test]
    fn an_inline_image_counts() {
        let raw = "Subject: x\r\nContent-Type: multipart/related; boundary=B\r\n\r\n\
                   --B\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:p1\">\r\n\
                   --B\r\nContent-Type: image/png\r\nContent-ID: <p1>\r\n\r\nx\r\n--B--\r\n";
        assert_eq!(kinds(raw), vec![Kind::Image]);
    }

    /// The byte check lets through every message `readable_parts` would
    /// find something in, including a name hidden in an encoded word.
    #[test]
    fn the_byte_check_never_drops_a_readable_part() {
        let encoded = "\"=?UTF-8?B?6KuL5rGC5pu4LnBkZg==?=\"";
        for raw in [
            with_part("application/pdf", "a.bin", "x"),
            with_part("Application/PDF", "a.bin", "x"),
            with_part("application/octet-stream", "A.PDF", "x"),
            with_part("application/octet-stream", encoded, "x"),
            with_part("image/png", "a.png", "x"),
            with_part("IMAGE/JPEG", "a.jpg", "x"),
        ] {
            assert_eq!(kinds(&raw).len(), 1, "{raw}");
            assert!(may_carry_readable(raw.as_bytes()), "{raw}");
        }
        assert!(!may_carry_readable(b"Subject: x\r\n\r\njust a letter\r\n"));
    }

    #[test]
    fn an_oversized_pdf_is_counted_and_not_read() {
        let big = "x".repeat(MAX_OCR_PDF_BYTES + 1);
        let (texts, state, _) = extract_all(
            with_part("application/pdf", "big.pdf", &big).as_bytes(),
            None,
        );
        assert!(texts.is_empty());
        assert_eq!((state.pdfs, state.too_large, state.failed), (1, 1, 0));
    }

    #[test]
    fn without_ocr_an_unreadable_pdf_fails_and_images_are_passed_over() {
        let pdf = with_part("application/pdf", "bad.pdf", "%PDF-1.4 junk");
        let image = "x".repeat(4096);
        let raw = format!(
            "{}--B\r\nContent-Type: image/png; name=a.png\r\n\r\n{image}\r\n--B--\r\n",
            pdf.trim_end_matches("--B--\r\n"),
        );
        let (texts, state, _) = extract_all(raw.as_bytes(), None);
        assert!(texts.is_empty());
        assert_eq!((state.parts, state.pdfs, state.images), (2, 1, 1));
        assert_eq!((state.failed, state.stored, state.too_small), (1, 0, 0));
        assert_eq!(state.version, VERSION_PDF_ONLY | WITH_OFFICE);
    }

    #[test]
    fn a_tiny_image_is_too_small_to_read() {
        let (_, state, _) = extract_all(with_part("image/png", "dot.png", "x").as_bytes(), None);
        assert_eq!((state.images, state.too_small), (1, 1));
    }
}
