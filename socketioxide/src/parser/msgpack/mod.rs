use super::{
    value::{MsgPackValue, ParseError},
    Error, Parse, TransportPayload, Value,
};
use crate::packet::Packet;
use attachments::get_attachments;
use bytes::Bytes;
use de::deserialize_packet;
use ser::serialize_packet;
use serde::{Deserialize, Serialize};

mod attachments;
mod de;
mod ser;

/// The MsgPack parser
#[derive(Default, Debug, Clone)]
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

impl Parse for MsgPackParser {
    fn encode<'a>(&self, packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>) {
        let data = serialize_packet(packet);
        (TransportPayload::Bytes(data.into()), Vec::new())
    }

    fn decode_str(&self, data: engineioxide::Str) -> Result<Packet<'static>, Error> {
        Err(Error::UnexpectedStringPacket)
    }

    fn decode_bin(&self, bin: bytes::Bytes) -> Result<Packet<'static>, Error> {
        Ok(deserialize_packet(bin)?)
    }

    fn to_value<T: Serialize>(&self, data: T) -> Result<Value, ParseError> {
        let attachments = get_attachments(&data); // TODO: only get attachments for bin packet
        let data = rmp_serde::to_vec_named(&data)?;
        Ok(Value::MsgPack(MsgPackValue {
            data: data.into(),
            attachments,
        }))
    }
}

/// All the static binary data is generated from this script, using the official socket.io implementation:
/// https://gist.github.com/Totodore/943fac5107325589bfbfb50f55925698
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use engineioxide::sid::Sid;
    use serde_json::json;

    use crate::{packet::ConnectPacket, ProtocolVersion};

    use super::*;

    fn decode(value: &'static [u8]) -> Packet<'static> {
        MsgPackParser::default()
            .decode_bin(Bytes::from_static(value))
            .unwrap()
    }
    fn encode(packet: Packet<'_>) -> Bytes {
        match MsgPackParser::default().encode(packet).0 {
            TransportPayload::Bytes(b) => b,
            TransportPayload::Str(_) => panic!("implementation should only return bytes"),
        }
    }
    fn to_msgpack(value: impl serde::Serialize) -> Value {
        MsgPackParser::default().to_value(value).unwrap()
    }

    #[test]
    fn packet_encode_connect_root_ns() {
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = encode(Packet::connect("/", sid, ProtocolVersion::V5));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_connect_root_ns() {
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];
        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::connect("/", sid, ProtocolVersion::V5));
    }

    #[test]
    fn packet_encode_connect_custom_ns() {
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 129, 163, 115, 105, 100, 176, 110, 119, 122, 51,
            67, 56, 117, 55, 113, 121, 115, 118, 103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = encode(Packet::connect("/admin™", sid, ProtocolVersion::V5));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_connect_custom_ns() {
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 129, 163, 115, 105, 100, 176, 110, 119, 122, 51,
            67, 56, 117, 55, 113, 121, 115, 118, 103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::connect("/admin™", sid, ProtocolVersion::V5));
    }

    #[test]
    fn packet_encode_disconnect_root_ns() {
        const DATA: [u8; 13] = [130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 161, 47];
        let packet = encode(Packet::disconnect("/"));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_disconnect_root_ns() {
        const DATA: [u8; 13] = [130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 161, 47];
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::disconnect("/"));
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
    fn packet_decode_disconnect_custom_ns() {
        const DATA: [u8; 21] = [
            130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162,
        ];
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::disconnect("/admin™"));
    }

    #[test]
    fn packet_encode_event_root_ns() {
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let data = to_msgpack(json!({ "data": "value™" }));
        let packet = encode(Packet::event("/", "event", data));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_root_ns() {
        const DATA: [u8; 40] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let data = to_msgpack(json!([{ "data": "value™" }]));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::event("/", "event", data));
    }
    #[test]
    fn packet_encode_event_root_ns_empty_data() {
        const DATA: [u8; 25] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 165, 101, 118, 101, 110, 116,
        ];
        let data = to_msgpack(json!([]));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::event("/", "event", data));
    }

    #[test]
    fn packet_encode_event_root_ns_with_ack_id() {
        const DATA: [u8; 44] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!({ "data": "value™" }));
        let mut packet = Packet::event("/", "event", data);
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_root_ns_with_ack_id() {
        const DATA: [u8; 44] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{ "data": "value™" }]));
        let mut packet_comp = Packet::event("/", "event", data);
        packet_comp.inner.set_ack_id(1);
        let packet = decode(&DATA);
        assert_eq!(packet, packet_comp);
    }
    #[test]
    fn packet_encode_event_custom_ns() {
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let data = to_msgpack(json!({"data": "value™"}));
        let packet = encode(Packet::event("/admin™", "event", data));

        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_custom_ns() {
        const DATA: [u8; 48] = [
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let packet = decode(&DATA);

        assert_eq!(packet, Packet::event("/admin™", "event", data));
    }
    #[test]
    fn packet_encode_event_custom_ns_with_ack_id() {
        const DATA: [u8; 52] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let mut packet = Packet::event("/admin™", "event", data);
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_custom_ns_with_ack_id() {
        const DATA: [u8; 52] = [
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let mut packet_ref = Packet::event("/admin™", "event", data);
        packet_ref.inner.set_ack_id(1);
        let packet = decode(&DATA);
        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_event_ack_root_ns() {
        const DATA: [u8; 28] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let data = to_msgpack(json!("data"));
        let packet = encode(Packet::ack("/", data, 54));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_root_ns() {
        const DATA: [u8; 28] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let data = to_msgpack(json!(["data"]));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::ack("/", data, 54));
    }

    #[test]
    fn packet_encode_event_ack_custom_ns() {
        const DATA: [u8; 36] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_msgpack(json!(["data"]));
        let packet = encode(Packet::ack("/admin™", data, 54));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_custom_ns() {
        const DATA: [u8; 36] = [
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_msgpack(json!(["data"]));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::ack("/admin™", data, 54));
    }

    #[test]
    fn packet_encode_binary_event_root_ns() {
        const DATA: [u8; 59] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::bin_event("/", "event", data, vec![]));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_root_ns() {
        const DATA: [u8; 59] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::bin_event("/", "event", data, vec![]));
    }

    #[test]
    fn packet_encode_binary_event_root_ns_with_ack_id() {
        const DATA: [u8; 64] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let mut packet = Packet::bin_event("/", "event", data, vec![]);
        packet.inner.set_ack_id(254);
        let packet = encode(packet);

        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_root_ns_with_ack_id() {
        const DATA: [u8; 64] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110,
            116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let mut packet_ref = Packet::bin_event("/", "event", data, vec![]);
        packet_ref.inner.set_ack_id(254);
        let packet = decode(&DATA);

        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_binary_event_custom_ns() {
        const DATA: [u8; 67] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let packet = encode(Packet::bin_event("/admin™", "event", data, vec![]));

        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_custom_ns() {
        const DATA: [u8; 67] = [
            132, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let packet = decode(&DATA);

        assert_eq!(packet, Packet::bin_event("/admin™", "event", data, vec![]));
    }

    #[test]
    fn packet_encode_binary_event_custom_ns_with_ack_id() {
        const DATA: [u8; 72] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let mut packet = Packet::bin_event("/admin™", "event", data, vec![]);
        packet.inner.set_ack_id(254);
        let packet = encode(packet);
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_custom_ns_with_ack_id() {
        const DATA: [u8; 72] = [
            133, 164, 116, 121, 112, 101, 5, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97,
            116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let mut packet_ref = Packet::bin_event("/admin™", "event", data, vec![]);
        packet_ref.inner.set_ack_id(254);
        let packet = decode(&DATA);
        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_binary_ack_root_ns() {
        const DATA: [u8; 57] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1,
            2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::bin_ack("/", data, vec![], 54));
        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_ack_root_ns() {
        const DATA: [u8; 57] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1,
            2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109, 101, 110, 116, 115, 1, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(&DATA);
        assert_eq!(packet, Packet::bin_ack("/", data, vec![], 54));
    }

    #[test]
    fn packet_encode_binary_ack_custom_ns() {
        const DATA: [u8; 65] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 129, 164, 100, 97, 116, 97, 168, 118, 97,
            108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109,
            101, 110, 116, 115, 1, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::bin_ack("/admin™", data, vec![], 54));

        assert_eq!(&DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_ack_custom_ns() {
        const DATA: [u8; 65] = [
            133, 164, 116, 121, 112, 101, 6, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 129, 164, 100, 97, 116, 97, 168, 118, 97,
            108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 171, 97, 116, 116, 97, 99, 104, 109,
            101, 110, 116, 115, 1, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(&DATA);

        assert_eq!(packet, Packet::bin_ack("/admin™", data, vec![], 54));
    }
}
