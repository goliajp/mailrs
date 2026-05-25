#![no_main]
//! Fuzz mailrs-imap-format helpers on arbitrary input. Every fn here
//! runs on per-FETCH-response data — must never panic on malformed
//! MIME / attribute strings.

use libfuzzer_sys::fuzz_target;
use mailrs_imap_format::{
    extract_body_section, extract_header_fields, extract_header_section, parse_imap_flags,
    parse_header_fields_request, parse_generic_body_sections, parse_mime_headers,
    split_mime_parts,
};

fuzz_target!(|data: &[u8]| {
    // String-shaped helpers
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_imap_flags(s);
        let _ = parse_header_fields_request(s);
        let _ = parse_generic_body_sections(s);
        let _ = parse_mime_headers(s);
    }
    // Byte-shaped helpers
    let _ = extract_header_section(data);
    let _ = extract_body_section(data);
    let _ = extract_header_fields(data, &["Subject".into(), "From".into()]);
    let _ = split_mime_parts(data, "boundary");
});
