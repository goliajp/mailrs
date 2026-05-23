#![no_main]
//! Fuzz MIME tree parser. Multipart boundaries are notorious for parser
//! bugs (off-by-one on terminators, infinite recursion on nested parts).

use libfuzzer_sys::fuzz_target;
use mailrs_mime::parse;

fuzz_target!(|data: &[u8]| {
    let part = parse(data);
    // Exercise the walk + lookup methods too — they may trip cases the parser
    // missed (e.g. empty leaves, malformed children).
    for p in part.walk() {
        let _ = p.body_text();
        let _ = p.attachment_filename();
    }
    let _ = part.find_by_content_type("text/plain");
    let _ = part.find_by_content_type("multipart/alternative");
});
