#![no_main]
//! Fuzz the MTA-STS policy-file parser. Untrusted input — the policy
//! file is fetched over HTTPS from `mta-sts.<domain>`, which the sender
//! does not control, so the parser MUST tolerate any byte sequence.

use libfuzzer_sys::fuzz_target;
use mailrs_mta_sts::Policy;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = Policy::parse(s);
    }
});
