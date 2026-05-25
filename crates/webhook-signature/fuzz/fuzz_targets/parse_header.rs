#![no_main]
//! Fuzz mailrs-webhook-signature on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = mailrs_webhook_signature::parse_header(s);
        let _ = mailrs_webhook_signature::verify(b"secret", b"payload", s);
    }
});
