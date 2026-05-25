#![no_main]
//! Fuzz mailrs-sieve compile + evaluate. Sieve scripts ship from user
//! config — must never panic on a malformed script or message.

use libfuzzer_sys::fuzz_target;
use mailrs_sieve::{compile_sieve, evaluate_sieve, evaluate_sieve_with_envelope};

fuzz_target!(|data: &[u8]| {
    if let Ok(script) = std::str::from_utf8(data) {
        if let Ok(compiled) = compile_sieve(script) {
            let _ = evaluate_sieve(&compiled, b"From: a@b.c\r\n\r\nbody");
            let _ = evaluate_sieve_with_envelope(
                &compiled,
                b"From: a@b.c\r\n\r\nbody",
                Some("a@b.c"),
                Some("me@d.e"),
            );
        }
    }
});
