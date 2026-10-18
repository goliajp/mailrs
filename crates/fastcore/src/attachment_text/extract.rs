//! Which parts are PDFs, and reading their text.

use mailrs_mailbox_kevy::AttachmentTextState;
use mailrs_mailbox_kevy::keys::{ATTACHMENT_TEXT_CAP, cap_text};

/// A PDF over this many bytes is not read. The extractor keeps the whole
/// document in memory, more than once over.
pub const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;

/// The message's PDF attachments, in MIME order.
///
/// `application/pdf`, or `application/octet-stream` named `*.pdf` — the
/// type some mailers give every attachment.
pub(crate) fn pdf_parts<'p, 'a>(
    parsed: &'p mailrs_mime::Part<'a>,
) -> impl Iterator<Item = &'p mailrs_mime::Part<'a>> {
    parsed.attachments().filter(|p| {
        let ct = &p.content_type;
        if ct.type_ != "application" {
            return false;
        }
        match ct.subtype.as_str() {
            "pdf" => true,
            "octet-stream" => p
                .attachment_filename()
                .is_some_and(|n| n.to_ascii_lowercase().ends_with(".pdf")),
            _ => false,
        }
    })
}

/// Whether `raw` can hold a PDF part at all, checked on the bytes before
/// a full MIME parse. An octet-stream part counts: its file name may be
/// an encoded word that hides the `.pdf`.
pub(crate) fn may_carry_pdf(raw: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 3] = [b"application/pdf", b"application/octet-stream", b".pdf"];
    NEEDLES.iter().any(|n| contains_ignore_case(raw, n))
}

fn contains_ignore_case(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// Read every PDF in `raw`. Blocking: CPU-bound and unbounded in time on
/// a hostile document, so callers run it off the async runtime.
///
/// Returns each stored text with its position among the PDFs, and the
/// counts that say what happened to the rest.
pub(crate) fn extract_all(raw: &[u8]) -> (Vec<(usize, String)>, AttachmentTextState) {
    let parsed = mailrs_mime::parse(raw);
    let mut texts = Vec::new();
    let mut state = AttachmentTextState::default();
    for (n, part) in pdf_parts(&parsed).enumerate() {
        state.pdfs += 1;
        if part.body.len() > MAX_PDF_BYTES {
            state.too_large += 1;
            continue;
        }
        // The PDF came from outside, and a parser panic on it must cost
        // this attachment only — not the others, and not the worker.
        let body: &[u8] = &part.body;
        let result = std::panic::catch_unwind(|| mailrs_attachment_extract::extract_pdf_text(body));
        match result {
            Ok(Ok(r)) => {
                let text = r.text.trim();
                if text.is_empty() {
                    continue;
                }
                let capped = cap_text(text, ATTACHMENT_TEXT_CAP);
                if capped.len() < text.len() {
                    state.truncated += 1;
                }
                state.stored += 1;
                texts.push((n, capped.to_string()));
            }
            Ok(Err(error)) => {
                tracing::info!(%error, attachment = n, "pdf text: unreadable attachment");
                state.failed += 1;
            }
            Err(_) => {
                tracing::warn!(attachment = n, "pdf text: the extractor panicked");
                state.failed += 1;
            }
        }
    }
    (texts, state)
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

    fn pdfs(raw: &str) -> usize {
        pdf_parts(&mailrs_mime::parse(raw.as_bytes())).count()
    }

    #[test]
    fn a_pdf_is_known_by_its_type_or_by_its_name() {
        assert_eq!(pdfs(&with_part("application/pdf", "a.bin", "x")), 1);
        assert_eq!(
            pdfs(&with_part("application/octet-stream", "A.PDF", "x")),
            1
        );
        assert_eq!(
            pdfs(&with_part("application/octet-stream", "a.zip", "x")),
            0
        );
        assert_eq!(pdfs(&with_part("text/plain", "notes.pdf", "x")), 0);
        assert_eq!(pdfs("Subject: x\r\n\r\njust a letter\r\n"), 0);
    }

    /// The byte check lets through every message `pdf_parts` would find a
    /// PDF in, including a name hidden in an encoded word.
    #[test]
    fn the_byte_check_never_drops_a_pdf() {
        let encoded = "\"=?UTF-8?B?6KuL5rGC5pu4LnBkZg==?=\"";
        for raw in [
            with_part("application/pdf", "a.bin", "x"),
            with_part("Application/PDF", "a.bin", "x"),
            with_part("application/octet-stream", "A.PDF", "x"),
            with_part("application/octet-stream", encoded, "x"),
        ] {
            assert_eq!(pdfs(&raw), 1, "{raw}");
            assert!(may_carry_pdf(raw.as_bytes()), "{raw}");
        }
        assert!(!may_carry_pdf(b"Subject: x\r\n\r\njust a letter\r\n"));
        assert!(!may_carry_pdf(
            with_part("image/png", "a.png", "x").as_bytes()
        ));
    }

    #[test]
    fn an_oversized_pdf_is_counted_and_not_read() {
        let big = "x".repeat(MAX_PDF_BYTES + 1);
        let (texts, state) = extract_all(with_part("application/pdf", "big.pdf", &big).as_bytes());
        assert!(texts.is_empty());
        assert_eq!((state.pdfs, state.too_large, state.failed), (1, 1, 0));
    }

    #[test]
    fn an_unreadable_pdf_is_counted_as_failed() {
        let (texts, state) =
            extract_all(with_part("application/pdf", "bad.pdf", "%PDF-1.4 junk").as_bytes());
        assert!(texts.is_empty());
        assert_eq!((state.pdfs, state.failed, state.stored), (1, 1, 0));
    }
}
