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

#[cfg(test)]
mod tests {

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
}
