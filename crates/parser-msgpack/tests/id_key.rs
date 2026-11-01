//! Regression tests for the `id` key of a packet.
//!
//! When the `id` key was the last element of a packet, without any value,
//! the decoder advanced past the end of the buffer and panicked.

use bytes::Bytes;
use socketioxide_core::{
    packet::{Packet, PacketData},
    parser::{Parse, ParseError},
};
use socketioxide_parser_msgpack::MsgPackParser;

fn decode(data: &[u8]) -> Result<Packet, ParseError> {
    MsgPackParser.decode_bin(
        &Default::default(),
        &Default::default(),
        Bytes::copy_from_slice(data),
    )
}

/// `{ "type": <type>, "nsp": "/", "data": [], "id": <id> }`
fn packet(r#type: u8, id: &[u8]) -> Vec<u8> {
    let mut buff = vec![];
    rmp::encode::write_map_len(&mut buff, 4).unwrap();
    rmp::encode::write_str(&mut buff, "type").unwrap();
    rmp::encode::write_uint(&mut buff, r#type as u64).unwrap();
    rmp::encode::write_str(&mut buff, "nsp").unwrap();
    rmp::encode::write_str(&mut buff, "/").unwrap();
    rmp::encode::write_str(&mut buff, "data").unwrap();
    rmp::encode::write_array_len(&mut buff, 0).unwrap();
    rmp::encode::write_str(&mut buff, "id").unwrap();
    buff.extend_from_slice(id);
    buff
}

#[test]
fn id_key_without_value_alone() {
    let err = decode(&[0x81, 0xa2, b'i', b'd']).unwrap_err();
    assert!(matches!(err, ParseError::ParserError(_)), "{err:?}");
}

#[test]
fn id_key_without_value_after_other_keys() {
    let err = decode(&packet(2, &[])).unwrap_err();
    assert!(matches!(err, ParseError::ParserError(_)), "{err:?}");
}

#[test]
fn nil_id() {
    let packet = decode(&packet(2, &[0xc0])).unwrap();
    assert!(matches!(packet.inner, PacketData::Event(_, None)));
}

#[test]
fn integer_id() {
    let packet = decode(&packet(2, &[0x07])).unwrap();
    assert!(matches!(packet.inner, PacketData::Event(_, Some(7))));
}

#[test]
fn nil_id_on_ack_is_rejected() {
    let err = decode(&packet(3, &[0xc0])).unwrap_err();
    assert!(matches!(err, ParseError::InvalidAckId), "{err:?}");
}

/// The bytes following a nil id must still be read: only the nil byte is consumed.
#[test]
fn nil_id_followed_by_another_key() {
    let mut buff = vec![];
    rmp::encode::write_map_len(&mut buff, 4).unwrap();
    rmp::encode::write_str(&mut buff, "id").unwrap();
    buff.push(0xc0);
    rmp::encode::write_str(&mut buff, "type").unwrap();
    rmp::encode::write_uint(&mut buff, 2).unwrap();
    rmp::encode::write_str(&mut buff, "nsp").unwrap();
    rmp::encode::write_str(&mut buff, "/custom").unwrap();
    rmp::encode::write_str(&mut buff, "data").unwrap();
    rmp::encode::write_array_len(&mut buff, 0).unwrap();

    let packet = decode(&buff).unwrap();
    assert_eq!(packet.ns, "/custom");
    assert!(matches!(packet.inner, PacketData::Event(_, None)));
}
