use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use engineioxide::sid::Sid;
use socketioxide::{
    packet::{ConnectPacket, Packet, PacketData},
    parser::{CommonParser, Parse, TransportPayload},
    ProtocolVersion, Value,
};

fn encode(packet: Packet) -> String {
    match CommonParser::default().encode(black_box(packet)).0 {
        TransportPayload::Str(d) => d.into(),
        TransportPayload::Bytes(_) => panic!("testing only returns str"),
    }
}
fn to_value<T: serde::Serialize>(data: T) -> Value {
    Value::Json(serde_json::to_value(data).unwrap())
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("socketio_packet/encode");

    let connect = CommonParser::default()
        .to_value(ConnectPacket { sid: Sid::ZERO })
        .unwrap();

    group.bench_function("Encode packet connect on /", |b| {
        b.iter_batched(
            || Packet::connect("/", connect.clone(), ProtocolVersion::V5),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet connect on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::connect("/custom_nsp", connect.clone(), ProtocolVersion::V5),
            encode,
            BatchSize::SmallInput,
        )
    });

    const DATA: &str = r#"{"_placeholder":true,"num":0}"#;
    const BINARY: Bytes = Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    group.bench_function("Encode packet event on /", |b| {
        b.iter_batched(
            || Packet::event("/", "event", to_value(DATA)),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::event("custom_nsp", "event", to_value(DATA)),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event with ack on /", |b| {
        b.iter_batched(
            || {
                let packet = Packet::event("/", "event", to_value(DATA));
                if let PacketData::Event(_, _, mut ack) = packet.inner {
                    let _ = ack.insert(black_box(0));
                }
                packet
            },
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet event with ack on /custom_nsp", |b| {
        b.iter_batched(
            || {
                let packet = Packet::event("/custom_nsp", "event", to_value(DATA));
                if let PacketData::Event(_, _, mut ack) = packet.inner {
                    let _ = ack.insert(black_box(0));
                }
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
            || Packet::bin_event("/", "event", to_value(DATA), vec![BINARY.clone()]),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary event (b64) on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::bin_event("/custom_nsp", "event", to_value(DATA), vec![BINARY.clone()]),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary ack (b64) on /", |b| {
        b.iter_batched(
            || Packet::bin_ack("/", to_value(DATA), vec![BINARY.clone()], 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Encode packet binary ack (b64) on /custom_nsp", |b| {
        b.iter_batched(
            || Packet::bin_ack("/custom_nsp", to_value(DATA), vec![BINARY.clone()], 0),
            encode,
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
