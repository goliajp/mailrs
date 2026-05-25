#![no_main]
//! Fuzz mailrs-imap-codec on arbitrary bytes. The codec is on every
//! IMAP connection's hot path — must never panic on adversarial input.

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use mailrs_imap_codec::ImapCodec;
use tokio_util::codec::Decoder;

fuzz_target!(|data: &[u8]| {
    let mut codec = ImapCodec::new();
    let mut buf = BytesMut::from(data);
    // Repeatedly decode until the buffer settles. Mimics how the
    // session loop drains a TCP read.
    while let Ok(Some(_)) = codec.decode(&mut buf) {
        if buf.is_empty() {
            break;
        }
    }
});
