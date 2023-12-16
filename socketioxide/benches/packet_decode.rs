use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engineioxide::sid::Sid;
use socketioxide::{
    packet::{Packet, PacketData},
    ProtocolVersion,
};
fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("Decode packet connect on /", |b| {
        let packet: String =
            Packet::connect(black_box("/"), black_box(Sid::ZERO), ProtocolVersion::V5)
                .try_into()
                .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });
    c.bench_function("Decode packet connect on /custom_nsp", |b| {
        let packet: String = Packet::connect(
            black_box("/custom_nsp"),
            black_box(Sid::ZERO),
            ProtocolVersion::V5,
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    const DATA: &str = r#"{"_placeholder":true,"num":0}"#;
    const BINARY: [u8; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    c.bench_function("Decode packet event on /", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String =
            Packet::event(black_box("/"), black_box("event"), black_box(data.clone()))
                .try_into()
                .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet event on /custom_nsp", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::event(
            black_box("custom_nsp"),
            black_box("event"),
            black_box(data.clone()),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet event with ack on /", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: Packet =
            Packet::event(black_box("/"), black_box("event"), black_box(data.clone()));
        match packet.inner {
            PacketData::Event(_, _, mut ack) => ack.insert(black_box(0)),
            _ => panic!("Wrong packet type"),
        };
        let packet: String = packet.try_into().unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet event with ack on /custom_nsp", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet = Packet::event(
            black_box("/custom_nsp"),
            black_box("event"),
            black_box(data.clone()),
        );
        match packet.inner {
            PacketData::Event(_, _, mut ack) => ack.insert(black_box(0)),
            _ => panic!("Wrong packet type"),
        };
        let packet: String = packet.try_into().unwrap();

        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet ack on /", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::ack(black_box("/"), black_box(data.clone()), black_box(0))
            .try_into()
            .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet ack on /custom_nsp", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::ack(
            black_box("/custom_nsp"),
            black_box(data.clone()),
            black_box(0),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet binary event (b64) on /", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::bin_event(
            black_box("/"),
            black_box("event"),
            black_box(data.clone()),
            black_box(vec![BINARY.to_vec().clone()]),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet binary event (b64) on /custom_nsp", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::bin_event(
            black_box("/custom_nsp"),
            black_box("event"),
            black_box(data.clone()),
            black_box(vec![BINARY.to_vec().clone()]),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet binary ack (b64) on /", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::bin_ack(
            black_box("/"),
            black_box(data.clone()),
            black_box(vec![BINARY.to_vec().clone()]),
            black_box(0),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });

    c.bench_function("Decode packet binary ack (b64) on /custom_nsp", |b| {
        let data = serde_json::to_value(DATA).unwrap();
        let packet: String = Packet::bin_ack(
            black_box("/custom_nsp"),
            black_box(data.clone()),
            black_box(vec![BINARY.to_vec().clone()]),
            black_box(0),
        )
        .try_into()
        .unwrap();
        b.iter(|| Packet::try_from(packet.clone()).unwrap())
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
