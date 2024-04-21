use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use engineioxide::Packet;

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("engineio_packet/decode");
    group.bench_function("Decode packet ping/pong", |b| {
        let packet: String = Packet::Ping.try_into().unwrap();
        b.iter_batched(
            || packet.clone(),
            |p| Packet::try_from(p).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode packet ping/pong upgrade", |b| {
        let packet: String = Packet::PingUpgrade.try_into().unwrap();
        b.iter_batched(
            || packet.clone(),
            |p| Packet::try_from(p).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode packet message", |b| {
        let packet: String = Packet::Message(black_box("Hello").into())
            .try_into()
            .unwrap();
        b.iter_batched(
            || packet.clone(),
            |p| Packet::try_from(p).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode packet noop", |b| {
        let packet: String = Packet::Noop.try_into().unwrap();
        b.iter_batched(
            || packet.clone(),
            |p| Packet::try_from(p).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Decode packet binary b64", |b| {
        const BYTES: Bytes = Bytes::from_static(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
        let packet: String = Packet::Binary(BYTES).try_into().unwrap();
        b.iter_batched(
            || packet.clone(),
            |p| Packet::try_from(p).unwrap(),
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
