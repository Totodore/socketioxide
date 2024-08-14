//! Socket.io packet implementation.
//! The [`Packet`] is the base unit of data that is sent over the engine.io socket.
//! It should not be used directly except when implementing the [`Adapter`](crate::adapter::Adapter) trait.
use std::borrow::Cow;

use crate::{parser::Value, ProtocolVersion};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use engineioxide::{sid::Sid, Str};

/// The socket.io packet type.
/// Each packet has a type and a namespace
#[derive(Debug, Clone, PartialEq)]
pub struct Packet<'a> {
    /// The packet data
    pub inner: PacketData<'a>,
    /// The namespace the packet belongs to
    pub ns: Str,
}

impl<'a> Packet<'a> {
    /// Send a connect packet with a default payload for v5 and no payload for v4
    pub fn connect(
        ns: impl Into<Str>,
        #[allow(unused_variables)] value: Value,
        #[allow(unused_variables)] protocol: ProtocolVersion,
    ) -> Self {
        #[cfg(not(feature = "v4"))]
        {
            Self::connect_v5(ns.into(), value)
        }

        #[cfg(feature = "v4")]
        {
            match protocol {
                ProtocolVersion::V4 => Self::connect_v4(ns.into()),
                ProtocolVersion::V5 => Self::connect_v5(ns.into(), value),
            }
        }
    }

    /// Sends a connect packet without payload.
    #[cfg(feature = "v4")]
    fn connect_v4(ns: Str) -> Self {
        Self {
            inner: PacketData::Connect(None),
            ns,
        }
    }

    /// Sends a connect packet with payload.
    fn connect_v5(ns: Str, value: Value) -> Self {
        Self {
            inner: PacketData::Connect(Some(value)),
            ns,
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

impl<'a> Packet<'a> {
    /// Create a connect error packet for the given namespace with a message
    pub fn connect_error(ns: impl Into<Str>, message: &str) -> Self {
        let message = serde_json::to_string(message).unwrap();
        let packet = format!(r#"{{"message":{}}}"#, message);
        Self {
            inner: PacketData::ConnectError(packet),
            ns: ns.into(),
        }
    }

    /// Create an event packet for the given namespace
    pub fn event(ns: impl Into<Str>, e: impl Into<Cow<'a, str>>, data: Value) -> Self {
        Self {
            inner: PacketData::Event(e.into(), data, None),
            ns: ns.into(),
        }
    }

    /// Create a binary event packet for the given namespace
    pub fn bin_event(
        ns: impl Into<Str>,
        e: impl Into<Cow<'a, str>>,
        data: Value,
        bin: Vec<Bytes>,
    ) -> Self {
        Self {
            inner: PacketData::BinaryEvent(e.into(), BinaryPacket::new(data, bin), None),
            ns: ns.into(),
        }
    }

    /// Create an ack packet for the given namespace
    pub fn ack(ns: impl Into<Str>, data: Value, ack: i64) -> Self {
        Self {
            inner: PacketData::EventAck(data, ack),
            ns: ns.into(),
        }
    }

    /// Create a binary ack packet for the given namespace
    pub fn bin_ack(ns: impl Into<Str>, data: Value, bin: Vec<Bytes>, ack: i64) -> Self {
        Self {
            inner: PacketData::BinaryAck(BinaryPacket::new(data, bin), ack),
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
pub enum PacketData<'a> {
    /// Connect packet with optional payload (only used with v5 for response)
    Connect(Option<Value>),
    /// Disconnect packet, used to disconnect from a namespace
    Disconnect,
    /// Event packet with optional ack id, to request an ack from the other side
    Event(Cow<'a, str>, Value, Option<i64>),
    /// Event ack packet, to acknowledge an event
    EventAck(Value, i64),
    /// Connect error packet, sent when the namespace is invalid
    ConnectError(String),
    /// Binary event packet with optional ack id, to request an ack from the other side
    BinaryEvent(Cow<'a, str>, BinaryPacket, Option<i64>),
    /// Binary ack packet, to acknowledge an event with binary data
    BinaryAck(BinaryPacket, i64),
}

/// Binary packet used when sending binary data
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryPacket {
    /// Data related to the packet
    pub data: Value,
    /// Binary payload
    pub bin: Vec<Bytes>,
    /// The number of expected payloads (used when receiving data)
    pub payload_count: usize,
}

impl<'a> PacketData<'a> {
    pub(crate) fn index(&self) -> usize {
        match self {
            PacketData::Connect(_) => 0,
            PacketData::Disconnect => 1,
            PacketData::Event(_, _, _) => 2,
            PacketData::EventAck(_, _) => 3,
            PacketData::ConnectError(_) => 4,
            PacketData::BinaryEvent(_, _, _) => 5,
            PacketData::BinaryAck(_, _) => 6,
        }
    }

    /// Set the ack id for the packet
    /// It will only set the ack id for the packets that support it
    pub(crate) fn set_ack_id(&mut self, ack_id: i64) {
        match self {
            PacketData::Event(_, _, ack) | PacketData::BinaryEvent(_, _, ack) => {
                *ack = Some(ack_id)
            }
            _ => {}
        };
    }

    /// Check if the packet is a binary packet (either binary event or binary ack)
    pub(crate) fn is_binary(&self) -> bool {
        matches!(
            self,
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _)
        )
    }

    /// Check if the binary packet is complete, it means that all payloads have been received
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            PacketData::BinaryEvent(_, bin, _) | PacketData::BinaryAck(bin, _) => {
                bin.payload_count == bin.bin.len()
            }
            _ => true,
        }
    }
}

impl BinaryPacket {
    /// Create a new binary packet.
    pub fn new(data: Value, bin: Vec<Bytes>) -> BinaryPacket {
        let payload_count = bin.len();
        BinaryPacket {
            data,
            bin,
            payload_count,
        }
    }
    /// Add a payload to the binary packet, when all payloads are added,
    /// the packet is complete and can be further processed
    pub fn add_payload<B: Into<Bytes>>(&mut self, payload: B) {
        self.bin.push(payload.into());
    }
    /// Check if the binary packet is complete, it means that all payloads have been received
    pub fn is_complete(&self) -> bool {
        self.payload_count == self.bin.len()
    }
}

/// Connect packet sent by the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectPacket {
    pub(crate) sid: Sid,
}
