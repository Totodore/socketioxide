//! # itoa_bench, used to compare best solutions related to this issue: https://github.com/Totodore/socketioxide/issues/143

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// This solution doesn't imply additional buffer or dependency call
fn insert_number_reverse(mut v: u32, res: &mut String) {
    let mut buf = [0u8; 10];
    let mut i = 1;
    while v > 0 {
        let n = (v % 10) as u8;
        buf[10 - i] = n + 0x30;
        v /= 10;
        i += 1;
    }
    res.push_str(unsafe { std::str::from_utf8_unchecked(&buf[10 - (i - 1)..]) });
}

/// This solution uses the itoa crate
fn insert_number_itoa(v: u32, res: &mut String) {
    let mut buffer = itoa::Buffer::new();
    res.push_str(buffer.format(v));
}

fn bench_itoa(c: &mut Criterion) {
    let mut group = c.benchmark_group("itoa");
    for i in [u32::MAX / 200000, u32::MAX / 2, u32::MAX].iter() {
        group.bench_with_input(BenchmarkId::new("number_reverse", i), i, |b, i| {
            b.iter(|| insert_number_reverse(*i, &mut String::new()))
        });
        group.bench_with_input(BenchmarkId::new("number_itoa", i), i, |b, i| {
            b.iter(|| insert_number_itoa(*i, &mut String::new()))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_itoa);
criterion_main!(benches);
