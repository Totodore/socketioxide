//! Socket.io packet implementation.
//! The [`Packet`] is the base unit of data that is sent over the engine.io socket.

use serde::{Deserialize, Serialize};

pub use engineioxide::{sid::Sid, Str};

use crate::Value;

/// The socket.io packet type.
/// Each packet has a type and a namespace
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    /// The packet data
    pub inner: PacketData,
    /// The namespace the packet belongs to
    pub ns: Str,
}

impl Packet {
    /// Send a connect packet with a default payload for v5 and no payload for v4
    pub fn connect(ns: impl Into<Str>, value: Option<Value>) -> Self {
        Self {
            inner: PacketData::Connect(value),
            ns: ns.into(),
        }
    }

    /// Create a disconnect packet for the given namespace
    pub fn disconnect(ns: impl Into<Str>) -> Self {
        Self {
            inner: PacketData::Disconnect,
            ns: ns.into(),
        }
    }
}

impl Packet {
    /// Create a connect error packet for the given namespace with a message
    pub fn connect_error(ns: impl Into<Str>, message: impl Into<String>) -> Self {
        Self {
            inner: PacketData::ConnectError(message.into()),
            ns: ns.into(),
        }
    }

    /// Create an event packet for the given namespace.
    /// If the there is adjacent binary data, it will be a binary packet.
    pub fn event(ns: impl Into<Str>, data: Value) -> Self {
        Self {
            inner: match data {
                Value::Str(_, Some(ref bins)) if !bins.is_empty() => {
                    PacketData::BinaryEvent(data, None)
                }
                _ => PacketData::Event(data, None),
            },
            ns: ns.into(),
        }
    }

    /// Create an ack packet for the given namespace.
    /// If the there is adjacent binary data, it will be a binary packet.
    pub fn ack(ns: impl Into<Str>, data: Value, ack: i64) -> Self {
        Self {
            inner: match data {
                Value::Str(_, Some(ref bins)) if !bins.is_empty() => {
                    PacketData::BinaryAck(data, ack)
                }
                _ => PacketData::EventAck(data, ack),
            },
            ns: ns.into(),
        }
    }
}

/// | Type          | ID  | Usage                                                                                 |
/// |---------------|-----|---------------------------------------------------------------------------------------|
/// | CONNECT       | 0   | Used during the [connection to a namespace](#connection-to-a-namespace).              |
/// | DISCONNECT    | 1   | Used when [disconnecting from a namespace](#disconnection-from-a-namespace).          |
/// | EVENT         | 2   | Used to [send data](#sending-and-receiving-data) to the other side.                   |
/// | ACK           | 3   | Used to [acknowledge](#acknowledgement) an event.                                     |
/// | CONNECT_ERROR | 4   | Used during the [connection to a namespace](#connection-to-a-namespace).              |
/// | BINARY_EVENT  | 5   | Used to [send binary data](#sending-and-receiving-data) to the other side.            |
/// | BINARY_ACK    | 6   | Used to [acknowledge](#acknowledgement) an event (the response includes binary data). |
#[derive(Debug, Clone, PartialEq)]
pub enum PacketData {
    /// Connect packet with optional payload (only used with v5 for response)
    Connect(Option<Value>),
    /// Disconnect packet, used to disconnect from a namespace
    Disconnect,
    /// Event packet with optional ack id, to request an ack from the other side
    Event(Value, Option<i64>),
    /// Event ack packet, to acknowledge an event
    EventAck(Value, i64),
    /// Connect error packet, sent when the namespace is invalid
    ConnectError(String),
    /// Binary event packet with optional ack id, to request an ack from the other side
    BinaryEvent(Value, Option<i64>),
    /// Binary ack packet, to acknowledge an event with binary data
    BinaryAck(Value, i64),
}

impl PacketData {
    /// Returns the index of the packet type
    pub fn index(&self) -> usize {
        match self {
            PacketData::Connect(_) => 0,
            PacketData::Disconnect => 1,
            PacketData::Event(_, _) => 2,
            PacketData::EventAck(_, _) => 3,
            PacketData::ConnectError(_) => 4,
            PacketData::BinaryEvent(_, _) => 5,
            PacketData::BinaryAck(_, _) => 6,
        }
    }

    /// Set the ack id for the packet
    /// It will only set the ack id for the packets that support it
    pub fn set_ack_id(&mut self, ack_id: i64) {
        match self {
            PacketData::Event(_, ack) | PacketData::BinaryEvent(_, ack) => *ack = Some(ack_id),
            _ => {}
        };
    }

    /// Check if the packet is a binary packet (either binary event or binary ack)
    pub fn is_binary(&self) -> bool {
        matches!(
            self,
            PacketData::BinaryEvent(_, _) | PacketData::BinaryAck(_, _)
        )
    }
}

/// Connect packet sent by the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectPacket {
    /// The socket ID
    pub sid: Sid,
}

impl Serialize for Packet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct RawPacket<'a> {
            ns: &'a Str,
            r#type: u8,
            data: Option<&'a Value>,
            ack: Option<i64>,
            error: Option<&'a String>,
        }
        let (r#type, data, ack, error) = match &self.inner {
            PacketData::Connect(v) => (0, v.as_ref(), None, None),
            PacketData::Disconnect => (1, None, None, None),
            PacketData::Event(v, ack) => (2, Some(v), *ack, None),
            PacketData::EventAck(v, ack) => (3, Some(v), Some(*ack), None),
            PacketData::ConnectError(e) => (4, None, None, Some(e)),
            PacketData::BinaryEvent(v, ack) => (5, Some(v), *ack, None),
            PacketData::BinaryAck(v, ack) => (6, Some(v), Some(*ack), None),
        };
        let raw = RawPacket {
            ns: &self.ns,
            data,
            ack,
            error,
            r#type,
        };
        raw.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Packet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawPacket {
            ns: Str,
            r#type: u8,
            data: Option<Value>,
            ack: Option<i64>,
            error: Option<String>,
        }
        let raw = RawPacket::deserialize(deserializer)?;
        let err = |field| serde::de::Error::custom(format!("missing field: {}", field));
        let inner = match raw.r#type {
            0 => PacketData::Connect(raw.data),
            1 => PacketData::Disconnect,
            2 => PacketData::Event(raw.data.ok_or(err("data"))?, raw.ack),
            3 => PacketData::EventAck(raw.data.ok_or(err("data"))?, raw.ack.ok_or(err("ack"))?),
            4 => PacketData::ConnectError(raw.error.ok_or(err("error"))?),
            5 => PacketData::BinaryEvent(raw.data.ok_or(err("data"))?, raw.ack),
            6 => PacketData::BinaryAck(raw.data.ok_or(err("data"))?, raw.ack.ok_or(err("ack"))?),
            i => return Err(serde::de::Error::custom(format!("invalid packet type {i}"))),
        };
        Ok(Self { inner, ns: raw.ns })
    }
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

    use super::{Packet, PacketData, Value};
    use bytes::Bytes;

    #[test]
    fn should_create_bin_packet_with_adjacent_binary() {
        let val = Value::Str(
            "test".into(),
            Some(vec![Bytes::from_static(&[1, 2, 3])].into()),
        );
        assert!(matches!(
            Packet::event("/", val.clone()).inner,
            PacketData::BinaryEvent(v, None) if v == val));

        assert!(matches!(
            Packet::ack("/", val.clone(), 120).inner,
            PacketData::BinaryAck(v, 120) if v == val));
    }

    #[test]
    fn should_create_default_packet_with_base_data() {
        let val = Value::Str("test".into(), None);
        let val1 = Value::Bytes(Bytes::from_static(b"test"));

        assert!(matches!(
            Packet::event("/", val.clone()).inner,
            PacketData::Event(v, None) if v == val));

        assert!(matches!(
            Packet::ack("/", val.clone(), 120).inner,
            PacketData::EventAck(v, 120) if v == val));

        assert!(matches!(
            Packet::event("/", val1.clone()).inner,
            PacketData::Event(v, None) if v == val1));

        assert!(matches!(
            Packet::ack("/", val1.clone(), 120).inner,
            PacketData::EventAck(v, 120) if v == val1));
    }

    fn assert_serde_packet(packet: Packet) {
        let serialized = serde_json::to_string(&packet).unwrap();
        let deserialized: Packet = serde_json::from_str(&serialized).unwrap();
        assert_eq!(packet, deserialized);
    }
    #[test]
    fn packet_serde_connect() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::Connect(Some(Value::Str("test_data".into(), None))),
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_disconnect() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::Disconnect,
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_event() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::Event(Value::Str("event_data".into(), None), None),
        };
        assert_serde_packet(packet);

        let mut bins = VecDeque::new();
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::Event(Value::Str("event_data".into(), Some(bins)), Some(12)),
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_event_ack() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::EventAck(Value::Str("event_ack_data".into(), None), 42),
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_connect_error() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::ConnectError("connection_error".into()),
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_binary_event() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::BinaryEvent(Value::Str("binary_event_data".into(), None), None),
        };
        assert_serde_packet(packet);

        let mut bins = VecDeque::new();
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::BinaryEvent(Value::Str("event_data".into(), Some(bins)), Some(12)),
        };
        assert_serde_packet(packet);
    }

    #[test]
    fn packet_serde_binary_ack() {
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::BinaryAck(Value::Str("binary_ack_data".into(), None), 99),
        };
        assert_serde_packet(packet);

        let mut bins = VecDeque::new();
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        let packet = Packet {
            ns: "/".into(),
            inner: PacketData::BinaryAck(Value::Str("binary_ack_data".into(), Some(bins)), 99),
        };
        assert_serde_packet(packet);
    }
}
