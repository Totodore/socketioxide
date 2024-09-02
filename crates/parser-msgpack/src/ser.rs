use bytes::BufMut;
use rmp::{encode, Marker};
use socketioxide_core::{
    packet::{Packet, PacketData},
    SocketIoValue,
};

/// Manual packet serialization.
pub fn serialize_packet(packet: Packet) -> Vec<u8> {
    let mut buff = Vec::new(); // TODO: with_capacity

    let map_len = get_packet_map_len(&packet);
    encode::write_map_len(&mut buff, map_len).unwrap();
    encode::write_str(&mut buff, "type").unwrap();
    encode::write_uint(&mut buff, packet.inner.index() as u64).unwrap();
    encode::write_str(&mut buff, "nsp").unwrap();
    encode::write_str(&mut buff, &packet.ns).unwrap();

    match packet.inner {
        PacketData::Event(SocketIoValue::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::BinaryEvent(SocketIoValue::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);

            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::EventAck(SocketIoValue::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            serialize_id(&mut buff, id);
        }
        PacketData::BinaryAck(SocketIoValue::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            serialize_id(&mut buff, id);
        }
        PacketData::Connect(Some(SocketIoValue::Bytes(data))) => {
            encode::write_str(&mut buff, "data").unwrap();
            buff.put_slice(&data)
        }
        PacketData::ConnectError(data) => {
            encode::write_str(&mut buff, "data").unwrap();
            encode::write_map_len(&mut buff, 1).unwrap();
            encode::write_str(&mut buff, "message").unwrap();
            encode::write_str(&mut buff, &data).unwrap();
        }
        _ => (),
    };
    buff
}

/// Serialize event data in place to the following form: `[event, ...data]` if data is an array.
/// Or `[event, data]` if data is not an array.
/// ## Params:
/// - `event`: the event name
/// - `data`: preserialized msgpack data
/// - `buff`: a output buffer to write into
fn serialize_data(data: &[u8], buff: &mut Vec<u8>) {
    buff.put_slice(data)
}

fn serialize_id(buff: &mut Vec<u8>, id: i64) {
    encode::write_str(buff, "id").unwrap();
    encode::write_sint(buff, id).unwrap();
}

fn get_packet_map_len(packet: &Packet) -> u32 {
    2 + match packet.inner {
        PacketData::Connect(Some(_)) => 1,
        PacketData::Connect(None) => 0,
        PacketData::Disconnect => 0,
        PacketData::Event(_, Some(_)) => 2,
        PacketData::Event(_, None) => 1,
        PacketData::EventAck(_, _) => 2,
        PacketData::ConnectError(_) => 1,
        PacketData::BinaryEvent(_, Some(_)) => 3,
        PacketData::BinaryEvent(_, None) => 2,
        PacketData::BinaryAck(_, _) => 3,
    }
}

#[cfg(test)]
mod tests {
    use crate::MsgPackParser;

    use super::*;
    use ::bytes::Bytes;
    use encode::*;
    use rmp::decode::*;
    use socketioxide_core::parser::Parse;
    use std::io::Cursor;
    use std::u8;

    const BIN: Bytes = Bytes::from_static(&[1, 2, 3, 4]);

    fn assert_str(reader: &mut Cursor<Vec<u8>>, val: &str) {
        let mut buff = [0; 256];
        assert_eq!(read_str(reader, &mut buff).unwrap(), val);
    }

    fn to_value(value: impl serde::Serialize, event: Option<&str>) -> SocketIoValue {
        MsgPackParser::default()
            .encode_value(&value, event)
            .unwrap()
    }

    #[test]
    fn serialize_packet_event() {
        let packet = Packet {
            ns: "/test".into(),
            inner: PacketData::Event(to_value(vec![1, 2, 3]), Some(42)),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut reader = Cursor::new(serialized);

        assert_eq!(read_map_len(&mut reader).unwrap(), 4);
        assert_str(&mut reader, "type");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), index);
        assert_str(&mut reader, "nsp");
        assert_str(&mut reader, "/test");
        assert_str(&mut reader, "data");

        dbg!(&reader);
        // Check the event data structure
        assert_eq!(read_array_len(&mut reader).unwrap(), 4);
        assert_str(&mut reader, "test_event");
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 1);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 2);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 3);

        // Check the ID
        assert_str(&mut reader, "id");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 42);
    }

    #[test]
    fn serialize_packet_binary_event() {
        let packet = Packet {
            ns: "/binary".into(),
            inner: PacketData::BinaryEvent(
                "bin_event".into(),
                BinaryPacket {
                    data: to_value_event((1, 2, 3, &BIN, &BIN)),
                    bin: vec![],
                },
                Some(99),
            ),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut reader = Cursor::new(serialized);

        assert_eq!(read_map_len(&mut reader).unwrap(), 5);
        assert_str(&mut reader, "type");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), index);
        assert_str(&mut reader, "nsp");
        assert_str(&mut reader, "/binary");
        assert_str(&mut reader, "data");

        // Check the event data structure
        assert_eq!(read_array_len(&mut reader).unwrap(), 6);
        assert_str(&mut reader, "bin_event");
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 1);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 2);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 3);

        let mut buff = [0; 4];
        assert_eq!(read_bin_len(&mut reader).unwrap(), 4);
        reader.read_exact_buf(&mut buff).unwrap();
        assert_eq!(&buff, BIN.as_ref());

        assert_eq!(read_bin_len(&mut reader).unwrap(), 4);
        reader.read_exact_buf(&mut buff).unwrap();
        assert_eq!(&buff, BIN.as_ref());

        // Check the ID
        assert_str(&mut reader, "id");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 99);
    }

    #[test]
    fn serialize_packet_event_ack() {
        let packet = Packet {
            ns: "/ack".into(),
            inner: PacketData::EventAck(to_value_event(vec![4, 5, 6]), 100),
        };

        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut reader = Cursor::new(serialized);

        assert_eq!(read_map_len(&mut reader).unwrap(), 4);
        assert_str(&mut reader, "type");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), index);
        assert_str(&mut reader, "nsp");
        assert_str(&mut reader, "/ack");
        assert_str(&mut reader, "data");

        // Check the ack data structure
        let data_len = read_array_len(&mut reader).unwrap();
        assert_eq!(data_len, 3); // Acknowledgement wraps the data in an array
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 4);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 5);
        assert_eq!(read_int::<u8, _>(&mut reader).unwrap(), 6);

        // Check the ID
        assert_str(&mut reader, "id");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 100);
    }

    #[test]
    fn serialize_packet_connect_error() {
        let packet = Packet {
            ns: "/error".into(),
            inner: PacketData::ConnectError("Error message".into()),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut reader = Cursor::new(serialized);

        assert_eq!(read_map_len(&mut reader).unwrap(), 3);
        assert_str(&mut reader, "type");
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), index);
        assert_str(&mut reader, "nsp");
        assert_str(&mut reader, "/error");
        assert_str(&mut reader, "data");

        assert_eq!(read_map_len(&mut reader).unwrap(), 1);
        assert_str(&mut reader, "message");
        assert_str(&mut reader, "Error message");
    }

    #[test]
    fn serialize_event_data_with_array() {
        // Test when data is an array
        let mut data = Vec::new();
        write_array_len(&mut data, 2).unwrap();
        write_sint(&mut data, 42).unwrap();
        write_sint(&mut data, 43).unwrap();

        const EVENT: &'static str = "test_event";
        let mut buff = Vec::new();

        serialize_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(3)));

        let mut reader = Cursor::new(buff);

        assert_eq!(read_array_len(&mut reader).unwrap(), 3);
        assert_str(&mut reader, EVENT);
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 42);
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 43);
    }

    #[test]
    fn serialize_event_data_with_non_array() {
        // Test when data is a non-array (e.g., integer)
        let mut data = Vec::new();
        encode::write_sint(&mut data, 42).unwrap();

        const EVENT: &'static str = "test_event";
        let mut buff = Vec::new();

        serialize_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(2)));

        let mut reader = Cursor::new(buff);
        assert_eq!(read_array_len(&mut reader).unwrap(), 2);
        assert_str(&mut reader, EVENT);
        assert_eq!(read_int::<usize, _>(&mut reader).unwrap(), 42);
    }

    #[test]
    fn serialize_event_data_with_string() {
        // Test when data is a non-array (e.g., string)
        let mut data = Vec::new();
        encode::write_str(&mut data, "hello").unwrap();

        const EVENT: &'static str = "test_event";
        let mut buff = Vec::new();

        serialize_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(2)));

        let mut reader = Cursor::new(buff);
        assert_eq!(read_array_len(&mut reader).unwrap(), 2);

        assert_str(&mut reader, EVENT);
        assert_str(&mut reader, "hello");
    }

    #[test]
    fn serialize_ack_data_array() {
        // Test when buff already contains an array
        let mut data = Vec::new();
        let mut buff = Vec::new();
        write_array_len(&mut data, 2).unwrap();
        write_uint(&mut data, 42).unwrap();
        write_uint(&mut data, 43).unwrap();

        serialize_ack_data(&data, &mut buff);

        // Verify that the marker is still an array and nothing changed
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(2)));
    }

    #[test]
    fn serialize_ack_data_null() {
        // Test when buff contains a null value
        let mut buff = Vec::new();
        let mut data = Vec::new();
        write_nil(&mut data).unwrap();

        serialize_ack_data(&data, &mut buff);

        // Verify that the buff is now an empty array
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(0)));
    }

    #[test]
    fn serialize_ack_data_integer() {
        // Test when buff contains an integer (non-array, non-null data)
        let mut data = Vec::new();
        let mut buff = Vec::new();
        write_uint(&mut data, 42).unwrap();

        serialize_ack_data(&data, &mut buff);
        // Verify that the integer has been wrapped in an array
        let marker = Marker::from_u8(buff[0]);
        assert_eq!(marker, Marker::FixArray(1));
        assert_eq!(buff, [145, 42]);
    }

    #[test]
    fn serialize_ack_data_string() {
        // Test when buff contains a string (non-array, non-null data)
        let mut buff = Vec::new();
        let mut data = Vec::new();

        write_str(&mut data, "test").unwrap();

        serialize_ack_data(&data, &mut buff);
        dbg!(&data, &buff);

        // Verify that the string has been wrapped in an array
        let marker = Marker::from_u8(buff[0]);
        assert!(matches!(marker, Marker::FixArray(1)));
    }
}
