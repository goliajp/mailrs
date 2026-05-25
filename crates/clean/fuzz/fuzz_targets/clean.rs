#![no_main]
//! Fuzz mailrs-clean on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = mailrs_clean::clean_email_html(s);
        let _ = mailrs_clean::split_quoted_content(s);
        let _ = mailrs_clean::detect_bulk_sender(s);
    }
});
