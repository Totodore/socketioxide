use super::{Error, Parse, TransportPayload};
use crate::packet::{BinaryPacket, Packet, PacketData};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};

#[derive(Default, Clone)]
pub struct MsgPackParser;

#[derive(Debug, Serialize, Deserialize)]
struct MsgPackPacket {
    r#type: usize,
    nsp: engineioxide::Str,
    data: MsgPackData,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum MsgPackData {
    Connect(serde_json::Value),
    ConnectError { message: String },
    Event(serde_json::Value),
    EventAck(serde_json::Value),
    BinaryEvent(serde_json::Value),
    BinaryAck(serde_json::Value),
    Disconnect,
}

impl Parse for MsgPackParser {
    fn serialize<'a>(&self, packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>) {
        let index = packet.inner.index();
        let data = packet.inner.into();
        let payload = MsgPackPacket {
            r#type: index,
            nsp: packet.ns,
            data,
        };

        let data = rmp_serde::encode::to_vec_named(&payload).unwrap();
        (TransportPayload::Bytes(data.into()), Vec::new())
    }

    fn parse_str(&self, data: engineioxide::Str) -> Result<Packet<'static>, Error> {
        Err(Error::UnexpectedStringPacket)
    }

    fn parse_bin(&self, bin: bytes::Bytes) -> Result<Packet<'static>, Error> {
        Err(Error::UnexpectedStringPacket)
    }
}

impl<'a> From<PacketData<'a>> for MsgPackData {
    fn from(value: PacketData<'a>) -> Self {
        match value {
            PacketData::Connect(data) => MsgPackData::Connect(data.unwrap()),
            PacketData::ConnectError(message) => MsgPackData::ConnectError { message },
            PacketData::Disconnect => MsgPackData::Disconnect,
            PacketData::Event(_, data, _) => MsgPackData::Event(data),
            PacketData::EventAck(data, _) => MsgPackData::EventAck(data),
            PacketData::BinaryEvent(_, BinaryPacket { data, .. }, _) => {
                MsgPackData::BinaryEvent(data)
            }
            PacketData::BinaryAck(BinaryPacket { data, .. }, _) => MsgPackData::BinaryAck(data),
        }
    }
}
impl<'a> From<MsgPackData> for PacketData<'a> {
    fn from(value: MsgPackData) -> Self {
        match value {
            MsgPackData::Connect(data) => PacketData::Connect(Some(data)),
            MsgPackData::ConnectError { message } => PacketData::ConnectError(message),
            MsgPackData::Event(data) => PacketData::Event("".into(), data, None),
            MsgPackData::EventAck(data) => PacketData::EventAck(data, 0),
            MsgPackData::BinaryEvent(data) => unimplemented!(),
            MsgPackData::BinaryAck(_) => todo!(),
            MsgPackData::Disconnect => todo!(),
        }
    }
}

/// All the static binary data is generated from this script, using the official socket.io implementation:
/// https://gist.github.com/Totodore/943fac5107325589bfbfb50f55925698
mod tests {
    use std::str::FromStr;

    use bytes::BufMut;
    use engineioxide::sid::Sid;
    use serde_json::json;

    use crate::ProtocolVersion;

    use super::*;

    fn decode(value: Bytes) -> Packet<'static> {
        MsgPackParser::default().parse_bin(value.into()).unwrap()
    }
    fn encode(packet: Packet<'_>) -> Bytes {
        match MsgPackParser::default().serialize(packet).0 {
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
