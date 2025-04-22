use bytes::BufMut;
use rmp::encode;
use socketioxide_core::{
    Value,
    packet::{Packet, PacketData},
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
        PacketData::Event(Value::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::BinaryEvent(Value::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);

            if let Some(id) = id {
                serialize_id(&mut buff, id);
            }
        }
        PacketData::EventAck(Value::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            serialize_id(&mut buff, id);
        }
        PacketData::BinaryAck(Value::Bytes(data), id) => {
            encode::write_str(&mut buff, "data").unwrap();
            serialize_data(&data, &mut buff);
            serialize_id(&mut buff, id);
        }
        PacketData::Connect(Some(Value::Bytes(data))) => {
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
        PacketData::Event(_, Some(_)) | PacketData::BinaryEvent(_, Some(_)) => 2,
        PacketData::Event(_, None) | PacketData::BinaryEvent(_, None) => 1,
        PacketData::EventAck(_, _) | PacketData::BinaryAck(_, _) => 2,
        PacketData::ConnectError(_) => 1,
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use ::bytes::Bytes;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    const BIN: Bytes = Bytes::from_static(&[1, 2, 3, 4]);
    #[derive(Serialize, Deserialize)]
    struct StubPacket<T: serde::Serialize> {
        r#type: usize,
        nsp: &'static str,
        data: T,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<i64>,
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
            id,
            data,
        })
        .unwrap()
    }

    fn to_value(value: impl serde::Serialize) -> Value {
        Value::Bytes(rmp_serde::to_vec_named(&value).unwrap().into())
    }

    #[test]
    fn serialize_packet_event() {
        let data = serialize_packet(Packet {
            ns: "/test".into(),
            inner: PacketData::Event(to_value(("test_event", vec![1, 2, 3])), Some(42)),
        });
        assert_eq!(
            data,
            packet(2, "/test", ("test_event", vec![1, 2, 3]), Some(42))
        );
    }

    #[test]
    fn serialize_packet_binary_event() {
        let data = serialize_packet(Packet {
            ns: "/binary".into(),
            inner: PacketData::BinaryEvent(to_value(("bin_event", 1, 2, 3, BIN, BIN)), Some(99)),
        });
        assert_eq!(
            data,
            packet(5, "/binary", ("bin_event", 1, 2, 3, BIN, BIN), Some(99))
        );
    }

    #[test]
    fn serialize_packet_event_ack() {
        let data = serialize_packet(Packet {
            ns: "/ack".into(),
            inner: PacketData::EventAck(to_value([vec![4, 5, 6]]), 100),
        });
        assert_eq!(data, packet(3, "/ack", (vec![4, 5, 6],), Some(100)));
    }

    #[test]
    fn serialize_packet_connect_error() {
        let data = serialize_packet(Packet {
            ns: "/error".into(),
            inner: PacketData::ConnectError("Error message".into()),
        });
        assert_eq!(
            data,
            packet(4, "/error", json!({ "message": "Error message" }), None)
        );
    }

    #[test]
    fn serialize_packet_ack() {
        let data = serialize_packet(Packet {
            ns: "/ack".into(),
            inner: PacketData::EventAck(to_value((1, 2, "test")), 132),
        });
        assert_eq!(data, packet(3, "/ack", (1, 2, "test"), Some(132)));
    }

    #[test]
    fn serialize_packet_ack_binary() {
        let data = serialize_packet(Packet {
            ns: "/ack".into(),
            inner: PacketData::BinaryAck(to_value((1, 2, "test", BIN)), 132),
        });
        assert_eq!(data, packet(6, "/ack", (1, 2, "test", BIN), Some(132)));
    }
}
