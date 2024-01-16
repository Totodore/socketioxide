//! Socket.io packet implementation.
//! The [`Packet`] is the base unit of data that is sent over the engine.io socket.
//! It should not be used directly except when implementing the [`Adapter`](crate::adapter::Adapter) trait.
use std::borrow::Cow;

use crate::ProtocolVersion;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::Error;
use engineioxide::sid::Sid;

/// The socket.io packet type.
/// Each packet has a type and a namespace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet<'a> {
    /// The packet data
    pub inner: PacketData<'a>,
    /// The namespace the packet belongs to
    pub ns: Cow<'a, str>,
}

impl<'a> Packet<'a> {
    /// Send a connect packet with a default payload for v5 and no payload for v4
    pub fn connect(
        ns: &'a str,
        #[allow(unused_variables)] sid: Sid,
        #[allow(unused_variables)] protocol: ProtocolVersion,
    ) -> Self {
        #[cfg(not(feature = "v4"))]
        {
            Self::connect_v5(ns, sid)
        }

        #[cfg(feature = "v4")]
        {
            match protocol {
                ProtocolVersion::V4 => Self::connect_v4(ns),
                ProtocolVersion::V5 => Self::connect_v5(ns, sid),
            }
        }
    }

    /// Sends a connect packet without payload.
    #[cfg(feature = "v4")]
    fn connect_v4(ns: &'a str) -> Self {
        Self {
            inner: PacketData::Connect(None),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Sends a connect packet with payload.
    fn connect_v5(ns: &'a str, sid: Sid) -> Self {
        let val = serde_json::to_string(&ConnectPacket { sid }).unwrap();
        Self {
            inner: PacketData::Connect(Some(val)),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create a disconnect packet for the given namespace
    pub fn disconnect(ns: &'a str) -> Self {
        Self {
            inner: PacketData::Disconnect,
            ns: Cow::Borrowed(ns),
        }
    }
}

impl<'a> Packet<'a> {
    /// Create a connect error packet for the given namespace
    pub fn invalid_namespace(ns: &'a str) -> Self {
        Self {
            inner: PacketData::ConnectError,
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create an event packet for the given namespace
    pub fn event(ns: impl Into<Cow<'a, str>>, e: impl Into<Cow<'a, str>>, data: Value) -> Self {
        Self {
            inner: PacketData::Event(e.into(), data, None),
            ns: ns.into(),
        }
    }

    /// Create a binary event packet for the given namespace
    pub fn bin_event(
        ns: impl Into<Cow<'a, str>>,
        e: impl Into<Cow<'a, str>>,
        data: Value,
        bin: Vec<Vec<u8>>,
    ) -> Self {
        debug_assert!(!bin.is_empty());

        let packet = BinaryPacket::outgoing(data, bin);
        Self {
            inner: PacketData::BinaryEvent(e.into(), packet, None),
            ns: ns.into(),
        }
    }

    /// Create an ack packet for the given namespace
    pub fn ack(ns: &'a str, data: Value, ack: i64) -> Self {
        Self {
            inner: PacketData::EventAck(data, ack),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create a binary ack packet for the given namespace
    pub fn bin_ack(ns: &'a str, data: Value, bin: Vec<Vec<u8>>, ack: i64) -> Self {
        debug_assert!(!bin.is_empty());
        let packet = BinaryPacket::outgoing(data, bin);
        Self {
            inner: PacketData::BinaryAck(packet, ack),
            ns: Cow::Borrowed(ns),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketData<'a> {
    /// Connect packet with optional payload (only used with v5 for response)
    Connect(Option<String>),
    /// Disconnect packet, used to disconnect from a namespace
    Disconnect,
    /// Event packet with optional ack id, to request an ack from the other side
    Event(Cow<'a, str>, Value, Option<i64>),
    /// Event ack packet, to acknowledge an event
    EventAck(Value, i64),
    /// Connect error packet, sent when the namespace is invalid
    ConnectError,
    /// Binary event packet with optional ack id, to request an ack from the other side
    BinaryEvent(Cow<'a, str>, BinaryPacket, Option<i64>),
    /// Binary ack packet, to acknowledge an event with binary data
    BinaryAck(BinaryPacket, i64),
}

/// Binary packet used when sending binary data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPacket {
    /// Data related to the packet
    pub data: Value,
    /// Binary payload
    pub bin: Vec<Vec<u8>>,
    /// The number of expected payloads (used when receiving data)
    /* todo: remove pub */ pub payload_count: usize,
}

impl<'a> PacketData<'a> {
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
}

impl BinaryPacket {
    /// Create a binary packet from incoming data and remove all placeholders and get the payload count
    pub fn incoming(mut data: Value) -> Self {
        let payload_count = match &mut data {
            Value::Array(ref mut v) => {
                let count = v.len();
                v.retain(|v| v.as_object().and_then(|o| o.get("_placeholder")).is_none());
                count - v.len()
            }
            val => {
                if val
                    .as_object()
                    .and_then(|o| o.get("_placeholder"))
                    .is_some()
                {
                    data = Value::Array(vec![]);
                    1
                } else {
                    0
                }
            }
        };

        Self {
            data,
            bin: Vec::new(),
            payload_count,
        }
    }

    /// Create a binary packet from outgoing data and a payload
    pub fn outgoing(data: Value, bin: Vec<Vec<u8>>) -> Self {
        let mut data = match data {
            Value::Array(v) => Value::Array(v),
            d => Value::Array(vec![d]),
        };
        let payload_count = bin.len();
        (0..payload_count).for_each(|i| {
            data.as_array_mut().unwrap().push(json!({
                "_placeholder": true,
                "num": i
            }))
        });
        Self {
            data,
            bin,
            payload_count,
        }
    }

    /// Add a payload to the binary packet, when all payloads are added,
    /// the packet is complete and can be further processed
    pub fn add_payload(&mut self, payload: Vec<u8>) {
        self.bin.push(payload);
    }
    /// Check if the binary packet is complete, it means that all payloads have been received
    pub fn is_complete(&self) -> bool {
        self.payload_count == self.bin.len()
    }
}

impl<'a> From<Packet<'a>> for String {
    fn from(mut packet: Packet<'a>) -> String {
        use PacketData::*;

        "".to_string()
    }
}

/// Connect packet sent by the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectPacket {
    pub sid: Sid,
}
