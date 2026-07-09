use bytes::Bytes;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use rmp_serde::{decode::from_slice, encode::to_vec_named};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use socketioxide_core::{Value, parser::Parse};
use socketioxide_parser_msgpack::MsgPackParser;

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
struct NestedDataWithBinaries {
    foo: String,
    inner: Data<Vec<Bytes>>,
}
impl NestedDataWithBinaries {
    fn new() -> Self {
        NestedDataWithBinaries {
            foo: "bar".to_string(),
            inner: Data::new(vec![
                Bytes::from_static(&[
                    20, 120, 24, 2, 14, 3, 13, 31, 13, 45, 67
                ]);
                10
            ]),
        }
    }
}

fn serde_decode<T: DeserializeOwned>(data: &[u8]) -> T {
    from_slice(black_box(data)).unwrap()
}
fn socketio_decode<T: DeserializeOwned>(data: &mut Value, with_event: bool) -> T {
    MsgPackParser
        .decode_value(black_box(data), with_event)
        .unwrap()
}
fn benchmark_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_msgpack/decode_value_default");
    group.bench_function("Decode classic input data with rmp_serde", |b| {
        b.iter_batched_ref(
            || {
                MsgPackParser
                    .encode_value(&Data::new("test".to_string()), None)
                    .unwrap()
            },
            |d| serde_decode::<(Data<String>,)>(d.as_bytes().unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode classic input data with msgpack parser", |b| {
        b.iter_batched_ref(
            || {
                MsgPackParser
                    .encode_value(&Data::new("test".to_string()), None)
                    .unwrap()
            },
            |d| socketio_decode::<Data<String>>(d, false),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode classic input data with rmp_serde and event", |b| {
        b.iter_batched_ref(
            || {
                MsgPackParser
                    .encode_value(&Data::new("test".to_string()), Some("event"))
                    .unwrap()
            },
            |d| serde_decode::<(String, Data<String>)>(d.as_bytes().unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.bench_function(
        "Decode classic input data with msgpack parser and event",
        |b| {
            b.iter_batched_ref(
                || {
                    MsgPackParser
                        .encode_value(&Data::new("test".to_string()), Some("event"))
                        .unwrap()
                },
                |d| socketio_decode::<Data<String>>(d, true),
                BatchSize::SmallInput,
            )
        },
    );
}
fn benchmark_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_msgpack/decode_value_binary");
    group.bench_function("Decode binary input data with rmp_serde", |b| {
        b.iter_batched_ref(
            || to_vec_named(&(NestedDataWithBinaries::new(),)).unwrap(),
            |d| serde_decode::<(NestedDataWithBinaries,)>(d),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode binary input data with msgpack parser", |b| {
        b.iter_batched_ref(
            || {
                MsgPackParser
                    .encode_value(&NestedDataWithBinaries::new(), None)
                    .unwrap()
            },
            |d| socketio_decode::<NestedDataWithBinaries>(d, false),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode binary input data with rmp_serde and event", |b| {
        b.iter_batched_ref(
            || to_vec_named(&("event", NestedDataWithBinaries::new())).unwrap(),
            |d| serde_decode::<(String, NestedDataWithBinaries)>(d),
            BatchSize::SmallInput,
        )
    });
    group.bench_function(
        "Decode binary input data with msgpack parser and event",
        |b| {
            b.iter_batched_ref(
                || {
                    MsgPackParser
                        .encode_value(&NestedDataWithBinaries::new(), Some("event"))
                        .unwrap()
                },
                |d| socketio_decode::<NestedDataWithBinaries>(d, true),
                BatchSize::SmallInput,
            )
        },
    );
}
criterion_group!(benches, benchmark_default, benchmark_binary);
criterion_main!(benches);
