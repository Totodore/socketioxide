use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

use bytes::Bytes;

use serde::{de::DeserializeOwned, Serialize};
use socketioxide_core::{
    packet::{Packet, PacketData},
    parser::{Parse, ParseError},
    SocketIoValue, Str,
};

mod de;
mod ser;
mod value;

/// Parse and serialize from and into the socket.io common packet format.
///
/// The resulting string should be in the format of:
/// ```text
/// <packet type>[<# of binary attachments>-][<namespace>,][<acknowledgment id>][JSON-stringified payload without binary]
/// + binary attachments extracted
/// ```
#[derive(Debug, Default)]
pub struct CommonParser {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet>>,
    /// The number of expected binary attachments (used when receiving data)
    pub incoming_binary_cnt: AtomicUsize,
}

impl Parse for CommonParser {
    type Error = serde_json::Error;
    fn encode(&self, packet: Packet) -> SocketIoValue {
        ser::serialize_packet(packet)
    }

    fn decode_str(&self, value: Str) -> Result<Packet, ParseError<Self::Error>> {
        let (packet, incoming_binary_cnt) = de::deserialize_packet(value)?;
        if packet.inner.is_binary() {
            let incoming_binary_cnt = incoming_binary_cnt.ok_or(ParseError::InvalidAttachments)?;
            if !is_bin_packet_complete(&packet.inner, incoming_binary_cnt) {
                *self.partial_bin_packet.lock().unwrap() = Some(packet);
                self.incoming_binary_cnt
                    .store(incoming_binary_cnt, Ordering::Release);
                Err(ParseError::NeedsMoreBinaryData)
            } else {
                Ok(packet)
            }
        } else {
            Ok(packet)
        }
    }

    fn decode_bin(&self, data: Bytes) -> Result<Packet, ParseError<Self::Error>> {
        let packet = &mut *self.partial_bin_packet.lock().unwrap();
        match packet {
            Some(Packet {
                inner:
                    PacketData::BinaryEvent(SocketIoValue::Str((_, binaries)), _)
                    | PacketData::BinaryAck(SocketIoValue::Str((_, binaries)), _),
                ..
            }) => {
                let binaries = binaries.get_or_insert(Vec::new());
                binaries.push(data);
                if self.incoming_binary_cnt.load(Ordering::Relaxed) > binaries.len() {
                    Err(ParseError::NeedsMoreBinaryData)
                } else {
                    Ok(packet.take().unwrap())
                }
            }
            _ => Err(ParseError::UnexpectedBinaryPacket),
        }
    }

    fn encode_value<T: Serialize>(
        &self,
        data: &T,
        event: Option<&str>,
    ) -> Result<SocketIoValue, Self::Error> {
        value::to_value(data, event)
    }

    fn decode_value<T: DeserializeOwned>(
        &self,
        value: SocketIoValue,
        with_event: bool,
    ) -> Result<T, Self::Error> {
        value::from_value(value, with_event)
    }
}

impl CommonParser {
    /// Create a new [`CommonParser`]. This is the default socket.io packet parser.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Check if the binary packet is complete, it means that all payloads have been received
fn is_bin_packet_complete(packet: &PacketData, incoming_binary_cnt: usize) -> bool {
    match &packet {
        PacketData::BinaryEvent(SocketIoValue::Str((_, binaries)), _)
        | PacketData::BinaryAck(SocketIoValue::Str((_, binaries)), _) => {
            incoming_binary_cnt == binaries.as_ref().map(Vec::len).unwrap_or(0)
        }
        _ => true,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;
    use socketioxide_core::{packet::ConnectPacket, Sid};

    fn to_event_value(data: &impl serde::Serialize, event: &str) -> SocketIoValue {
        CommonParser::default()
            .encode_value(data, Some(event))
            .unwrap()
    }

    fn to_value(data: &impl serde::Serialize) -> SocketIoValue {
        CommonParser::default().encode_value(data, None).unwrap()
    }
    fn to_connect_value(data: &impl serde::Serialize) -> SocketIoValue {
        SocketIoValue::Str((Str::from(serde_json::to_string(data).unwrap()), None))
    }
    fn encode(packet: Packet) -> String {
        match CommonParser::default().encode(packet) {
            SocketIoValue::Str((d, _)) => d.into(),
            SocketIoValue::Bytes(_) => panic!("testing only returns str"),
        }
    }
    fn decode(value: String) -> Packet {
        CommonParser::default().decode_str(value.into()).unwrap()
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
        let packet = encode(Packet::bin_event(
            "/",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        ));

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = Packet::bin_event(
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
        let packet = encode(Packet::bin_event(
            "/admin™",
            to_event_value(
                &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
                "event",
            ),
        ));

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::bin_event(
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
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| {
            let data = to_event_value(
                &(json!({"data": "value™"}), Bytes::from_static(&[1])),
                "event",
            );
            Packet {
                inner: PacketData::BinaryEvent(data, ack),
                ns: ns.into(),
            }
        };
        let parser = CommonParser::default();
        let payload = format!("51-{}", json);
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(None, "/"));

        // Check with ack ID
        let parser = CommonParser::default();
        let payload = format!("51-254{}", json);
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/"));

        // Check with NS
        let parser = CommonParser::default();
        let payload = format!("51-/admin™,{}", json);
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(None, "/admin™"));

        // Check with ack ID and NS
        let parser = CommonParser::default();
        let payload = format!("51-/admin™,254{}", json);
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/admin™"));
    }

    // BinaryAck(BinaryPacket, i64),
    #[test]
    fn packet_encode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("61-54{}", json);
        let packet = encode(Packet::bin_ack(
            "/",
            to_value(&(json!({ "data": "value™" }), Bytes::from_static(&[1]))),
            54,
        ));

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("61-/admin™,54{}", json);
        let packet = encode(Packet::bin_ack(
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
        let parser = CommonParser::default();
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(54, "/"));

        // Check with NS
        let parser = CommonParser::default();
        let payload = format!("61-/admin™,54{}", json);
        assert!(matches!(
            parser.decode_str(payload.into()),
            Err(ParseError::NeedsMoreBinaryData)
        ));
        let packet = parser.decode_bin(Bytes::from_static(&[1])).unwrap();
        assert_eq!(packet, comparison_packet(54, "/admin™"));
    }

    #[test]
    fn packet_reject_invalid_binary_event() {
        let payload = "5invalid".to_owned();
        let err = CommonParser::default()
            .decode_str(payload.into())
            .unwrap_err();

        assert!(matches!(err, ParseError::InvalidAttachments));
    }
}
