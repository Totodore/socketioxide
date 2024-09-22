use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use socketioxide_core::{
    packet::{ConnectPacket, Packet},
    parser::Parse,
    Sid, Value,
};
use socketioxide_parser_common::CommonParser;

fn encode(packet: Packet) -> String {
    match CommonParser::default().encode(black_box(packet)) {
        Value::Str(d, _) => d.into(),
        Value::Bytes(_) => panic!("testing only returns str"),
    }
}
fn to_value<T: serde::Serialize>(data: T) -> Value {
    Value::Str(serde_json::to_string(&data).unwrap().into(), None)
}
fn to_value_event<T: serde::Serialize>(data: T, event: &str) -> Value {
    Value::Str(serde_json::to_string(&(event, data)).unwrap().into(), None)
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("socketio_packet/encode");

    let connect = CommonParser::default()
        .encode_value(&ConnectPacket { sid: Sid::ZERO }, None)
        .unwrap();

    group.bench_function("Encode packet connect on /", |b| {
        b.iter_batched(
            || Packet::connect("/", Some(connect.clone())),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet connect on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::connect("/custom_nsp", Some(connect.clone())),
            encode,
            BatchSize::SmallInput,
        )
    });

    const DATA: &str = r#"{"_placeholder":true,"num":0}"#;
    const BINARY: Bytes = Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    group.bench_function("Encode packet event on /", |b| {
        b.iter_batched(
            || Packet::event("/", to_value_event(DATA, "event")),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::event("custom_nsp", to_value_event(DATA, "event")),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event with ack on /", |b| {
        b.iter_batched(
            || {
                let mut packet = Packet::event("/", to_value_event(DATA, "event"));
                packet.inner.set_ack_id(0);
                packet
            },
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event with ack on /custom_nsp", |b| {
        b.iter_batched(
            || {
                let mut packet = Packet::event("/custom_nsp", to_value_event(DATA, "event"));
                packet.inner.set_ack_id(0);
                packet
            },
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet ack on /", |b| {
        b.iter_batched(
            || Packet::ack("/", to_value(DATA), 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet ack on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::ack("/custom_nsp", to_value(DATA), 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary event (b64) on /", |b| {
        b.iter_batched(
            || Packet::event("/", to_value_event((DATA, BINARY), "event")),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary event (b64) on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::event("/custom_nsp", to_value_event((DATA, BINARY), "event")),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary ack (b64) on /", |b| {
        b.iter_batched(
            || Packet::ack("/", to_value((DATA, BINARY)), 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary ack (b64) on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::ack("/custom_nsp", to_value((DATA, BINARY)), 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
