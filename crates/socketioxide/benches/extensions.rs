use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use rand::Rng;
use socketioxide::extensions::Extensions;

fn bench_extensions(c: &mut Criterion) {
    let i = black_box(5i32);
    let mut group = c.benchmark_group("extensions");
    group.bench_function("concurrent_inserts", |b| {
        let ext = Extensions::new();
        b.iter(|| {
            ext.insert(i);
        });
    });
    group.bench_function("concurrent_get", |b| {
        let ext = Extensions::new();
        ext.insert(i);
        b.iter(|| {
            ext.get::<i32>();
        })
    });
    group.bench_function("concurrent_get_inserts", |b| {
        let ext = Extensions::new();
        b.iter_batched(
            || rand::rng().random_range(0..3),
            |i| {
                if i == 0 {
                    ext.insert(i);
                } else {
                    ext.get::<i32>();
                }
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, bench_extensions);
criterion_main!(benches);
