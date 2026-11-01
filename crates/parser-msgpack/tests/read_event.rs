//! Regression tests for the event name length read by [`MsgPackParser::read_event`].
//!
//! The declared length of the event name was used to slice the data without any check.

use bytes::Bytes;
use socketioxide_core::{Value, parser::Parse};
use socketioxide_parser_msgpack::MsgPackParser;

fn read_event(data: &'static [u8]) -> Result<String, String> {
    let value = Value::Bytes(Bytes::from_static(data));
    MsgPackParser
        .read_event(&value)
        .map(str::to_string)
        .map_err(|e| e.to_string())
}

#[test]
fn event_name_longer_than_data() {
    for data in [
        // fixstr(3), with only 1 byte
        &[0x91, 0xa3, b'a'][..],
        // fixstr(3), with no bytes
        &[0x91, 0xa3],
        // str8(64), with only 1 byte
        &[0x91, 0xd9, 0x40, b'a'],
        // str16(65535), with only 1 byte
        &[0x91, 0xda, 0xff, 0xff, b'a'],
        // str32(u32::MAX), with only 1 byte
        &[0x91, 0xdb, 0xff, 0xff, 0xff, 0xff, b'a'],
    ] {
        assert!(read_event(data).is_err(), "{data:02x?} should be rejected");
    }
}

#[test]
fn event_name_with_exact_length() {
    assert_eq!(read_event(&[0x91, 0xa3, b'a', b'b', b'c']).unwrap(), "abc");
    assert_eq!(read_event(&[0x91, 0xa0]).unwrap(), "");
    assert_eq!(
        read_event(&[0x91, 0xd9, 0x03, b'a', b'b', b'c']).unwrap(),
        "abc"
    );
}

#[test]
fn event_name_followed_by_data() {
    assert_eq!(read_event(&[0x92, 0xa1, b'a', 0x01]).unwrap(), "a");
}

#[test]
fn invalid_event_packets() {
    for data in [
        &[][..],             // no array
        &[0x90],             // empty array
        &[0x91],             // no event name
        &[0x91, 0x01],       // the event name is not a string
        &[0x91, 0xa1, 0xff], // the event name is not valid utf8
        &[0xa1, b'a'],       // not an array
    ] {
        assert!(read_event(data).is_err(), "{data:02x?} should be rejected");
    }
}
