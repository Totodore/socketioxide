use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use socketioxide::extensions::Extensions;

fn bench_extensions(c: &mut Criterion) {
    let mut group = c.benchmark_group("extensions");
    group.bench_function("concurrent_inserts", |b| {
        let ext = Extensions::new();
        b.iter(|| {
            ext.insert(5i32);
        });
    });
    group.bench_function("concurrent_get", |b| {
        let ext = Extensions::new();
        b.iter(|| {
            ext.insert(5i32);
        })
    });
    group.bench_function("concurrent_get_inserts", |b| {
        b.iter(|| {
            let mut ext = Extensions::new();
            ext.insert(5i32);
        })
    });
    group.finish();
}

criterion_group!(benches, bench_extensions);
criterion_main!(benches);
