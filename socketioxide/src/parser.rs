use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use dyn_clone::DynClone;
use futures::AsyncReadExt;
use serde::de::DeserializeOwned;
use engineioxide::Socket as EIoSocket;
use crate::client::SocketData;
use crate::errors::Error;
use crate::packet::Packet;

pub mod msgpack;
pub mod default;

/// A payload that is sent over the network.
/// Can be either an [`Emittable(String)`] to send a plaintext payload
/// or an [`Emittable(Vec<u8>)`] to send a binary payload.
pub enum Emittable {
    /// A string payload that will be sent as a string websocket frame over the network.
    String(String),
    /// A binary payload that will be sent as a binary websocket frame over the network.
    Binary(Vec<u8>)
}

/// Interface that can be implemented to support custom protocols or serialization formats.
///
/// # Available Parsers
/// Currently, socketioxide supports two parsers to choose from:
/// - **[`default::DefaultParser`]**
/// - **[`msgpack::MsgpackParser`]**
///
/// Configuration happens during the initialization of the SocketIo instance:
/// ```rust
/// use socketioxide::parser::msgpack::MsgpackParser;
/// use socketioxide::SocketIoBuilder;
///
/// let (layer, io) = SocketIoBuilder::new()
///     .with_parser(MsgpackParser)
///     .build_layer();
/// ```
///
pub trait Parser: DynClone + Send + Sync + 'static {
    /// Encodes the packet into an array of websocket payloads that are sent over the connection.
    ///
    /// # Returns
    /// An array of [`Emittable`] which can be either [`Emittable(String)`] to send a plaintext payload
    /// or [`Emittable(Vec<u8>)`] to send a binary payload.
    fn encode(&self, packet: Packet) -> Vec<Emittable>;
    /// Decodes a string packet into its general form [`Packet`].
    /// # Returns
    /// If [`Ok(Packet)`] is returned, it will be further processed and passed to the receiving namespace if it is a text packet.
    /// If [`Err()`] is returned, a serialization error will be thrown and the packet is lost.
    fn decode_msg<'a>(&self, msg: String, socket: Arc<EIoSocket<SocketData>>) -> Result<Packet<'a>, Error>;
    /// Decodes a binary packet into its general form [`Packet`].
    /// # Returns
    /// If [`Some(Packet)`] is returned, it will be immediately passed down to the receiving namespace as a binary packet.
    /// If [`None`] is returned, nothing happens, and the logic for e.g. collecting all binary packets need to be implemented in this function.
    fn decode_bin<'a>(&self, bin: Vec<u8>, socket: Arc<EIoSocket<SocketData>>) -> Option<Packet<'a>>;
}

dyn_clone::clone_trait_object!(Parser);

impl Debug for dyn Parser {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parser()")
    }
}

/*
#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    #[test]
    fn packet_decode_connect() {
        let parser = DefaultParser::default();
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(Packet::connect("/", sid, ProtocolVersion::V5), packet);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(Packet::connect("/admin™", sid, ProtocolVersion::V5), packet);
    }

    #[test]
    fn packet_encode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet: String = Packet::connect("/", sid, ProtocolVersion::V5)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet: String = Packet::connect("/admin™", sid, ProtocolVersion::V5)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);
    }

    // Disconnect,

    #[test]
    fn packet_decode_disconnect() {
        let parser = DefaultParser::default();
        let payload = "1".to_string();
        let packet = parser.decode_msg(payload).unwrap();
        assert_eq!(Packet::disconnect("/"), packet);

        let payload = "1/admin™,".to_string();
        let packet = parser.decode_msg(payload).unwrap();
        assert_eq!(Packet::disconnect("/admin™"), packet);
    }

    #[test]
    fn packet_encode_disconnect() {
        let payload = "1".to_string();
        let packet: String = Packet::disconnect("/").try_into().unwrap();
        assert_eq!(packet, payload);

        let payload = "1/admin™,".to_string();
        let packet: String = Packet::disconnect("/admin™").try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // Event(String, Value, Option<i64>),
    #[test]
    fn packet_decode_event() {
        let parser = DefaultParser::default();
        let payload = format!("2{}", json!(["event", { "data": "value" }]));
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(
            Packet::event("/", "event", json!([{"data": "value"}])),
            packet
        );

        // Check with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value" }]));
        let packet = parser.decode_msg(payload).unwrap();

        let mut comparison_packet = Packet::event("/", "event", json!([{"data": "value"}]));
        comparison_packet.inner.set_ack_id(1);
        assert_eq!(packet, comparison_packet);

        // Check with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(
            Packet::event("/admin™", "event", json!([{"data": "value™"}])),
            packet
        );

        // Check with ack ID and NS
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = parser.decode_msg(payload).unwrap();
        packet.inner.set_ack_id(1);

        let mut comparison_packet = Packet::event("/admin™", "event", json!([{"data": "value™"}]));
        comparison_packet.inner.set_ack_id(1);

        assert_eq!(packet, comparison_packet);
    }

    #[test]
    fn packet_encode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value™" }]));
        let packet: String = Packet::event("/", "event", json!({ "data": "value™" }))
            .try_into()
            .unwrap();

        assert_eq!(packet, payload);

        // Encode empty data
        let payload = format!("2{}", json!(["event", []]));
        let packet: String = Packet::event("/", "event", json!([])).try_into().unwrap();

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event("/", "event", json!({ "data": "value™" }));
        packet.inner.set_ack_id(1);
        let packet: String = packet.try_into().unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet: String = Packet::event("/admin™", "event", json!({"data": "value™"}))
            .try_into()
            .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event("/admin™", "event", json!([{"data": "value™"}]));
        packet.inner.set_ack_id(1);
        let packet: String = packet.try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // EventAck(Value, i64),
    #[test]
    fn packet_decode_event_ack() {
        let parser = DefaultParser::default();
        let payload = "354[\"data\"]".to_string();
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(Packet::ack("/", json!(["data"]), 54), packet);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = parser.decode_msg(payload).unwrap();

        assert_eq!(Packet::ack("/admin™", json!(["data"]), 54), packet);
    }

    #[test]
    fn packet_encode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet: String = Packet::ack("/", json!("data"), 54).try_into().unwrap();
        assert_eq!(packet, payload);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet: String = Packet::ack("/admin™", json!("data"), 54)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_encode_connect_error() {
        let payload = format!("4{}", json!({ "message": "Invalid namespace" }));
        let packet: String = Packet::invalid_namespace("/").try_into().unwrap();
        assert_eq!(packet, payload);

        let payload = format!("4/admin™,{}", json!({ "message": "Invalid namespace" }));
        let packet: String = Packet::invalid_namespace("/admin™").try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // BinaryEvent(String, BinaryPacket, Option<i64>),
    #[test]
    fn packet_encode_binary_event() {
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("51-{}", json);
        let packet: String =
            Packet::bin_event("/", "event", json!({ "data": "value™" }), vec![vec![1]])
                .try_into()
                .unwrap();

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("51-254{}", json);
        let mut packet =
            Packet::bin_event("/", "event", json!({ "data": "value™" }), vec![vec![1]]);
        packet.inner.set_ack_id(254);
        let packet: String = packet.try_into().unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("51-/admin™,{}", json);
        let packet: String = Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![vec![1]],
        )
            .try_into()
            .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![vec![1]],
        );
        packet.inner.set_ack_id(254);
        let packet: String = packet.try_into().unwrap();
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_event() {
        let parser = DefaultParser::default();
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryEvent(
                "event".into(),
                BinaryPacket {
                    bin: vec![vec![1]],
                    data: json!([{"data": "value™"}]),
                    payload_count: 1,
                },
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("51-{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(None, "/"));

        // Check with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(Some(254), "/"));

        // Check with NS
        let payload = format!("51-/admin™,{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(None, "/admin™"));

        // Check with ack ID and NS
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }
        assert_eq!(packet, comparison_packet(Some(254), "/admin™"));
    }

    // BinaryAck(BinaryPacket, i64),
    #[test]
    fn packet_encode_binary_ack() {
        let parser = DefaultParser::default();
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("61-54{}", json);
        let packet: String = Packet::bin_ack("/", json!({ "data": "value™" }), vec![vec![1]], 54)
            .try_into()
            .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("61-/admin™,54{}", json);
        let packet: String =
            Packet::bin_ack("/admin™", json!({ "data": "value™" }), vec![vec![1]], 54)
                .try_into()
                .unwrap();

        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_ack() {
        let parser = DefaultParser::default();
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryAck(
                BinaryPacket {
                    bin: vec![vec![1]],
                    data: json!([{"data": "value™"}]),
                    payload_count: 1,
                },
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("61-54{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryAck(ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(54, "/"));

        // Check with NS
        let payload = format!("61-/admin™,54{}", json);
        let mut packet = parser.decode_msg(payload).unwrap();
        match packet.inner {
            PacketData::BinaryAck(ref mut bin, _) => bin.add_payload(vec![1]),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(54, "/admin™"));
    }

    #[test]
    fn packet_size_hint() {
        let sid = Sid::new();
        let len = serde_json::to_string(&ConnectPacket { sid }).unwrap().len();
        let packet = Packet::connect("/", sid, ProtocolVersion::V5);
        assert_eq!(DefaultParser::get_size_hint(&packet), len + 1);

        let packet = Packet::connect("/admin", sid, ProtocolVersion::V5);
        assert_eq!(DefaultParser::get_size_hint(&packet), len + 8);

        let packet = Packet::connect("admin", sid, ProtocolVersion::V4);
        assert_eq!(DefaultParser::get_size_hint(&packet), 8);

        let packet = Packet::disconnect("/");
        assert_eq!(DefaultParser::get_size_hint(&packet), 1);

        let packet = Packet::disconnect("/admin");
        assert_eq!(DefaultParser::get_size_hint(&packet), 8);

        let packet = Packet::event("/", "event", json!({ "data": "value™" }));
        assert_eq!(DefaultParser::get_size_hint(&packet), 1);

        let packet = Packet::event("/admin", "event", json!({ "data": "value™" }));
        assert_eq!(DefaultParser::get_size_hint(&packet), 8);

        let packet = Packet::ack("/", json!("data"), 54);
        assert_eq!(DefaultParser::get_size_hint(&packet), 3);

        let packet = Packet::ack("/admin", json!("data"), 54);
        assert_eq!(DefaultParser::get_size_hint(&packet), 10);

        let packet = Packet::bin_event("/", "event", json!({ "data": "value™" }), vec![vec![1]]);
        assert_eq!(DefaultParser::get_size_hint(&packet), 3);

        let packet = Packet::bin_event(
            "/admin",
            "event",
            json!({ "data": "value™" }),
            vec![vec![1]],
        );
        assert_eq!(DefaultParser::get_size_hint(&packet), 10);

        let packet = Packet::bin_ack("/", json!("data"), vec![vec![1]], 54);
        assert_eq!(DefaultParser::get_size_hint(&packet), 5);
    }
}
*/