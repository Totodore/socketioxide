//! Regression tests for the recursion depth limit of the msgpack packet decoder.
//!
//! A malicious client could send a packet with deeply nested containers in the `data`
//! field, triggering unbounded recursion in the decoder and crashing the whole process
//! with a stack overflow. The decoder must instead reject such packets with an error.

use bytes::Bytes;
use socketioxide_core::packet::PacketData;
use socketioxide_core::parser::Parse;
use socketioxide_parser_msgpack::MsgPackParser;

/// Build a raw event packet `{ "type": 2, "nsp": "/", "data": <data> }`
/// with an arbitrary (potentially malicious) msgpack-encoded `data` field.
fn event_packet(data: &[u8]) -> Bytes {
    let mut buff = Vec::with_capacity(data.len() + 16);
    rmp::encode::write_map_len(&mut buff, 3).unwrap();
    rmp::encode::write_str(&mut buff, "type").unwrap();
    rmp::encode::write_uint(&mut buff, 2).unwrap();
    rmp::encode::write_str(&mut buff, "nsp").unwrap();
    rmp::encode::write_str(&mut buff, "/").unwrap();
    rmp::encode::write_str(&mut buff, "data").unwrap();
    buff.extend_from_slice(data);
    buff.into()
}

/// `n` nested `FixArray(1)` with a nil innermost element: `[[[...nil...]]]`
fn nested_arrays(n: usize) -> Vec<u8> {
    let mut data = vec![rmp::Marker::FixArray(1).to_u8(); n];
    data.push(rmp::Marker::Null.to_u8());
    data
}

#[test]
fn deeply_nested_packet_is_rejected_not_crash() {
    let packet = event_packet(&nested_arrays(50_000));
    // Decode on a thread with a small stack so that, without the depth limit,
    // the stack overflow is deterministic whatever the test runner's stack size.
    let res = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || MsgPackParser.decode_bin(&Default::default(), &Default::default(), packet))
        .unwrap()
        .join()
        .unwrap();
    let err = res.unwrap_err();
    assert!(
        err.to_string().contains("DepthLimitExceeded"),
        "unexpected error: {err}"
    );
}

#[test]
fn nested_within_limit_is_accepted() {
    let packet = event_packet(&nested_arrays(100));
    let packet = MsgPackParser
        .decode_bin(&Default::default(), &Default::default(), packet)
        .unwrap();
    assert!(matches!(packet.inner, PacketData::Event(_, None)));
}

#[test]
fn large_flat_payload_is_accepted() {
    // way more elements than the depth limit, but only 2 levels deep
    let mut data = vec![rmp::Marker::Array16.to_u8()];
    data.extend_from_slice(&1000u16.to_be_bytes());
    data.extend(vec![rmp::Marker::Null.to_u8(); 1000]);

    let packet = event_packet(&data);
    let packet = MsgPackParser
        .decode_bin(&Default::default(), &Default::default(), packet)
        .unwrap();
    assert!(matches!(packet.inner, PacketData::Event(_, None)));
}
