use core::str;
use std::{io::Cursor, ops::Range};

use bytes::{Buf, Bytes};
use rmp::{
    decode::{self, read_map_len, RmpRead, ValueReadError},
    Marker,
};
use rmp_serde::decode::Error as DecodeError;
use socketioxide_core::{
    packet::{Packet, PacketData},
    parser::ParseError,
    Str, Value,
};

pub fn deserialize_packet(buff: Bytes) -> Result<Packet, ParseError<DecodeError>> {
    let mut reader = Cursor::new(buff);
    let maplen = read_map_len(&mut reader).map_err(|e| {
        use DecodeError::*;
        let e = match e {
            ValueReadError::InvalidMarkerRead(e) => InvalidMarkerRead(e),
            ValueReadError::InvalidDataRead(e) => InvalidDataRead(e),
            ValueReadError::TypeMismatch(e) => TypeMismatch(e),
        };
        ParseError::ParserError(e)
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
    let mut data = buff.slice(data_pos.clone());

    // Current js socket.io msgpack implementation has a weird way to represent "undefined" with zeroed 1-ext.
    // This is a little workaround to convert this to a proper "undefined" value.
    if data.as_ref() == &[0xd4, 0x00, 0x00] {
        data = Bytes::from_static(&[0xc0]); // nil
    };

    let data = Value::Bytes(data);
    let inner = match index {
        // some implementations might completely omit the data field
        0 => PacketData::Connect((!data_pos.is_empty()).then_some(data)),
        1 => PacketData::Disconnect,
        2 => PacketData::Event(data, id),
        3 => PacketData::EventAck(data, id.ok_or(ParseError::InvalidAckId)?),
        4 => {
            #[derive(serde::Deserialize)]
            struct ErrorMessage {
                message: String,
            }
            let ErrorMessage { message } = rmp_serde::decode::from_slice(&buff[data_pos])?;
            PacketData::ConnectError(message)
        }
        5 => PacketData::BinaryEvent(data, id),
        6 => PacketData::BinaryAck(data, id.ok_or(ParseError::InvalidAckId)?),
        _ => Err(ParseError::InvalidPacketType)?,
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
            *index = decode::read_int::<usize, _>(reader)?;
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
            *id = match reader
                .get_ref()
                .get(reader.position() as usize)
                .map(|b| Marker::from_u8(*b))       //TODO: use remaining_slice when stabilized (issue #86369)
            {
                Some(Marker::Null) | None => {
                    reader.advance(1);
                    None
                }
                Some(_) => Some(decode::read_int::<i64, _>(reader)?),
            }
        }
        _ => move_to_next_element(reader)?, // Skip the data corresponding to the key
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
/// Read a str slice
fn read_str(reader: &mut Cursor<Bytes>) -> Result<&str, DecodeError> {
    let len = decode::read_str_len(reader)? as usize;
    let start = reader.position() as usize;
    let end = start + len;
    reader.advance(len);
    Ok(str::from_utf8(&reader.get_ref()[start..end])?)
}

/// Iterate over the next element
fn move_to_next_element(reader: &mut Cursor<Bytes>) -> Result<(), DecodeError> {
    let marker = decode::read_marker(reader)?;
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
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use socketioxide_core::Value;

    const BIN: Bytes = Bytes::from_static(&[1, 2, 3, 4]);
    #[derive(Serialize)]
    struct StubPacket<T: serde::Serialize> {
        r#type: usize,
        nsp: &'static str,
        id: Option<i64>,
        data: T,
    }

    fn packet(
        r#type: usize,
        nsp: &'static str,
        data: impl serde::Serialize,
        id: Option<i64>,
    ) -> Vec<u8> {
        rmp_serde::to_vec_named(&StubPacket {
            r#type,
            nsp,
            data,
            id,
        })
        .unwrap()
    }

    #[test]
    fn deserialize_connect_packet() {
        let data = packet(0, "/", "connect_data", None);
        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::Connect(Some(Value::Bytes(data))),
                ns,
            } if ns == "/" => assert_eq!(
                rmp_serde::decode::from_slice::<&str>(&data).unwrap(),
                "connect_data"
            ),
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[test]
    fn deserialize_disconnect_packet() {
        let data = packet(1, "/", (), None);

        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::Disconnect,
                ns,
            } if ns == "/" => (),
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[test]
    fn deserialize_event_packet() {
        let data = packet(2, "/", ["event_name", "event_data"], None);
        let packet = deserialize_packet(Bytes::from(data)).unwrap();

        match packet {
            Packet {
                inner: PacketData::Event(Value::Bytes(data), None),
                ns,
            } if ns == "/" => assert_eq!(
                rmp_serde::decode::from_slice::<(&str, &str)>(&data).unwrap(),
                ("event_name", "event_data")
            ),
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[test]
    fn deserialize_event_ack_packet() {
        let data = packet(3, "/", ["ack_data"], Some(123));
        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::EventAck(Value::Bytes(data), id),
                ns,
            } if id == 123 && ns == "/" => {
                assert_eq!(
                    rmp_serde::decode::from_slice::<(&str,)>(&data).unwrap(),
                    ("ack_data",)
                )
            }
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[test]
    fn deserialize_connect_error_packet() {
        let data = packet(4, "/", json!({ "message": "error_message" }), None);
        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::ConnectError(error_message),
                ns,
            } => {
                if ns == "/" {
                    assert_eq!(error_message, "error_message");
                }
            }
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct Data {
        binary_data: Bytes,
        test: usize,
    }
    #[test]
    fn deserialize_binary_event_packet() {
        let bin_data = Data {
            binary_data: BIN,
            test: 1,
        };
        let data = packet(5, "/", ("binary_event", bin_data.clone()), None);
        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::BinaryEvent(Value::Bytes(data), None),
                ns,
            } if ns == "/" => {
                assert_eq!(
                    rmp_serde::from_slice::<(&str, Data)>(&data).unwrap(),
                    ("binary_event", bin_data)
                );
            }
            _ => panic!("invalid packet: {:?}", packet),
        }
    }

    #[test]
    fn deserialize_binary_ack_packet() {
        let bin_data = Data {
            binary_data: BIN,
            test: 1,
        };
        let data = packet(6, "/", [bin_data.clone()], Some(456));
        let packet = deserialize_packet(Bytes::from(data)).unwrap();
        match packet {
            Packet {
                inner: PacketData::BinaryAck(Value::Bytes(data), id),
                ns,
            } if ns == "/" && id == 456 => {
                assert_eq!(
                    rmp_serde::from_slice::<(Data,)>(&data).unwrap(),
                    (bin_data.clone(),)
                )
            }
            _ => panic!("invalid packet: {:?}", packet),
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
        let mut data = vec![Marker::FixMap(1).to_u8()];
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
        let mut data = vec![Marker::FixArray(3).to_u8()];
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
        let data = vec![Marker::Null.to_u8()];
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_reserved() {
        let data = vec![Marker::Reserved.to_u8()];
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_false() {
        let data = vec![Marker::False.to_u8()];
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_true() {
        let data = vec![Marker::True.to_u8()];
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
        rmp::encode::write_bin(&mut data, b"test").unwrap();
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_bin16() {
        let mut data = vec![Marker::Bin16.to_u8()];
        data.extend_from_slice(&[0x00, 0x04, b't', b'e', b's', b't']);
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_bin32() {
        let mut data = vec![Marker::Bin32.to_u8()];
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
        let mut data = vec![Marker::Str16.to_u8()];
        data.extend_from_slice(&[0x00, 0x04, b't', b'e', b's', b't']);
        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_str32() {
        let mut data = vec![Marker::Str32.to_u8()];
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04, b't', b'e', b's', b't']);

        let len = data.len();
        let mut reader = Cursor::new(data.into());
        move_to_next_element(&mut reader).unwrap();
        let bytelen = reader.position() as usize;
        assert_eq!(bytelen, len);
    }

    #[test]
    pub fn data_bytelen_array16() {
        let mut data = vec![Marker::Array16.to_u8()];
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
        let mut data = vec![Marker::Array32.to_u8()];
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
        let mut data = vec![Marker::Map16.to_u8()];
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
        let mut data = vec![Marker::Map32.to_u8()];
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
