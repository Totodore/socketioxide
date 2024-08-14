use super::{value::ParseError, Error, Parse, TransportPayload, Value};
use crate::packet::{BinaryPacket, Packet, PacketData};
use bytes::Bytes;
use rmpv::Value as MsgPackValue;
use serde::{Deserialize, Serialize};

/// The MsgPack parser
#[derive(Default, Clone)]
pub struct MsgPackParser;

#[derive(Debug, Serialize, Deserialize)]
enum MsgPackPacketData {
    Value(Value),
    ConnectError(MsgPackConnectErrorMessage),
}

#[derive(Debug, Serialize, Deserialize)]
struct MsgPackConnectErrorMessage {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MsgPackPacket {
    r#type: usize,
    nsp: engineioxide::Str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<MsgPackValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments: Option<usize>,
}

impl Parse for MsgPackParser {
    fn encode<'a>(&self, packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>) {
        use PacketData::*;
        let index = packet.inner.index();
        let id = match packet.inner {
            Event(_, _, id) | BinaryEvent(_, _, id) => id,
            EventAck(_, id) | BinaryAck(_, id) => Some(id),
            _ => None,
        };
        let attachments = match packet.inner {
            BinaryAck(BinaryPacket { payload_count, .. }, _)
            | BinaryEvent(_, BinaryPacket { payload_count, .. }, _) => Some(payload_count),
            _ => None,
        };
        let data = packet_to_value(packet.inner);
        let payload = MsgPackPacket {
            r#type: index,
            nsp: packet.ns,
            data,
            id,
            attachments,
        };

        let data = rmp_serde::encode::to_vec_named(&payload).unwrap();
        (TransportPayload::Bytes(data.into()), Vec::new())
    }

    fn decode_str(&self, data: engineioxide::Str) -> Result<Packet<'static>, Error> {
        Err(Error::UnexpectedStringPacket)
    }

    fn decode_bin(&self, bin: bytes::Bytes) -> Result<Packet<'static>, Error> {
        Err(Error::UnexpectedStringPacket)
    }

    fn to_value<T: Serialize>(&self, data: T) -> Result<Value, ParseError> {
        Ok(Value::MsgPack(rmpv::ext::to_value(data)?))
    }
}

fn packet_to_value<'a>(packet: PacketData<'a>) -> Option<MsgPackValue> {
    use PacketData::*;
    match packet {
        Connect(Some(Value::MsgPack(data))) => Some(data),
        Connect(_) => None,
        ConnectError(message) => {
            Some(rmpv::ext::to_value(MsgPackConnectErrorMessage { message }).unwrap())
        }
        Disconnect => None,
        Event(e, data, _) | BinaryEvent(e, BinaryPacket { data, .. }, _) => {
            // Expand the packet if it is an array with data -> ["event", ...data]
            let data = match data {
                Value::MsgPack(MsgPackValue::Array(mut arr)) => {
                    arr.insert(0, MsgPackValue::String(e.to_string().into()));
                    MsgPackValue::Array(arr)
                }
                Value::MsgPack(data) => {
                    MsgPackValue::Array(vec![MsgPackValue::String(e.to_string().into()), data])
                }
                _ => panic!("unsuported value type"),
            };
            Some(data)
        }
        EventAck(data, _) | BinaryAck(BinaryPacket { data, .. }, _) => {
            // Enforce that the packet is an array -> [data]
            let data = match data {
                Value::MsgPack(MsgPackValue::Array(data)) => MsgPackValue::Array(data),
                Value::MsgPack(MsgPackValue::Nil) => MsgPackValue::Array(vec![]),
                Value::MsgPack(data) => MsgPackValue::Array(vec![data]),
                _ => panic!("unsuported value type"),
            };
            Some(data)
        }
    }
}

/// All the static binary data is generated from this script, using the official socket.io implementation:
/// https://gist.github.com/Totodore/943fac5107325589bfbfb50f55925698
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use bytes::BufMut;
    use engineioxide::sid::Sid;
    use serde_json::json;

    use crate::ProtocolVersion;

    use super::*;

    fn decode(value: Bytes) -> Packet<'static> {
        MsgPackParser::default().decode_bin(value.into()).unwrap()
    }
    fn encode(packet: Packet<'_>) -> Bytes {
        match MsgPackParser::default().encode(packet).0 {
            TransportPayload::Bytes(b) => b,
            TransportPayload::Str(_) => panic!("implementation should only return bytes"),
        }
    }
    #[test]
    fn packet_encode_connect_root_ns() {
        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];
        let packet = encode(Packet::connect("/", sid, ProtocolVersion::V5));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_connect_custom_ns() {
        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 129, 163, 115, 105, 100, 176, 110, 119, 122, 51,
            67, 56, 117, 55, 113, 121, 115, 118, 103, 86, 113, 106,
        ];
        let packet = encode(Packet::connect("/admin™", sid, ProtocolVersion::V5));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_disconnect_root_ns() {
        const DATA: [u8; 13] = [130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 161, 47];
        let packet = encode(Packet::disconnect("/"));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_disconnect_custom_ns() {
        const DATA: [u8; 21] = [
            130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162,
        ];
        let packet = encode(Packet::disconnect("/admin™"));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_event_root_ns() {
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let packet = encode(Packet::event("/", "event", json!({ "data": "value™" })));
        assert_eq!(&DATA, packet.as_ref());
    }
    #[test]
    fn packet_encode_event_root_ns_empty_data() {
        const DATA: [u8; 26] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 144,
        ];
        let packet = encode(Packet::event("/", "event", json!([])));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_event_root_ns_with_ack_id() {
        const DATA: [u8; 44] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let mut packet = Packet::event("/", "event", json!({ "data": "value™" }));
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }
    #[test]
    fn packet_encode_event_custom_ns() {
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let packet = encode(Packet::event("/admin™", "event", json!({"data": "value™"})));

        assert_eq!(&DATA, packet.as_ref());
    }
    #[test]
    fn packet_encode_event_custom_ns_with_ack_id() {
        const DATA: [u8; 52] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let mut packet = Packet::event("/admin™", "event", json!([{"data": "value™"}]));
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_event_ack_root_ns() {
        const DATA: [u8; 28] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let packet = encode(Packet::ack("/", json!("data"), 54));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_event_ack_custom_ns() {
        const DATA: [u8; 36] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let packet = encode(Packet::ack("/admin™", json!("data"), 54));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_binary_event_root_ns() {
        const DATA: [u8; 59] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1,
        ];
        let packet = encode(Packet::bin_event(
            "/",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        ));
        assert_eq!(&DATA, packet.as_ref());
    }
    #[test]
    fn packet_encode_binary_event_root_ns_with_ack_id() {
        const DATA: [u8; 64] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let mut packet = Packet::bin_event(
            "/",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);

        assert_eq!(&DATA, packet.as_ref());
    }
    #[test]
    fn packet_encode_binary_event_custom_ns() {
        const DATA: [u8; 67] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1,
        ];
        let packet = encode(Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![Bytes::from_static(&[1])],
        ));

        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_binary_event_custom_ns_with_ack_id() {
        const DATA: [u8; 72] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let mut packet = Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![Bytes::from_static(&[1])],
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_binary_ack_root_ns() {
        const DATA: [u8; 63] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1, 162, 105, 100, 54,
        ];
        let packet = encode(Packet::bin_ack(
            "/",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
            54,
        ));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_encode_binary_ack_custom_ns() {
        const DATA: [u8; 71] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 54,
        ];
        let packet = encode(Packet::bin_ack(
            "/admin™",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
            54,
        ));

        assert_eq!(&DATA, packet.as_ref());
    }
}
