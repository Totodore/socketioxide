use bytes::BufMut;
use rmp::{decode::Bytes, Marker};

use crate::packet::{Packet, PacketData};

/// Manual packet serialization.
pub fn serialize_packet(packet: Packet<'_>) -> Vec<u8> {
    let mut buff = Vec::new(); // TODO: with_capacity

    let map_len = get_packet_map_len(&packet);
    rmp::encode::write_map_len(&mut buff, map_len).unwrap();
    rmp::encode::write_str(&mut buff, "type").unwrap();
    rmp::encode::write_uint(&mut buff, packet.inner.index() as u64).unwrap();
    rmp::encode::write_str(&mut buff, "nsp").unwrap();
    rmp::encode::write_str(&mut buff, &packet.ns).unwrap();

    match packet.inner {
        PacketData::Event(event, data, id) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            serialize_event_data(&event, &data.to_msgpack().unwrap().data, &mut buff);
            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::BinaryEvent(event, bin, id) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            let value = bin.data.to_msgpack().unwrap();
            serialize_event_data(&event, &value.data, &mut buff);

            serialize_attachments(&mut buff, value.attachments);
            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::EventAck(data, id) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            serialize_ack_data(&data.to_msgpack().unwrap().data, &mut buff);
            serialize_id(&mut buff, id);
        }
        PacketData::BinaryAck(bin, id) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            let value = bin.data.to_msgpack().unwrap();
            serialize_ack_data(&value.data, &mut buff);
            serialize_attachments(&mut buff, value.attachments);
            serialize_id(&mut buff, id);
        }
        PacketData::Connect(Some(data)) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            buff.put_slice(&data.to_msgpack().unwrap().data)
        }
        PacketData::ConnectError(data) => {
            rmp::encode::write_str(&mut buff, "data").unwrap();
            rmp::encode::write_map_len(&mut buff, 1).unwrap();
            rmp::encode::write_str(&mut buff, "message").unwrap();
            rmp::encode::write_str(&mut buff, &data).unwrap();
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
fn serialize_event_data(event: &str, data: &[u8], buff: &mut Vec<u8>) {
    let marker = rmp::decode::read_marker(&mut Bytes::new(data)).unwrap();
    match marker {
        Marker::FixArray(_) | Marker::Array16 | Marker::Array32 => {
            let mut bytes = Bytes::new(data);
            let arr_len = rmp::decode::read_array_len(&mut bytes).unwrap();
            rmp::encode::write_array_len(buff, arr_len + 1).unwrap();
            rmp::encode::write_str(buff, event).unwrap();
            buff.put_slice(bytes.remaining_slice());
        }
        _ => {
            rmp::encode::write_array_len(buff, 2).unwrap();
            rmp::encode::write_str(buff, event).unwrap();
            buff.put_slice(data);
        }
    }
}

fn serialize_id(buff: &mut Vec<u8>, id: i64) {
    rmp::encode::write_str(buff, "id").unwrap();
    rmp::encode::write_sint(buff, id).unwrap();
}
fn serialize_attachments(buff: &mut Vec<u8>, len: usize) {
    rmp::encode::write_str(buff, "attachments").unwrap();
    rmp::encode::write_uint(buff, len as u64).unwrap();
}
/// Serialize ack data in place to the following form: `[data]`
fn serialize_ack_data(data: &[u8], buff: &mut Vec<u8>) {
    let mut bytes = Bytes::new(data);
    let marker = rmp::decode::read_marker(&mut bytes).unwrap();
    match marker {
        Marker::FixArray(_) | Marker::Array16 | Marker::Array32 => {
            buff.put_slice(data);
        }
        Marker::Null => {
            rmp::encode::write_array_len(buff, 0).unwrap();
        }
        _ => {
            rmp::encode::write_array_len(buff, 1).unwrap();
            buff.put_slice(data);
        }
    };
}

fn get_packet_map_len(packet: &Packet<'_>) -> u32 {
    2 + match packet.inner {
        PacketData::Connect(Some(_)) => 1,
        PacketData::Connect(None) => 0,
        PacketData::Disconnect => 0,
        PacketData::Event(_, _, Some(_)) => 2,
        PacketData::Event(_, _, None) => 1,
        PacketData::EventAck(_, _) => 2,
        PacketData::ConnectError(_) => 1,
        PacketData::BinaryEvent(_, _, Some(_)) => 3,
        PacketData::BinaryEvent(_, _, None) => 2,
        PacketData::BinaryAck(_, _) => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::BinaryPacket;
    use crate::parser::MsgPackParser;
    use crate::parser::Parse;
    use crate::Value;
    use rmp::decode::*;
    use rmp::encode::*;
    use std::u8;

    const BIN: ::bytes::Bytes = ::bytes::Bytes::from_static(&[1, 2, 3, 4]);

    fn assert_str(bytes: &mut Bytes<'_>, val: &str) {
        let mut buff = [0; 256];
        assert_eq!(read_str(bytes, &mut buff).unwrap(), val);
    }
    fn to_msgpack(value: impl serde::Serialize) -> Value {
        MsgPackParser::default().to_value(value).unwrap()
    }

    #[test]
    fn serialize_packet_event() {
        let packet = Packet {
            ns: "/test".into(),
            inner: PacketData::Event("test_event".into(), to_msgpack(vec![1, 2, 3]), Some(42)),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut bytes = Bytes::new(&serialized);

        assert_eq!(read_map_len(&mut bytes).unwrap(), 4);
        assert_str(&mut bytes, "type");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), index);
        assert_str(&mut bytes, "nsp");
        assert_str(&mut bytes, "/test");
        assert_str(&mut bytes, "data");

        dbg!(&bytes);
        // Check the event data structure
        assert_eq!(read_array_len(&mut bytes).unwrap(), 4);
        assert_str(&mut bytes, "test_event");
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 1);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 2);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 3);

        // Check the ID
        assert_str(&mut bytes, "id");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 42);
    }

    #[test]
    fn serialize_packet_binary_event() {
        let packet = Packet {
            ns: "/binary".into(),
            inner: PacketData::BinaryEvent(
                "bin_event".into(),
                BinaryPacket {
                    data: to_msgpack((1, 2, 3, &BIN, &BIN)),
                    bin: vec![],
                },
                Some(99),
            ),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut bytes = Bytes::new(&serialized);

        assert_eq!(read_map_len(&mut bytes).unwrap(), 5);
        assert_str(&mut bytes, "type");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), index);
        assert_str(&mut bytes, "nsp");
        assert_str(&mut bytes, "/binary");
        assert_str(&mut bytes, "data");

        // Check the event data structure
        assert_eq!(read_array_len(&mut bytes).unwrap(), 6);
        assert_str(&mut bytes, "bin_event");
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 1);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 2);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 3);

        let mut buff = [0; 4];
        assert_eq!(read_bin_len(&mut bytes).unwrap(), 4);
        bytes.read_exact_buf(&mut buff).unwrap();
        assert_eq!(&buff, BIN.as_ref());

        assert_eq!(read_bin_len(&mut bytes).unwrap(), 4);
        bytes.read_exact_buf(&mut buff).unwrap();
        assert_eq!(&buff, BIN.as_ref());

        // Check the attachments
        assert_str(&mut bytes, "attachments");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 2);

        // Check the ID
        assert_str(&mut bytes, "id");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 99);
    }

    #[test]
    fn serialize_packet_event_ack() {
        let packet = Packet {
            ns: "/ack".into(),
            inner: PacketData::EventAck(to_msgpack(vec![4, 5, 6]), 100),
        };

        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut bytes = Bytes::new(&serialized);

        assert_eq!(read_map_len(&mut bytes).unwrap(), 4);
        assert_str(&mut bytes, "type");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), index);
        assert_str(&mut bytes, "nsp");
        assert_str(&mut bytes, "/ack");
        assert_str(&mut bytes, "data");

        // Check the ack data structure
        let data_len = read_array_len(&mut bytes).unwrap();
        assert_eq!(data_len, 3); // Acknowledgement wraps the data in an array
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 4);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 5);
        assert_eq!(read_int::<u8, _>(&mut bytes).unwrap(), 6);

        // Check the ID
        assert_str(&mut bytes, "id");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 100);
    }

    #[test]
    fn serialize_packet_connect_error() {
        let packet = Packet {
            ns: "/error".into(),
            inner: PacketData::ConnectError("Error message".into()),
        };
        let index = packet.inner.index();
        let serialized = serialize_packet(packet);
        let mut bytes = Bytes::new(&serialized);

        assert_eq!(read_map_len(&mut bytes).unwrap(), 3);
        assert_str(&mut bytes, "type");
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), index);
        assert_str(&mut bytes, "nsp");
        assert_str(&mut bytes, "/error");
        assert_str(&mut bytes, "data");

        assert_eq!(read_map_len(&mut bytes).unwrap(), 1);
        assert_str(&mut bytes, "message");
        assert_str(&mut bytes, "Error message");
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

        serialize_event_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
        assert!(matches!(marker, Marker::FixArray(3)));

        let mut bytes = Bytes::new(&buff);

        assert_eq!(read_array_len(&mut bytes).unwrap(), 3);
        assert_str(&mut bytes, EVENT);
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 42);
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 43);
    }

    #[test]
    fn serialize_event_data_with_non_array() {
        // Test when data is a non-array (e.g., integer)
        let mut data = Vec::new();
        rmp::encode::write_sint(&mut data, 42).unwrap();

        const EVENT: &'static str = "test_event";
        let mut buff = Vec::new();

        serialize_event_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
        assert!(matches!(marker, Marker::FixArray(2)));

        let mut bytes = Bytes::new(&buff);
        assert_eq!(read_array_len(&mut bytes).unwrap(), 2);
        assert_str(&mut bytes, EVENT);
        assert_eq!(read_int::<usize, _>(&mut bytes).unwrap(), 42);
    }

    #[test]
    fn serialize_event_data_with_string() {
        // Test when data is a non-array (e.g., string)
        let mut data = Vec::new();
        rmp::encode::write_str(&mut data, "hello").unwrap();

        const EVENT: &'static str = "test_event";
        let mut buff = Vec::new();

        serialize_event_data(EVENT, &data, &mut buff);

        // Verify that the buffer contains the expected serialized data
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
        assert!(matches!(marker, Marker::FixArray(2)));

        let mut bytes = Bytes::new(&buff);
        assert_eq!(read_array_len(&mut bytes).unwrap(), 2);

        assert_str(&mut bytes, EVENT);
        assert_str(&mut bytes, "hello");
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
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
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
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
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
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
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
        let marker = read_marker(&mut Bytes::new(&buff)).unwrap();
        assert!(matches!(marker, Marker::FixArray(1)));
    }
}