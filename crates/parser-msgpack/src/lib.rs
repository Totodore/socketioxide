use bytes::Bytes;
use de::deserialize_packet;
use ser::serialize_packet;
use socketioxide_core::{
    packet::Packet,
    parser::{Parse, ParseError, ParserState},
    Str, Value,
};

mod de;
mod ser;
mod value;

/// The MsgPack parser
#[derive(Default, Debug, Clone, Copy)]
pub struct MsgPackParser;

impl Parse for MsgPackParser {
    type EncodeError = rmp_serde::encode::Error;
    type DecodeError = rmp_serde::decode::Error;

    fn encode(self, packet: Packet) -> socketioxide_core::Value {
        let data = serialize_packet(packet);
        Value::Bytes(data.into())
    }

    fn decode_str(
        self,
        _: &ParserState,
        _data: Str,
    ) -> Result<Packet, ParseError<Self::DecodeError>> {
        Err(ParseError::UnexpectedStringPacket)
    }

    fn decode_bin(
        self,
        _: &ParserState,
        bin: Bytes,
    ) -> Result<Packet, ParseError<Self::DecodeError>> {
        Ok(deserialize_packet(bin)?)
    }

    fn encode_value<T: serde::Serialize>(
        self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, Self::EncodeError> {
        value::to_value(data, event)
    }

    fn decode_value<T: serde::de::DeserializeOwned>(
        self,
        value: &Value,
        with_event: bool,
    ) -> Result<T, Self::DecodeError> {
        value::from_value(value, with_event)
    }

    fn value_none(self) -> Value {
        Value::Bytes(Bytes::from_static(&[0xc0]))
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
    const BIN: Bytes = Bytes::from_static(&[1, 2, 3, 4]);

    fn decode(value: &'static [u8]) -> Packet {
        MsgPackParser
            .decode_bin(&Default::default(), Bytes::from_static(value))
            .unwrap()
    }
    fn encode(packet: Packet) -> Bytes {
        match MsgPackParser.encode(packet) {
            Value::Bytes(b) => b,
            Value::Str(_, _) => panic!("implementation should only return bytes"),
        }
    }
    fn to_event_value(data: &impl serde::Serialize, event: &str) -> Value {
        MsgPackParser.encode_value(data, Some(event)).unwrap()
    }
    fn to_value(data: &impl serde::Serialize) -> Value {
        MsgPackParser.encode_value(data, None).unwrap()
    }
    fn to_connect_value(data: &impl serde::Serialize) -> Value {
        Value::Bytes(rmp_serde::to_vec_named(data).unwrap().into())
    }

    #[test]
    fn packet_encode_connect_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 0, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            129, 163, 115, 105, 100, 176, 110, 119, 122, 51, 67, 56, 117, 55, 113, 121, 115, 118,
            103, 86, 113, 106,
        ];

        let sid = Sid::from_str("nwz3C8u7qysvgVqj").unwrap();
        let sid = to_connect_value(&ConnectPacket { sid });
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
        let sid = to_connect_value(&ConnectPacket { sid });
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
        let sid = to_connect_value(&ConnectPacket { sid });
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
        let sid = to_connect_value(&ConnectPacket { sid });
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
        let packet = encode(Packet::event(
            "/",
            to_event_value(&json!({ "data": "value™" }), "event"),
        ));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162,
        ];
        let data = to_event_value(&json!({ "data": "value™" }), "event");
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", data));
    }
    #[test]
    fn packet_decode_event_root_ns_empty_data() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 165, 101, 118, 101, 110, 116,
        ];
        let data = to_event_value(&[0; 0], "event");
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", data));
    }

    #[test]
    fn packet_encode_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            146, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_event_value(&json!({ "data": "value™" }), "event");
        let mut packet = Packet::event("/", data);
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
        let data = to_event_value(&json!({ "data": "value™" }), "event");
        let mut packet_comp = Packet::event("/", data);
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
        let data = to_event_value(&json!({"data": "value™"}), "event");
        let packet = encode(Packet::event("/admin™", data));

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162,
        ];
        let data = to_event_value(&json!({"data": "value™"}), "event");
        let packet = decode(DATA);

        assert_eq!(packet, Packet::event("/admin™", data));
    }
    #[test]
    fn packet_encode_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 146, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 162, 105, 100, 1,
        ];
        let data = to_event_value(&json!({"data": "value™"}), "event");
        let mut packet = Packet::event("/admin™", data);
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
        let data = to_event_value(&json!({"data": "value™"}), "event");
        let mut packet_ref = Packet::event("/admin™", data);
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
        let data = to_value(&"data");
        let packet = encode(Packet::ack("/", data, 54));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_root_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];
        let data = to_value(&"data");
        let packet = decode(DATA);
        assert_eq!(packet, Packet::ack("/", data, 54));
    }

    #[test]
    fn packet_encode_event_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_value(&"data");
        let packet = encode(Packet::ack("/admin™", data, 54));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_event_ack_custom_ns() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 3, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 145, 164, 100, 97, 116, 97, 162, 105, 100, 54,
        ];

        let data = to_value(&["data"]);
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
        let data = to_event_value(&(json!({ "data": "value™" }), BIN), "event");
        let packet = encode(Packet::event("/", data));
        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_root_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_event_value(&(json!({ "data": "value™" }), BIN), "event");
        let packet = decode(DATA);
        assert_eq!(packet, Packet::event("/", data));
    }

    #[test]
    fn packet_encode_binary_event_root_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 161, 47, 164, 100, 97, 116, 97,
            147, 165, 101, 118, 101, 110, 116, 129, 164, 100, 97, 116, 97, 168, 118, 97, 108, 117,
            101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105, 100, 204, 254,
        ];
        let data = to_event_value(&(json!({ "data": "value™" }), BIN), "event");
        let mut packet = Packet::event("/", data);
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
        let data = to_event_value(&(json!({ "data": "value™" }), BIN), "event");
        let mut packet_ref = Packet::event("/", data);
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
        let data = to_event_value(&(json!({"data": "value™"}), BIN), "event");
        let packet = encode(Packet::event("/admin™", data));

        assert_eq!(DATA, packet.as_ref());
    }

    #[test]
    fn packet_decode_binary_event_custom_ns() {
        const DATA: &'static [u8] = &[
            131, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4,
        ];
        let data = to_event_value(&(json!({"data": "value™"}), BIN), "event");
        let packet = decode(DATA);

        assert_eq!(packet, Packet::event("/admin™", data));
    }

    #[test]
    fn packet_encode_binary_event_custom_ns_with_ack_id() {
        const DATA: &'static [u8] = &[
            132, 164, 116, 121, 112, 101, 2, 163, 110, 115, 112, 169, 47, 97, 100, 109, 105, 110,
            226, 132, 162, 164, 100, 97, 116, 97, 147, 165, 101, 118, 101, 110, 116, 129, 164, 100,
            97, 116, 97, 168, 118, 97, 108, 117, 101, 226, 132, 162, 196, 4, 1, 2, 3, 4, 162, 105,
            100, 204, 254,
        ];
        let data = to_event_value(&(json!({"data": "value™"}), BIN), "event");
        let mut packet = Packet::event("/admin™", data);
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
        let data = to_event_value(&(json!({"data": "value™"}), BIN), "event");
        let mut packet_ref = Packet::event("/admin™", data);
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
        let data = to_value(&(json!({ "data": "value™" }), BIN));
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
        let data = to_value(&(json!({ "data": "value™" }), BIN));
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
        let data = to_value(&(json!({ "data": "value™" }), BIN));
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
        let data = to_value(&(json!({ "data": "value™" }), BIN));
        let packet = decode(DATA);

        assert_eq!(packet, Packet::ack("/admin™", data, 54));
    }
}
