#![no_main]
//! Fuzz mailrs-smtp-codec on arbitrary bytes. The smuggling-protection
//! check runs on every DATA payload — must hold on adversarial input.

use libfuzzer_sys::fuzz_target;
use mailrs_smtp_codec::{has_smuggle_sequence, normalize_line_endings};

fuzz_target!(|data: &[u8]| {
    let _ = has_smuggle_sequence(data);
    let _ = normalize_line_endings(data);
});
