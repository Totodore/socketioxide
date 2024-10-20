use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use serde::Serialize;
use socketioxide_core::{parser::Parse, Value};
use socketioxide_parser_common::CommonParser;
#[derive(Serialize)]
struct Data<T> {
    str: String,
    data: usize,
    arr: Vec<u8>,
    inner: T,
}
impl<T> Data<T> {
    pub fn new(inner: T) -> Self {
        Self {
            arr: vec![10; 50],
            data: 100,
            str: "test".to_string(),
            inner,
        }
    }
}

#[derive(Serialize)]
struct NestedDataWithBinaries {
    foo: &'static str,
    inner: Data<Vec<Bytes>>,
}
impl NestedDataWithBinaries {
    fn new() -> Self {
        NestedDataWithBinaries {
            foo: "bar",
            inner: Data::new(vec![
                Bytes::from_static(&[
                    20, 120, 24, 2, 14, 3, 13, 31, 13, 45, 67
                ]);
                10
            ]),
        }
    }
}

fn serde_encode<T: Serialize + ?Sized>(data: &mut T) -> String {
    serde_json::to_string(black_box(data)).unwrap()
}
fn socketio_encode<T: Serialize + ?Sized>(data: &T, event: Option<&str>) -> Value {
    CommonParser
        .encode_value(black_box(data), black_box(event))
        .unwrap()
}

fn benchmark_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_common/encode_value_default");

    group.bench_function("Encode classic input data with serde_json", |b| {
        b.iter_batched_ref(|| Data::new("test"), serde_encode, BatchSize::SmallInput);
    });
    group.bench_function("Encode classic input data with common_parser", |b| {
        b.iter_batched_ref(
            || Data::new("test"),
            |data| socketio_encode(data, None),
            BatchSize::SmallInput,
        );
    });
    group.bench_function(
        "Encode classic input data with common_parser and event",
        |b| {
            b.iter_batched_ref(
                || Data::new("test"),
                |data| socketio_encode(data, Some("event")),
                BatchSize::SmallInput,
            );
        },
    );
}

fn benchmark_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_common/encode_value_binary");

    group.bench_function("Encode binary input data with serde_json", |b| {
        b.iter_batched_ref(
            || NestedDataWithBinaries::new(),
            serde_encode,
            BatchSize::SmallInput,
        );
    });
    group.bench_function("Encode binary input data with common_parser", |b| {
        b.iter_batched_ref(
            || NestedDataWithBinaries::new(),
            |data| socketio_encode(data, None),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("Encode binary data with common_parser and event", |b| {
        b.iter_batched_ref(
            || NestedDataWithBinaries::new(),
            |data| socketio_encode(data, Some("event")),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, benchmark_default, benchmark_binary);
criterion_main!(benches);
