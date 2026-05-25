#![no_main]
//! Fuzz mailrs-attachment-extract on arbitrary PDF-shaped bytes. The
//! whole point is "extract text from any incoming attachment without
//! panicking" — must hold on garbage input that masquerades as PDF.

use libfuzzer_sys::fuzz_target;
use mailrs_attachment_extract::{extraction_method, extract_pdf_text};

fuzz_target!(|data: &[u8]| {
    if let Ok(ct) = std::str::from_utf8(data) {
        let _ = extraction_method(ct);
    }
    let _ = extract_pdf_text(data);
});
