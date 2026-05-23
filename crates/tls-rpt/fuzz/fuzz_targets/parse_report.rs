#![no_main]
//! Fuzz the JSON report deserializer. Report bodies arrive over
//! the network (HTTPS POST or signed email attachment) and may be
//! produced by any sender — the deserializer must tolerate any byte
//! sequence (serde_json already does this; this target exists to
//! confirm the bound holds for our specific struct shape).

use libfuzzer_sys::fuzz_target;
use mailrs_tls_rpt::Report;

fuzz_target!(|data: &[u8]| {
    let _: Result<Report, _> = serde_json::from_slice(data);
});
