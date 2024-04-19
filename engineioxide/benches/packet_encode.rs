use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engineioxide::{config::EngineIoConfig, sid::Sid, OpenPacket, Packet, TransportType};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("Encode packet open", |b| {
        let packet = Packet::Open(OpenPacket::new(
            black_box(TransportType::Polling),
            black_box(Sid::ZERO),
            &EngineIoConfig::default(),
        ));
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
    c.bench_function("Encode packet ping/pong", |b| {
        let packet = Packet::Ping;
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
    c.bench_function("Encode packet ping/pong upgrade", |b| {
        let packet = Packet::PingUpgrade;
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
    c.bench_function("Encode packet message", |b| {
        let packet = Packet::Message(black_box("Hello").to_string());
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
    c.bench_function("Encode packet noop", |b| {
        let packet = Packet::Noop;
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
    c.bench_function("Encode packet binary b64", |b| {
        const BYTES: Bytes = Bytes::from_static(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
        let packet = Packet::Binary(BYTES);
        b.iter(|| TryInto::<String>::try_into(packet.clone()))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
