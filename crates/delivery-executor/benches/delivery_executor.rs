use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_delivery_executor::DeliveryExecutor;
use std::hint::black_box;

fn bench_spawn(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("DeliveryExecutor::spawn", |b| {
        b.iter(|| {
            let _ex = rt.block_on(async { DeliveryExecutor::spawn() });
            black_box(_ex);
        });
    });
}

criterion_group!(benches, bench_spawn);
criterion_main!(benches);
