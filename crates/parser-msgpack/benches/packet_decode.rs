use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use socketioxide_core::{
    packet::{ConnectPacket, Packet, PacketData},
    parser::Parse,
    Sid, Value,
};
use socketioxide_parser_msgpack::MsgPackParser;

fn encode(packet: Packet) -> Bytes {
    match MsgPackParser.encode(black_box(packet)) {
        Value::Str(_, _) => panic!("testing only returns bytes"),
        Value::Bytes(d) => d.into(),
    }
}
fn decode(value: Bytes) -> Option<Packet> {
    MsgPackParser
        .decode_bin(&Default::default(), black_box(value.into()))
        .ok()
}

fn to_event_value(data: impl serde::Serialize, event: &str) -> Value {
    MsgPackParser.encode_value(&data, Some(event)).unwrap()
}

fn to_value(data: impl serde::Serialize) -> Value {
    MsgPackParser.encode_value(&data, None).unwrap()
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_msgpack/decode_packet");
    let connect = MsgPackParser
        .encode_default(&ConnectPacket { sid: Sid::ZERO })
        .unwrap();

    group.bench_function("Decode packet connect on /", |b| {
        b.iter_batched(
            || encode(Packet::connect("/", Some(connect.clone()))),
            decode,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode packet connect on /custom_nsp", |b| {
        b.iter_batched(
            || encode(Packet::connect("/custom_nsp", Some(connect.clone()))),
            decode,
            BatchSize::SmallInput,
        )
    });

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    struct Data {
        foo: &'static str,
        arr: [u8; 3],
    }

    const DATA: Data = Data {
        foo: "bar",
        arr: [1, 2, 3],
    };
    const BINARY: Bytes = Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    group.bench_function("Decode packet event on /", |b| {
        b.iter_batched(
            || encode(Packet::event("/", to_event_value(DATA, "event"))),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet event on /custom_nsp", |b| {
        b.iter_batched(
            || encode(Packet::event("custom_nsp", to_event_value(DATA, "event"))),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet event with ack on /", |b| {
        b.iter_batched(
            || {
                let packet = Packet::event("/", to_event_value(DATA, "event"));
                match packet.inner {
                    PacketData::Event(_, mut ack) => ack.insert(black_box(0)),
                    _ => panic!("Wrong packet type"),
                };
                encode(packet)
            },
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet event with ack on /custom_nsp", |b| {
        b.iter_batched(
            || {
                let packet = Packet::event("/custom_nsp", to_event_value(DATA, "event"));
                match packet.inner {
                    PacketData::Event(_, mut ack) => ack.insert(black_box(0)),
                    _ => panic!("Wrong packet type"),
                };
                encode(packet)
            },
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet ack on /", |b| {
        b.iter_batched(
            || {
                encode(Packet::ack(
                    "/",
                    to_event_value(DATA, "event"),
                    black_box(0),
                ))
            },
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet ack on /custom_nsp", |b| {
        b.iter_batched(
            || encode(Packet::ack("/custom_nsp", to_event_value(DATA, "event"), 0)),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet binary event on /", |b| {
        b.iter_batched(
            || encode(Packet::event("/", to_event_value((DATA, BINARY), "event"))),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet binary event on /custom_nsp", |b| {
        b.iter_batched(
            || {
                encode(Packet::event(
                    "/custom_nsp",
                    to_event_value((DATA, BINARY), "event"),
                ))
            },
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet binary ack on /", |b| {
        b.iter_batched(
            || encode(Packet::ack("/", to_value((DATA, BINARY)), 0)),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.bench_function("Decode packet binary ack on /custom_nsp", |b| {
        b.iter_batched(
            || encode(Packet::ack("/custom_nsp", to_value((DATA, BINARY)), 0)),
            decode,
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
