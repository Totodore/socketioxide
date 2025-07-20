use bytes::Bytes;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use engineioxide::{OpenPacket, Packet};

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("engineio_packet/encode");
    group.bench_function("Encode packet open", |b| {
        let packet = Packet::Open(OpenPacket::default());
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Encode packet ping/pong", |b| {
        let packet = Packet::Ping;
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Encode packet ping/pong upgrade", |b| {
        let packet = Packet::PingUpgrade;
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Encode packet message", |b| {
        let packet = Packet::Message(black_box("Hello").into());
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Encode packet noop", |b| {
        let packet = Packet::Noop;
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });
    group.bench_function("Encode packet binary b64", |b| {
        const BYTES: Bytes = Bytes::from_static(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
        let packet = Packet::Binary(BYTES);
        b.iter_batched(
            || packet.clone(),
            TryInto::<String>::try_into,
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
