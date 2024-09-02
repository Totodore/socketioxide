use std::fmt;

use bytes::Bytes;
use de::deserialize_packet;
use ser::serialize_packet;
use socketioxide_core::{
    packet::Packet,
    parser::{Parse, ParseError},
    SocketIoValue, Str,
};

mod de;
mod ser;
mod value;

/// The MsgPack parser
#[derive(Default, Debug, Clone)]
pub struct MsgPackParser;

#[derive(Debug)]
enum Error {
    Encode(rmp_serde::encode::Error),
    Decode(rmp_serde::decode::Error),
}
impl From<rmp_serde::encode::Error> for Error {
    fn from(e: rmp_serde::encode::Error) -> Self {
        Error::Encode(e)
    }
}
impl From<rmp_serde::decode::Error> for Error {
    fn from(e: rmp_serde::decode::Error) -> Self {
        Error::Decode(e)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Encode(e) => write!(f, "encode error: {}", e),
            Error::Decode(e) => write!(f, "decode error: {}", e),
        }
    }
}
impl std::error::Error for Error {}

impl Parse for MsgPackParser {
    type Error = Error;

    fn encode(&self, packet: Packet) -> socketioxide_core::SocketIoValue {
        let data = serialize_packet(packet);
        SocketIoValue::Bytes(data.into())
    }

    fn decode_str(&self, _data: Str) -> Result<Packet, ParseError<Self::Error>> {
        Err(ParseError::UnexpectedStringPacket)
    }

    fn decode_bin(&self, bin: Bytes) -> Result<Packet, ParseError<Self::Error>> {
        Ok(deserialize_packet(bin)?)
    }

    fn encode_value<T: serde::Serialize>(
        &self,
        data: &T,
        event: Option<&str>,
    ) -> Result<SocketIoValue, Self::Error> {
        value::to_value(data, event).map_err(Error::Encode)
    }

    fn decode_value<T: serde::de::DeserializeOwned>(
        &self,
        value: socketioxide_core::SocketIoValue,
        with_event: bool,
    ) -> Result<T, Self::Error> {
        value::from_value(value, with_event).map_err(Error::Decode)
    }
}

/// All the static binary data is generated from this script, using the official socket.io implementation:
/// https://gist.github.com/Totodore/943fac5107325589bfbfb50f55925698
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde_json::json;
    use socketioxide_core::{packet::ConnectPacket, Sid};

    use super::*;

    fn decode(value: &'static [u8]) -> Packet {
        MsgPackParser::default()
            .decode_bin(Bytes::from_static(value))
            .unwrap()
    }
    fn encode(packet: Packet) -> Bytes {
        match MsgPackParser::default().encode(packet) {
            SocketIoValue::Bytes(b) => b,
            SocketIoValue::Str(_) => panic!("implementation should only return bytes"),
        }
    }

    fn to_msgpack(value: impl serde::Serialize) -> SocketIoValue {
        MsgPackParser::default().encode_value(&value, None).unwrap()
    }

    #[test]
    fn packet_encode_connect_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = encode(Packet::connect("/", Some(sid)));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_connect_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];
        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = decode(DATA);
        assert_eq!(packet, Packet::connect("/", Some(sid)));
    }

    #[test]
    fn packet_encode_connect_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 129, 163, 115, 105, 100, 176, 110, 119, 122, 51,
            67, 56, 117, 55, 113, 121, 115, 118, 103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = encode(Packet::connect("/admin™", Some(sid)));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_connect_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 129, 163, 115, 105, 100, 176, 110, 119, 122, 51,
            67, 56, 117, 55, 113, 121, 115, 118, 103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_msgpack(ConnectPacket { sid });
        let packet = decode(DATA);
        assert_eq!(packet, Packet::connect("/admin™", Some(sid)));
    }

    #[test]
    fn packet_encode_disconnect_root_ns() {
        const DATA: &'static [u8] = &[130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 161, 47];
        let packet = encode(Packet::disconnect("/"));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_disconnect_root_ns() {
        const DATA: &'static [u8] = &[130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 161, 47];
        let packet = decode(DATA);
        assert_eq!(packet, Packet::disconnect("/"));
    }

    #[test]
    fn packet_encode_disconnect_custom_ns() {
        const DATA: &'static [u8] = &[
            130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162,
        ];
        let packet = encode(Packet::disconnect("/admin™"));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_disconnect_custom_ns() {
        const DATA: &'static [u8] = &[
            130, 164, 116, 121, 112, 101, 1, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162,
        ];
        let packet = decode(DATA);
        assert_eq!(packet, Packet::disconnect("/admin™"));
    }

    #[test]
    fn packet_encode_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let data = to_msgpack(json!({ "data": "value™" }));
        let packet = encode(Packet::event("/", "event", data));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let data = to_msgpack(json!([{ "data": "value™" }]));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", "event", data));
    }
    #[test]
    fn packet_encode_event_root_ns_empty_data() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 165, 101, 118, 101, 110, 116,
        ];
        let data = to_msgpack(json!([]));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", "event", data));
    }

    #[test]
    fn packet_encode_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!({ "data": "value™" }));
        let mut packet = Packet::event("/", "event", data);
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{ "data": "value™" }]));
        let mut packet_comp = Packet::event("/", "event", data);
        packet_comp.inner.set_ack_id(1);
        let packet = decode(DATA);
        assert_eq!(packet, packet_comp);
    }
    #[test]
    fn packet_encode_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let data = to_msgpack(json!({"data": "value™"}));
        let packet = encode(Packet::event("/admin™", "event", data));

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let packet = decode(DATA);

        assert_eq!(packet, Packet::event("/admin™", "event", data));
    }
    #[test]
    fn packet_encode_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let mut packet = Packet::event("/admin™", "event", data);
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_msgpack(json!([{"data": "value™"}]));
        let mut packet_ref = Packet::event("/admin™", "event", data);
        packet_ref.inner.set_ack_id(1);
        let packet = decode(DATA);
        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_event_ack_root_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let data = to_msgpack(json!("data"));
        let packet = encode(Packet::ack("/", data, 54));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_root_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let data = to_msgpack(json!(["data"]));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::ack("/", data, 54));
    }

    #[test]
    fn packet_encode_event_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_msgpack(json!(["data"]));
        let packet = encode(Packet::ack("/admin™", data, 54));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_msgpack(json!(["data"]));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::ack("/admin™", data, 54));
    }

    #[test]
    fn packet_encode_binary_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::event("/", "event", data));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", "event", data));
    }

    #[test]
    fn packet_encode_binary_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let mut packet = Packet::event("/", "event", data);
        packet.inner.set_ack_id(254);
        let packet = encode(packet);

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105, 100, 204, 254,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let mut packet_ref = Packet::event("/", "event", data);
        packet_ref.inner.set_ack_id(254);
        let packet = decode(DATA);

        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_binary_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let packet = encode(Packet::event("/admin™", "event", data));

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let packet = decode(DATA);

        assert_eq!(packet, Packet::event("/admin™", "event", data));
    }

    #[test]
    fn packet_encode_binary_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105,
            100, 204, 254,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let mut packet = Packet::event("/admin™", "event", data);
        packet.inner.set_ack_id(254);
        let packet = encode(packet);
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105,
            100, 204, 254,
        ];
        let data = to_msgpack((json!({"data": "value™"}), Bytes::from_static(&[1, 2, 3, 4])));
        let mut packet_ref = Packet::event("/admin™", "event", data);
        packet_ref.inner.set_ack_id(254);
        let packet = decode(DATA);
        assert_eq!(packet, packet_ref);
    }

    #[test]
    fn packet_encode_binary_ack_root_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1,
            2, 3, 4, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::ack("/", data, 54));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_ack_root_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1,
            2, 3, 4, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(DATA);
        assert_eq!(packet, Packet::ack("/", data, 54));
    }

    #[test]
    fn packet_encode_binary_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 129, 164, 100, 97, 116, 97, 168, 118, 97,
            108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = encode(Packet::ack("/admin™", data, 54));

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 129, 164, 100, 97, 116, 97, 168, 118, 97,
            108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105, 100, 54,
        ];
        let data = to_msgpack((
            json!({ "data": "value™" }),
            Bytes::from_static(&[1, 2, 3, 4]),
        ));
        let packet = decode(DATA);

        assert_eq!(packet, Packet::ack("/admin™", data, 54));
    }
}
