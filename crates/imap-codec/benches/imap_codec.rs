use bytes::BytesMut;
use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_imap_codec::ImapCodec;
use std::hint::black_box;
use tokio_util::codec::Decoder;

fn bench_decode(c: &mut Criterion) {
    c.bench_function("ImapCodec::decode/LOGIN", |b| {
        b.iter(|| {
            let mut codec = ImapCodec::new();
            let mut buf = BytesMut::from(&b"a001 LOGIN user pass\r\n"[..]);
            let _ = codec.decode(black_box(&mut buf));
        });
    });
}

criterion_group!(benches, bench_decode);
criterion_main!(benches);
