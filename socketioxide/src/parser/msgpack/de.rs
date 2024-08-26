use core::str;
use std::{borrow::Cow, io::Cursor, ops::Range};

use crate::{
    packet::{BinaryPacket, Packet, PacketData},
    parser::value::ParseError,
    Value,
};
use bytes::{Buf, BufMut, Bytes};
use engineioxide::Str;
use rmp::decode::{read_map_len, RmpRead, ValueReadError};
use rmp_serde::decode::Error as DecodeError;

pub fn deserialize_packet(buff: Bytes) -> Result<Packet<'static>, ParseError> {
    let mut reader = Cursor::new(buff);
    let maplen = read_map_len(&mut reader).map_err(|e| {
        use DecodeError::*;
        match e {
            ValueReadError::InvalidMarkerRead(e) => InvalidMarkerRead(e),
            ValueReadError::InvalidDataRead(e) => InvalidDataRead(e),
            ValueReadError::TypeMismatch(e) => TypeMismatch(e),
        }
    })?;

    // Bound check to prevent DoS attacks.
    // other implementations might add some other keys that we don't support
    // Therefore, we limit the number of keys to 20
    if maplen == 0 || maplen > 20 {
        Err(DecodeError::Uncategorized(format!(
            "packet length too big or empty: {}",
            maplen
        )))?;
    }

    let mut index = 0xff;
    let mut nsp = Str::default();
    let mut data_pos: Range<usize> = 0..0;
    let mut id = None;

    for _ in 0..maplen {
        parse_key_value(&mut reader, &mut index, &mut nsp, &mut data_pos, &mut id)?;
    }
    let buff = reader.into_inner();
    let inner = match index {
        0 => PacketData::Connect(
            (!data_pos.is_empty()).then(|| Value::MsgPack(buff.slice(data_pos))),
        ),
        1 => PacketData::Disconnect,
        2 => {
            let (event, data) = read_data_event(buff.slice(data_pos))?;
            PacketData::Event(Cow::Owned(event.into()), Value::MsgPack(data), id)
        }
        3 => PacketData::EventAck(
            Value::MsgPack(buff.slice(data_pos)),
            id.ok_or(DecodeError::Uncategorized(
                "event ack id not present".to_string(),
            ))?,
        ),
        4 => {
            #[derive(serde::Deserialize)]
            struct ErrorMessage {
                message: String,
            }
            let ErrorMessage { message } = rmp_serde::decode::from_slice(&buff[data_pos])?;
            PacketData::ConnectError(message)
        }
        5 => {
            let (event, data) = read_data_event(buff.slice(data_pos))?;
            PacketData::BinaryEvent(
                Cow::Owned(event.into()),
                BinaryPacket::new(Value::MsgPack(data), vec![]),
                id,
            )
        }
        6 => PacketData::BinaryAck(
            BinaryPacket {
                data: Value::MsgPack(buff.slice(data_pos)),
                bin: Vec::new(),
            },
            id.ok_or(DecodeError::Uncategorized(
                "event ack id not present".to_string(),
            ))?,
        ),
        _ => todo!(),
    };

    Ok(Packet { inner, ns: nsp })
}

fn parse_key_value(
    reader: &mut Cursor<Bytes>,
    index: &mut usize,
    nsp: &mut Str,
    data_pos: &mut Range<usize>,
    id: &mut Option<i64>,
) -> Result<(), DecodeError> {
    let key = read_str(reader)?;

    match key {
        "type" => {
            *index = rmp::decode::read_int::<usize, _>(reader)?;
        }
        "nsp" => {
            let ns = read_str(reader)?;
            *nsp = Str::copy_from_slice(ns);
        }
        "data" => {
            let start = reader.position() as usize;
            move_to_next_element(reader)?;
            let end = reader.position() as usize;
            *data_pos = start..end;
        }
        "id" => {
            *id = Some(rmp::decode::read_int::<i64, _>(reader)?);
        }
        _ => (),
    };
    Ok(())
}
fn read_u16(rd: &mut Cursor<Bytes>) -> Result<u16, DecodeError> {
    let mut buff = [0u8; 2];
    rd.read_exact_buf(&mut buff)
        .map_err(DecodeError::InvalidDataRead)?;
    Ok(u16::from_be_bytes(buff))
}
fn read_u8(rd: &mut Cursor<Bytes>) -> Result<u8, DecodeError> {
    rd.read_u8().map_err(DecodeError::InvalidDataRead)
}

fn read_u32(rd: &mut Cursor<Bytes>) -> Result<u32, DecodeError> {
    let mut buff = [0u8; 4];
    rd.read_exact_buf(&mut buff)
        .map_err(DecodeError::InvalidDataRead)?;
    Ok(u32::from_be_bytes(buff))
}
/// Read a str slice and return the previous position in the buffer
fn read_str<'a>(reader: &'a mut Cursor<Bytes>) -> Result<&'a str, DecodeError> {
    let len = rmp::decode::read_str_len(reader)? as usize;
    let start = reader.position() as usize;
    let end = start + len;
    reader.advance(len);
    Ok(str::from_utf8(&reader.get_ref()[start..end])?)
}

/// Parse the data field for event types: `[event, ...data]`.
/// * The event is returned as a validated slice of the buffer without allocation.
/// * The data buffer is a newly allocated buffer.
fn read_data_event(bytes: Bytes) -> Result<(Str, Bytes), DecodeError> {
    let mut reader = bytes.as_ref();
    let mut len = rmp::decode::read_array_len(&mut reader)?;
    if len == 0 {
        return Err(DecodeError::LengthMismatch(2));
    }

    let strlen = rmp::decode::read_str_len(&mut reader)? as usize;
    let start = bytes.len() - reader.len() as usize;
    let end = start + strlen;
    str::from_utf8(&bytes[start..end])?; // ensure slice is valid utf8
    let event = unsafe { Str::from_bytes_unchecked(bytes.slice(start..end)) };
    reader = &reader[event.len()..];

    len -= 1; // remove the event element
    let len_size = if len < 16 {
        1 // Marker::FixArray(len as u8)
    } else if len <= u16::MAX as u32 {
        3 // Marker::Array16
    } else {
        5 // Marker::Array32
    };
    let mut data = Vec::with_capacity(len_size + reader.remaining());
    rmp::encode::write_array_len(&mut data, len).unwrap();
    debug_assert!(data.len() == len_size);
    data.put_slice(reader);

    Ok((event, data.into()))
}

/// Iterate over the next element
fn move_to_next_element(reader: &mut Cursor<Bytes>) -> Result<(), DecodeError> {
    let marker = rmp::decode::read_marker(reader)?;
    match marker {
        rmp::Marker::FixPos(_)
        | rmp::Marker::FixNeg(_)
        | rmp::Marker::Null
        | rmp::Marker::Reserved
        | rmp::Marker::False
        | rmp::Marker::True => (),
        rmp::Marker::FixMap(n) => {
            for _ in 0..n * 2 {
                move_to_next_element(reader)?
            }
        }
        rmp::Marker::FixArray(n) => {
            for _ in 0..n {
                move_to_next_element(reader)?
            }
        }
        rmp::Marker::FixStr(n) => reader.advance(n as usize),
        rmp::Marker::FixExt1 => reader.advance(2),
        rmp::Marker::FixExt2 => reader.advance(3),
        rmp::Marker::FixExt4 => reader.advance(5),
        rmp::Marker::FixExt8 => reader.advance(9),
        rmp::Marker::FixExt16 => reader.advance(17),
        rmp::Marker::U8 | rmp::Marker::I8 => reader.advance(1),
        rmp::Marker::U16 | rmp::Marker::I16 => reader.advance(2),
        rmp::Marker::F32 | rmp::Marker::U32 | rmp::Marker::I32 => reader.advance(4),
        rmp::Marker::F64 | rmp::Marker::U64 | rmp::Marker::I64 => reader.advance(8),
        rmp::Marker::Str8 | rmp::Marker::Bin8 => {
            let len = read_u8(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Str16 | rmp::Marker::Bin16 => {
            let len = read_u16(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Str32 | rmp::Marker::Bin32 => {
            let len = read_u32(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Ext8 => {
            let len = read_u8(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Ext16 => {
            let len = read_u16(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Ext32 => {
            let len = read_u32(reader)?;
            reader.advance(len as usize)
        }
        rmp::Marker::Array16 => {
            let arrlen = read_u16(reader)? as usize;
            for _ in 0..arrlen {
                move_to_next_element(reader)?;
            }
        }
        rmp::Marker::Array32 => {
            let arrlen = read_u32(reader)? as usize;
            for _ in 0..arrlen {
                move_to_next_element(reader)?;
            }
        }
        rmp::Marker::Map16 => {
            let maplen = read_u16(reader)? as usize;
            for _ in 0..maplen * 2 {
                move_to_next_element(reader)?;
            }
        }
        rmp::Marker::Map32 => {
            let len = read_u32(reader)? as usize;
            for _ in 0..len * 2 {
                move_to_next_element(reader)?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use ::bytes::Bytes;
    use rmp::Marker;

    fn create_packet_with_type(packet_type: u8, nsp: &'static str, len: u32) -> Vec<u8> {
        let mut data = Vec::new();
        rmp::encode::write_map_len(&mut data, len + 2).unwrap(); // Map length is 1
        rmp::encode::write_str(&mut data, "type").unwrap();
        rmp::encode::write_pfix(&mut data, packet_type).unwrap();
        rmp::encode::write_str(&mut data, "nsp").unwrap();
        rmp::encode::write_str(&mut data, nsp).unwrap();
        data
    }

    #[test]
    fn deserialize_connect_packet() {
        let mut data = create_packet_with_type(0, "/", 1);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_str(&mut data, "connect_data").unwrap();
        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::Connect(Some(Value::MsgPack(data))) = packet.inner {
            assert!(matches!(
                rmp_serde::decode::from_slice::<&str>(&data),
                Ok("connect_data")
            ));
        } else {
            panic!(
                "Expected PacketData::Connect with data, found: {:?}",
                packet
            );
        }
    }

    #[test]
    fn deserialize_disconnect_packet() {
        let data = create_packet_with_type(1, "/", 0);

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::Disconnect = packet.inner {
            // Successfully deserialized Disconnect packet
        } else {
            panic!("Expected PacketData::Disconnect");
        }
    }

    #[test]
    fn deserialize_event_packet() {
        let mut data = create_packet_with_type(2, "/", 1);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_array_len(&mut data, 2).unwrap(); // Event name and one argument
        rmp::encode::write_str(&mut data, "event_name").unwrap();
        rmp::encode::write_str(&mut data, "event_data").unwrap();

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::Event(event_name, Value::MsgPack(data), _) = packet.inner {
            assert_eq!(event_name, "event_name");
            rmp_serde::decode::from_slice::<(&str,)>(&data).unwrap();
            assert!(matches!(
                rmp_serde::decode::from_slice::<(&str,)>(&data),
                Ok(("event_data",))
            ));
        } else {
            panic!("Expected PacketData::Event");
        }
    }

    #[test]
    fn deserialize_event_ack_packet() {
        let mut data = create_packet_with_type(3, "/", 2);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_array_len(&mut data, 1).unwrap();
        rmp::encode::write_str(&mut data, "ack_data").unwrap();
        rmp::encode::write_str(&mut data, "id").unwrap();
        rmp::encode::write_sint(&mut data, 123).unwrap();

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::EventAck(Value::MsgPack(data), id) = packet.inner {
            assert_eq!(id, 123);
            assert!(matches!(
                rmp_serde::decode::from_slice::<(&str,)>(&data),
                Ok(("ack_data",))
            ));
        } else {
            panic!("Expected PacketData::EventAck");
        }
    }

    #[test]
    fn deserialize_connect_error_packet() {
        let mut data = create_packet_with_type(4, "/", 1);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_map_len(&mut data, 1).unwrap();
        rmp::encode::write_str(&mut data, "message").unwrap();
        rmp::encode::write_str(&mut data, "error_message").unwrap();

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::ConnectError(error_message) = packet.inner {
            assert_eq!(error_message, "error_message");
        } else {
            panic!("Expected PacketData::ConnectError");
        }
    }

    #[test]
    fn deserialize_binary_event_packet() {
        let mut data = create_packet_with_type(5, "/", 1);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_array_len(&mut data, 2).unwrap(); // Event name and one argument
        rmp::encode::write_str(&mut data, "binary_event").unwrap();
        rmp::encode::write_map_len(&mut data, 2).unwrap(); // { binary_data: [1, 2, 3, 4], test: 1 }
        rmp::encode::write_str(&mut data, "binary_data").unwrap();
        rmp::encode::write_bin(&mut data, &[1, 2, 3, 4]).unwrap();
        rmp::encode::write_str(&mut data, "test").unwrap();
        rmp::encode::write_sint(&mut data, 1).unwrap();

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::BinaryEvent(
            event_name,
            BinaryPacket {
                data: Value::MsgPack(data),
                bin,
            },
            _,
        ) = packet.inner
        {
            #[derive(serde::Deserialize, Debug)]
            struct TestStruct {
                binary_data: Vec<u8>,
                test: i32,
            }
            dbg!(rmp_serde::decode::from_slice::<(TestStruct,)>(&data).unwrap());
            assert_eq!(event_name, "binary_event");
            assert!(matches!(
                    rmp_serde::decode::from_slice::<(TestStruct,)>(&data),
                    Ok((TestStruct { binary_data, test },)) if binary_data == [1, 2, 3, 4] && test == 1
            ));
            assert!(bin.is_empty());
        } else {
            panic!("Expected PacketData::BinaryEvent");
        }
    }

    #[test]
    fn deserialize_binary_ack_packet() {
        let mut data = create_packet_with_type(6, "/", 2);
        rmp::encode::write_str(&mut data, "data").unwrap();
        rmp::encode::write_array_len(&mut data, 1).unwrap();
        rmp::encode::write_map_len(&mut data, 2).unwrap(); // { binary_data: [1, 2, 3, 4], test: 1 }
        rmp::encode::write_str(&mut data, "binary_data").unwrap();
        rmp::encode::write_bin(&mut data, &[1, 2, 3, 4]).unwrap();
        rmp::encode::write_str(&mut data, "test").unwrap();
        rmp::encode::write_sint(&mut data, 1).unwrap();

        rmp::encode::write_str(&mut data, "id").unwrap();
        rmp::encode::write_sint(&mut data, 456).unwrap();

        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        if let PacketData::BinaryAck(
            BinaryPacket {
                data: Value::MsgPack(data),
                bin,
            },
            id,
        ) = packet.inner
        {
            #[derive(serde::Deserialize, PartialEq, Debug)]
            struct TestStruct {
                binary_data: Vec<u8>,
                test: i32,
            }
            assert_eq!(id, 456);
            assert!(matches!(
                    rmp_serde::decode::from_slice(&data),
                    Ok((TestStruct { binary_data, test },)) if binary_data == [1, 2, 3, 4] && test == 1
            ));
            assert!(bin.is_empty());
        } else {
            panic!("Expected PacketData::BinaryAck");
        }
    }

    #[test]
    pub fn data_bytelen_fixpos() {
        let mut data = Vec::new();
        rmp::encode::write_pfix(&mut data, 0).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_fixneg() {
        let mut data = Vec::new();
        rmp::encode::write_nfix(&mut data, -1).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_fixmap() {
        let mut data = Vec::new();
        data.push(Marker::FixMap(1).to_u8());
        rmp::encode::write_str(&mut data, "test").unwrap();
        rmp::encode::write_u8(&mut data, 132).unwrap(); // { "test": 132 }

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_fixarray() {
        let mut data = Vec::new();
        data.push(Marker::FixArray(3).to_u8());
        rmp::encode::write_str(&mut data, "foo").unwrap(); // ["foo"]
        rmp::encode::write_bin(&mut data, &[1, 2, 3]).unwrap();
        rmp::encode::write_bin(&mut data, &[4, 5, 6]).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_fixstr() {
        let mut data = Vec::new();
        rmp::encode::write_str(&mut data, "a").unwrap(); // ["a"]
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_null() {
        let mut data = Vec::new();
        data.push(Marker::Null.to_u8());
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_reserved() {
        let mut data = Vec::new();
        data.push(Marker::Reserved.to_u8());
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_false() {
        let mut data = Vec::new();
        data.push(Marker::False.to_u8());
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_true() {
        let mut data = Vec::new();
        data.push(Marker::True.to_u8());
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_u8() {
        let mut data = Vec::new();
        rmp::encode::write_u8(&mut data, 0xff).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_u16() {
        let mut data = Vec::new();
        rmp::encode::write_u16(&mut data, 0xffff).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_u32() {
        let mut data = Vec::new();
        rmp::encode::write_u32(&mut data, 0xffffff).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_u64() {
        let mut data = Vec::new();
        rmp::encode::write_u32(&mut data, 0xffffffff).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_i8() {
        let mut data = Vec::new();
        rmp::encode::write_i8(&mut data, 1).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_i16() {
        let mut data = Vec::new();
        rmp::encode::write_i16(&mut data, 13).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_i32() {
        let mut data = Vec::new();
        rmp::encode::write_i32(&mut data, 132).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_i64() {
        let mut data = Vec::new();
        rmp::encode::write_i16(&mut data, 123).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_f32() {
        let mut data = Vec::new();
        rmp::encode::write_f32(&mut data, 23.1).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_f64() {
        let mut data = Vec::new();
        rmp::encode::write_f64(&mut data, 23.1).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_bin8() {
        let mut data = Vec::new();
        rmp::encode::write_bin(&mut data, &[b't', b'e', b's', b't']).unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_bin16() {
        let mut data = Vec::new();
        data.push(Marker::Bin16.to_u8());
        data.extend_from_slice(&[0x00, 0x04, b't', b'e', b's', b't']);
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_bin32() {
        let mut data = Vec::new();
        data.push(Marker::Bin32.to_u8());
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04, b't', b'e', b's', b't']);

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_str8() {
        let mut data = Vec::new();
        rmp::encode::write_str(&mut data, "test").unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_str16() {
        let mut data = Vec::new();
        data.push(Marker::Str16.to_u8());
        data.extend_from_slice(&[0x00, 0x04, b't', b'e', b's', b't']);
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_str32() {
        let mut data = Vec::new();
        data.push(Marker::Str32.to_u8());
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04, b't', b'e', b's', b't']);

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_array16() {
        let mut data = Vec::new();
        data.push(Marker::Array16.to_u8());
        data.extend_from_slice(&[0x00, 0x02]);
        rmp::encode::write_str(&mut data, "foo").unwrap();
        rmp::encode::write_str(&mut data, "foo").unwrap();

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_array32() {
        let mut data = Vec::new();
        data.push(Marker::Array32.to_u8());
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]);
        rmp::encode::write_str(&mut data, "foo").unwrap();
        rmp::encode::write_str(&mut data, "foo").unwrap();

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_map16() {
        let mut data = Vec::new();
        data.push(Marker::Map16.to_u8());
        data.extend_from_slice(&[0x00, 0x01]);
        rmp::encode::write_str(&mut data, "foo").unwrap();
        rmp::encode::write_str(&mut data, "foo").unwrap();

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }
    #[test]
    pub fn data_bytelen_map32() {
        let mut data = Vec::new();
        data.push(Marker::Map32.to_u8());
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        rmp::encode::write_str(&mut data, "foo").unwrap();
        rmp::encode::write_str(&mut data, "foo").unwrap();

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }
}
