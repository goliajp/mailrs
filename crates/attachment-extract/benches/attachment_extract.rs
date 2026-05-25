use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_attachment_extract::extraction_method;
use std::hint::black_box;

fn bench_extraction_method(c: &mut Criterion) {
    c.bench_function("extraction_method/text_plain", |b| {
        b.iter(|| {
            let _ = extraction_method(black_box("text/plain"));
        });
    });
    c.bench_function("extraction_method/application_pdf", |b| {
        b.iter(|| {
            let _ = extraction_method(black_box("application/pdf"));
        });
    });
}

criterion_group!(benches, bench_extraction_method);
criterion_main!(benches);
