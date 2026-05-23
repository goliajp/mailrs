#![no_main]
//! Fuzz RFC 5322 lazy message parser. The whole point of this crate is
//! "scan untrusted bytes without panicking" — must hold on any input.

use libfuzzer_sys::fuzz_target;
use mailrs_rfc5322::Message;

fuzz_target!(|data: &[u8]| {
    let msg = Message::new(data);
    let _ = msg.body_offset();
    let _ = msg.body();
    let _ = msg.header("Subject");
    let _ = msg.header("From");
    let _ = msg.header("To");
    let _ = msg.header("Received");
    // Walk all headers — exercises the iterator over potentially-malformed input.
    for h in msg.headers() {
        let _ = h.value_str();
    }
});
