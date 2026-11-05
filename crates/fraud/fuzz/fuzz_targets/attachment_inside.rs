#![no_main]
//! Fuzz what an attachment holds — the zip directory, the RAR 4 and RAR 5
//! header walks, the 7z and OLE signatures — on arbitrary bytes: must not
//! panic, loop, or allocate from a declared length.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = mailrs_fraud::attachment_inside::read([("attachment", data)]);
    let _ = mailrs_fraud::attachment_inside::carries_code("attachment", data);
});
