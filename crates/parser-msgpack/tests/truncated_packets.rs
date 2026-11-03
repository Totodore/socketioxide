//! Regression tests for the lengths declared by a packet.
//!
//! The packet decoder trusted the length of each msgpack element, and a client could
//! trigger a panic with a packet declaring more bytes than it contains.
//! The decoder must reject such packets with an error.

use bytes::Bytes;
use socketioxide_core::parser::{Parse, ParseError};
use socketioxide_parser_msgpack::MsgPackParser;

fn decode(data: impl Into<Bytes>) -> Result<(), ParseError> {
    MsgPackParser
        .decode_bin(&Default::default(), &Default::default(), data.into())
        .map(|_| ())
}

fn assert_invalid(data: &[u8]) {
    let err = decode(data.to_vec()).expect_err(&format!("{data:02x?} should not decode"));
    assert!(
        matches!(err, ParseError::ParserError(_)),
        "{data:02x?}: unexpected error {err:?}"
    );
}

/// `{ "type": 2, "nsp": "/", "data": <data> }`
fn event_packet(data: &[u8]) -> Vec<u8> {
    let mut buff = Vec::with_capacity(data.len() + 16);
    rmp::encode::write_map_len(&mut buff, 3).unwrap();
    rmp::encode::write_str(&mut buff, "type").unwrap();
    rmp::encode::write_uint(&mut buff, 2).unwrap();
    rmp::encode::write_str(&mut buff, "nsp").unwrap();
    rmp::encode::write_str(&mut buff, "/").unwrap();
    rmp::encode::write_str(&mut buff, "data").unwrap();
    buff.extend_from_slice(data);
    buff
}

#[test]
fn truncated_key() {
    // fixstr(5), with only 2 bytes
    assert_invalid(&[0x81, 0xa5, 0x74, 0x79]);
    // fixstr(3), with only 1 byte
    assert_invalid(&[0x81, 0xa3, 0x61]);
}

#[test]
fn truncated_nsp_value() {
    // { "nsp": fixstr(5) } with only 1 byte
    assert_invalid(&[0x81, 0xa3, b'n', b's', b'p', 0xa5, b'/']);
}

#[test]
fn truncated_data_value() {
    const DATA: &[u8] = b"\x81\xa4data";
    for value in [
        // str8(255), with only 1 byte
        &[0xd9, 0xff, 0x61][..],
        // str16(65535), with only 1 byte
        &[0xda, 0xff, 0xff, 0x61],
        // str32(u32::MAX), with no bytes
        &[0xdb, 0xff, 0xff, 0xff, 0xff],
        // bin8(255), bin16(65535) and bin32(u32::MAX), with no bytes
        &[0xc4, 0xff],
        &[0xc5, 0xff, 0xff],
        &[0xc6, 0xff, 0xff, 0xff, 0xff],
        // ext8(255), ext16(65535) and ext32(u32::MAX), with no bytes
        &[0xc7, 0xff],
        &[0xc8, 0xff, 0xff],
        &[0xc9, 0xff, 0xff, 0xff, 0xff],
        // fixstr(3) with no bytes
        &[0xa3],
    ] {
        assert_invalid(&[DATA, value].concat());
    }
}

#[test]
fn truncated_fixed_size_markers() {
    const DATA: &[u8] = b"\x81\xa4data";
    for marker in [
        0xcc, 0xcd, 0xce, 0xcf, // u8, u16, u32, u64
        0xd0, 0xd1, 0xd2, 0xd3, // i8, i16, i32, i64
        0xca, 0xcb, // f32, f64
        0xd4, 0xd5, 0xd6, 0xd7, 0xd8, // fixext 1, 2, 4, 8, 16
    ] {
        assert_invalid(&[DATA, &[marker]].concat());
    }
}

#[test]
fn truncated_unknown_key_value() {
    // The value of an unknown key is skipped with the same code.
    assert_invalid(&[0x81, 0xa1, b'x', 0xdb, 0xff, 0xff, 0xff, 0xff]);
}

#[test]
fn truncated_nested_value() {
    // [fixstr(3)] with no bytes
    assert_invalid(&event_packet(&[0x91, 0xa3]));
    // { "a": bin32(u32::MAX) } with no bytes
    assert_invalid(&event_packet(&[
        0x81, 0xa1, b'a', 0xc6, 0xff, 0xff, 0xff, 0xff,
    ]));
}

/// Each element is at the very end of the packet: this is not a truncated packet.
#[test]
fn exact_lengths_are_accepted() {
    let mut str8 = vec![0xd9, 200];
    str8.extend_from_slice(&[b'a'; 200]);
    let mut bin16 = vec![0xc5, 0x01, 0x00];
    bin16.extend_from_slice(&[0; 256]);
    for value in [
        &b"\xa3foo"[..],
        &str8,
        &bin16,
        &b"\xcc\x01"[..],
        &b"\xcd\x00\x01"[..],
        &b"\xce\x00\x00\x00\x01"[..],
        &b"\xcf\x00\x00\x00\x00\x00\x00\x00\x01"[..],
        &b"\xcb\x00\x00\x00\x00\x00\x00\x00\x00"[..],
        &b"\xd4\x01\x00"[..],
        &b"\xd8\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"[..],
    ] {
        decode(event_packet(value)).unwrap();
    }
}

/// Any prefix of a valid packet must either decode or return an error.
#[test]
fn every_truncation_of_a_packet_is_handled() {
    let mut data = vec![0x9a]; // array(10)
    rmp::encode::write_str(&mut data, "event").unwrap();
    rmp::encode::write_str(&mut data, &"a".repeat(40)).unwrap(); // str8
    rmp::encode::write_str(&mut data, &"a".repeat(300)).unwrap(); // str16
    rmp::encode::write_bin(&mut data, &[1; 10]).unwrap();
    rmp::encode::write_bin(&mut data, &[1; 300]).unwrap();
    rmp::encode::write_u64(&mut data, u64::MAX).unwrap();
    rmp::encode::write_i32(&mut data, -40000).unwrap();
    rmp::encode::write_f64(&mut data, 1.5).unwrap();
    data.extend_from_slice(&[0xd5, 0x01, 0x00, 0x00]); // fixext2
    data.extend_from_slice(&[0x81, 0xa1, b'a', 0xc0]); // { "a": nil }
    let packet = event_packet(&data);

    decode(packet.clone()).unwrap();
    for len in 0..packet.len() {
        let _ = decode(packet[..len].to_vec());
    }
}
