use bytes::Bytes;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use serde::Serialize;
use socketioxide_core::{Value, parser::Parse};
use socketioxide_parser_msgpack::MsgPackParser;

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

fn serde_encode<T: Serialize + ?Sized>(data: &mut T) -> Vec<u8> {
    rmp_serde::to_vec_named(black_box(data)).unwrap()
}
fn socketio_encode<T: Serialize + ?Sized>(data: &T, event: Option<&str>) -> Value {
    MsgPackParser
        .encode_value(black_box(data), black_box(event))
        .unwrap()
}

fn benchmark_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_msgpack/encode_value_default");

    group.bench_function("Encode classic input data with rmp_serde", |b| {
        b.iter_batched_ref(|| Data::new("test"), serde_encode, BatchSize::SmallInput);
    });
    group.bench_function("Encode classic input data with msgpack parser", |b| {
        b.iter_batched_ref(
            || Data::new("test"),
            |data| socketio_encode(data, None),
            BatchSize::SmallInput,
        );
    });
    group.bench_function(
        "Encode classic input data with msgpack parser and event",
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
    let mut group = c.benchmark_group("parser_msgpack/encode_value_binary");

    group.bench_function("Encode binary input data with rmp_serde", |b| {
        b.iter_batched_ref(
            NestedDataWithBinaries::new,
            serde_encode,
            BatchSize::SmallInput,
        );
    });
    group.bench_function("Encode binary input data with msgpack parser", |b| {
        b.iter_batched_ref(
            NestedDataWithBinaries::new,
            |data| socketio_encode(data, None),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("Encode binary data with msgpack parser and event", |b| {
        b.iter_batched_ref(
            NestedDataWithBinaries::new,
            |data| socketio_encode(data, Some("event")),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, benchmark_default, benchmark_binary);
criterion_main!(benches);
