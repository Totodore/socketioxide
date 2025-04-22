#![warn(
    clippy::all,
    clippy::todo,
    clippy::empty_enum,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    rust_2024_compatibility,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]

//! The common parser sub-crate for the socketioxide crate.
//!
//! This is the default parser implementation.
//!
//! It is used to parse and serialize the common packet format of the socket.io protocol:
//! ```text
//! <packet type>[<# of binary attachments>-][<namespace>,][<acknowledgment id>][JSON-stringified payload without binary]
//! + binary attachments extracted
//! ```
use std::{collections::VecDeque, sync::atomic::Ordering};

use bytes::Bytes;

use serde::{Deserialize, Serialize};
use socketioxide_core::{
    Str, Value,
    packet::{Packet, PacketData},
    parser::{Parse, ParseError, ParserError, ParserState},
};

mod de;
mod ser;
mod value;

/// Parse and serialize from and into the socket.io common packet format.
/// See details in the [socket.io protocol doc](https://socket.io/fr/docs/v4/socket-io-protocol/#packet-encoding).
#[derive(Debug, Default, Clone, Copy)]
pub struct CommonParser;

impl Parse for CommonParser {
    fn encode(self, packet: Packet) -> Value {
        ser::serialize_packet(packet)
    }

    fn decode_str(self, state: &ParserState, value: Str) -> Result<Packet, ParseError> {
        let (packet, incoming_binary_cnt) = de::deserialize_packet(value)?;
        if packet.inner.is_binary() {
            let incoming_binary_cnt = incoming_binary_cnt.ok_or(ParseError::InvalidAttachments)?;
            if !is_bin_packet_complete(&packet.inner, incoming_binary_cnt) {
                *state.partial_bin_packet.lock().unwrap() = Some(packet);
                state
                    .incoming_binary_cnt
                    .store(incoming_binary_cnt, Ordering::Release);
                Err(ParseError::NeedsMoreBinaryData)
            } else {
                Ok(packet)
            }
        } else {
            Ok(packet)
        }
    }

    fn decode_bin(self, state: &ParserState, data: Bytes) -> Result<Packet, ParseError> {
        let packet = &mut *state.partial_bin_packet.lock().unwrap();
        match packet {
            Some(Packet {
                inner:
                    PacketData::BinaryEvent(Value::Str(_, binaries), _)
                    | PacketData::BinaryAck(Value::Str(_, binaries), _),
                ..
            }) => {
                let binaries = binaries.get_or_insert(VecDeque::new());
                // We copy the data to avoid holding a ref to the engine.io
                // websocket buffer too long.
                binaries.push_back(Bytes::copy_from_slice(&data));
                if state.incoming_binary_cnt.load(Ordering::Relaxed) > binaries.len() {
                    Err(ParseError::NeedsMoreBinaryData)
                } else {
                    Ok(packet.take().unwrap())
                }
            }
            _ => Err(ParseError::UnexpectedBinaryPacket),
        }
    }

    #[inline]
    fn encode_value<T: ?Sized + Serialize>(
        self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, ParserError> {
        value::to_value(data, event).map_err(ParserError::new)
    }

    #[inline]
    fn decode_value<'de, T: Deserialize<'de>>(
        self,
        value: &'de mut Value,
        with_event: bool,
    ) -> Result<T, ParserError> {
        value::from_value(value, with_event).map_err(ParserError::new)
    }

    fn decode_default<'de, T: Deserialize<'de>>(
        self,
        value: Option<&'de Value>,
    ) -> Result<T, ParserError> {
        if let Some(value) = value {
            let data = value
                .as_str()
                .expect("CommonParser only supports string values");
            serde_json::from_str(data).map_err(ParserError::new)
        } else {
            serde_json::from_str("{}").map_err(ParserError::new)
        }
    }

    fn encode_default<T: ?Sized + Serialize>(self, data: &T) -> Result<Value, ParserError> {
        let value = serde_json::to_string(data).map_err(ParserError::new)?;
        Ok(Value::Str(Str::from(value), None))
    }

    #[inline]
    fn read_event(self, value: &Value) -> Result<&str, ParserError> {
        value::read_event(value).map_err(ParserError::new)
    }
}

/// Check if the binary packet is complete, it means that all payloads have been received
fn is_bin_packet_complete(packet: &PacketData, incoming_binary_cnt: usize) -> bool {
    match &packet {
        PacketData::BinaryEvent(Value::Str(_, binaries), _)
        | PacketData::BinaryAck(Value::Str(_, binaries), _) => {
            incoming_binary_cnt == binaries.as_ref().map(VecDeque::len).unwrap_or(0)
        }
        _ => true,
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use crate::is_bin_packet_complete;

    use super::*;
    use serde_json::json;
    use socketioxide_core::{Sid, packet::ConnectPacket};

    fn to_event_value(data: &impl serde::Serialize, event: &str) -> Value {
        CommonParser.encode_value(data, Some(event)).unwrap()
    }

    fn to_value(data: &impl serde::Serialize) -> Value {
        CommonParser.encode_value(data, None).unwrap()
    }
    fn to_connect_value(data: &impl serde::Serialize) -> Value {
        Value::Str(Str::from(serde_json::to_string(data).unwrap()), None)
    }
    fn encode(packet: Packet) -> String {
        match CommonParser.encode(packet) {
            Value::Str(d, _) => d.into(),
            Value::Bytes(_) => panic!("testing only returns str"),
        }
    }
    fn decode(value: String) -> Packet {
        CommonParser
            .decode_str(&Default::default(), value.into())
            .unwrap()
    }

    #[test]
    fn packet_decode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = decode(payload);
        let value = to_connect_value(&ConnectPacket { sid });
        assert_eq!(Packet::connect("/", Some(value.clone())), packet);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet = decode(payload);
        assert_eq!(Packet::connect("/admin™", Some(value)), packet);
    }

    #[test]
    fn packet_encode_connect() {
        let sid = Sid::new();
        let value = to_connect_value(&ConnectPacket { sid });
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = encode(Packet::connect("/", Some(value.clone())));
        assert_eq!(packet, payload);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet: String = encode(Packet::connect("/admin™", Some(value)));
        assert_eq!(packet, payload);
    }

    // Disconnect

    #[test]
    fn packet_decode_disconnect() {
        let payload = "1".to_string();
        let packet = decode(payload);
        assert_eq!(Packet::disconnect("/"), packet);

        let payload = "1/admin™,".to_string();
        let packet = decode(payload);
        assert_eq!(Packet::disconnect("/admin™"), packet);
    }

    #[test]
    fn packet_encode_disconnect() {
        let payload = "1".to_string();
        let packet = encode(Packet::disconnect("/"));
        assert_eq!(packet, payload);

        let payload = "1/admin™,".to_string();
        let packet = encode(Packet::disconnect("/admin™"));
        assert_eq!(packet, payload);
    }

    // Event(String, Value, Option<i64>),
    #[test]
    fn packet_decode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value" }]));
        let packet = decode(payload);

        assert_eq!(
            Packet::event("/", to_event_value(&json!({"data": "value"}), "event")),
            packet
        );

        // Check with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value" }]));
        let packet = decode(payload);

        let mut comparison_packet =
            Packet::event("/", to_event_value(&json!({"data": "value"}), "event"));
        comparison_packet.inner.set_ack_id(1);
        assert_eq!(packet, comparison_packet);

        // Check with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = decode(payload);

        assert_eq!(
            Packet::event(
                "/admin™",
                to_event_value(&json!({"data": "value™"}), "event")
            ),
            packet
        );

        // Check with ack ID and NS
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = decode(payload);
        packet.inner.set_ack_id(1);

        let mut comparison_packet = Packet::event(
            "/admin™",
            to_event_value(&json!({"data": "value™"}), "event"),
        );
        comparison_packet.inner.set_ack_id(1);

        assert_eq!(packet, comparison_packet);
    }

    #[test]
    fn packet_encode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value™" }]));
        let packet = encode(Packet::event(
            "/",
            to_event_value(&json!({ "data": "value™" }), "event"),
        ));

        assert_eq!(packet, payload);

        // Encode empty data
        let payload = format!("2{}", json!(["event", []]));
        let packet = encode(Packet::event("/", to_event_value(&json!([]), "event")));

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event("/", to_event_value(&json!({ "data": "value™" }), "event"));
        packet.inner.set_ack_id(1);
        let packet = encode(packet);

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = encode(Packet::event(
            "/admin™",
            to_event_value(&json!({"data": "value™"}), "event"),
        ));

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event(
            "/admin™",
            to_event_value(&json!({"data": "value™"}), "event"),
        );
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(packet, payload);
    }

    // EventAck(Value, i64),
    #[test]
    fn packet_decode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet = decode(payload);

        assert_eq!(Packet::ack("/", to_value(&json!("data")), 54), packet);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = decode(payload);

        assert_eq!(Packet::ack("/admin™", to_value(&json!("data")), 54), packet);
    }

    #[test]
    fn packet_encode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet = encode(Packet::ack("/", to_value(&json!("data")), 54));
        assert_eq!(packet, payload);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = encode(Packet::ack("/admin™", to_value(&json!("data")), 54));
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_encode_connect_error() {
        let payload = format!("4{}", json!({ "message": "Invalid namespace" }));
        let packet = encode(Packet::connect_error("/", "Invalid namespace"));
        assert_eq!(packet, payload);

        let payload = format!("4/admin™,{}", json!({ "message": "Invalid namespace" }));
        let packet = encode(Packet::connect_error("/admin™", "Invalid namespace"));
        assert_eq!(packet, payload);
    }

    // BinaryEvent(String, BinaryPacket, Option<i64>),
    #[test]
    fn packet_encode_binary_event() {
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("51-{}", json);
        let packet = encode(Packet::event(
            "/",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        ));

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = Packet::event(
            "/",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("51-/admin™,{}", json);
        let packet = encode(Packet::event(
            "/admin™",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        ));

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::event(
            "/admin™",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_event() {
        let json = json!(["event", { "data": "value™" },{ "_placeholder": true, "num": 0},{ "_placeholder": true, "num": 1}]);
        let comparison_packet = |ack, ns: &'static str| {
            let data = to_event_value(
                &(
                    json!({"data": "value™"}),
                    Bytes::from_static(&[1]),
                    Bytes::from_static(&[2]),
                ),
                "event",
            );
            Packet {
                inner: PacketData::BinaryEvent(data, ack),
                ns: ns.into(),
            }
        };
        let state = ParserState::default();
        let payload = format!("52-{}", json);
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        assert!(matches!(
            CommonParser.decode_bin(&state, Bytes::from_static(&[1])),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[2]))
            .unwrap();

        assert_eq!(packet, comparison_packet(None, "/"));

        // Check with ack ID
        let state = ParserState::default();
        let payload = format!("52-254{}", json);
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        assert!(matches!(
            CommonParser.decode_bin(&state, Bytes::from_static(&[1])),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[2]))
            .unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/"));

        // Check with NS
        let state = ParserState::default();
        let payload = format!("52-/admin™,{}", json);
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        assert!(matches!(
            CommonParser.decode_bin(&state, Bytes::from_static(&[1])),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[2]))
            .unwrap();

        assert_eq!(packet, comparison_packet(None, "/admin™"));

        // Check with ack ID and NS
        let state = ParserState::default();
        let payload = format!("52-/admin™,254{}", json);
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        assert!(matches!(
            CommonParser.decode_bin(&state, Bytes::from_static(&[1])),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[2]))
            .unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/admin™"));
    }

    // BinaryAck(BinaryPacket, i64),
    #[test]
    fn packet_encode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("61-54{}", json);
        let packet = encode(Packet::ack(
            "/",
            to_value(&(json!({ "data": "value™" }), Bytes::from_static(&[1]))),
            54,
        ));

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("61-/admin™,54{}", json);
        let packet = encode(Packet::ack(
            "/admin™",
            to_value(&(json!({ "data": "value™" }), Bytes::from_static(&[1]))),
            54,
        ));

        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryAck(
                to_value(&(json!({ "data": "value™" }), Bytes::from_static(&[1]))),
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("61-54{}", json);
        let state = ParserState::default();
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[1]))
            .unwrap();

        assert_eq!(packet, comparison_packet(54, "/"));

        // Check with NS
        let state = ParserState::default();
        let payload = format!("61-/admin™,54{}", json);
        assert!(matches!(
            CommonParser.decode_str(&state, payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = CommonParser
            .decode_bin(&state, Bytes::from_static(&[1]))
            .unwrap();
        assert_eq!(packet, comparison_packet(54, "/admin™"));
    }

    #[test]
    fn packet_reject_invalid_binary_event() {
        let payload = "5invalid".to_owned();
        let err = CommonParser
            .decode_str(&Default::default(), payload.into())
            .unwrap_err();

        assert!(matches!(err, ParseError::InvalidAttachments));
    }

    #[test]
    fn decode_default_none() {
        // Common parser should deserialize by default to an empty map to match the behavior of the
        // socket.io client when deserializing incoming connect message without an auth payload.
        let data = CommonParser.decode_default::<HashMap<String, ()>>(None);
        assert!(matches!(data, Ok(d) if d.is_empty()));
    }

    #[test]
    fn decode_default_some() {
        // Common parser should deserialize by default to an empty map to match the behavior of the
        // socket.io client when deserializing incoming connect message without an auth payload.
        let data =
            CommonParser.decode_default::<String>(Some(&Value::Str("\"test\"".into(), None)));
        assert!(matches!(data, Ok(d) if d == "test"));
    }

    #[test]
    fn encode_default() {
        let data = CommonParser.encode_default(&20);
        assert!(matches!(data, Ok(Value::Str(d, None)) if d == "20"));
    }

    #[test]
    fn read_event() {
        let data = Value::Str(r#"["event",{"data":1,"complex":[132,12]}]"#.into(), None);
        let event = CommonParser.read_event(&data).unwrap();
        assert_eq!(event, "event");
    }

    #[test]
    fn unexpected_bin_packet() {
        let err = CommonParser.decode_bin(&Default::default(), Bytes::new());
        assert!(matches!(err, Err(ParseError::UnexpectedBinaryPacket)));
    }
    #[test]
    fn check_is_bin_packet_complete() {
        let data = PacketData::BinaryEvent(Value::Str("".into(), Some(vec![].into())), None);
        assert!(!is_bin_packet_complete(&data, 2));
        assert!(is_bin_packet_complete(&data, 0));
        let data =
            PacketData::BinaryAck(Value::Str("".into(), Some(vec![Bytes::new()].into())), 12);
        assert!(is_bin_packet_complete(&data, 1));
        assert!(!is_bin_packet_complete(&data, 2));

        // any other packet
        let data = PacketData::Connect(None);
        assert!(is_bin_packet_complete(&data, 0));
        assert!(is_bin_packet_complete(&data, 1));
    }
}
