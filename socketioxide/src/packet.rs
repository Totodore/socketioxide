use crate::ProtocolVersion;
use itertools::{Itertools, PeekingNext};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

use crate::errors::Error;
use engineioxide::sid_generator::Sid;
use engineioxide::SendPacket as EnginePacket;

/// The socket.io packet type.
/// Each packet has a type and a namespace
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub inner: PacketData,
    pub ns: String,
}

impl Packet {
    /// Send a connect packet with a default payload for v5 and no payload for v4
    pub fn connect(
        ns: String,
        #[allow(unused_variables)] sid: Sid,
        #[allow(unused_variables)] protocol: ProtocolVersion,
    ) -> Self {
        #[cfg(all(feature = "v5", not(feature = "v4")))]
        {
            Self::connect_v5(ns, sid)
        }

        #[cfg(all(feature = "v4", not(feature = "v5")))]
        {
            Self::connect_v4(ns)
        }

        #[cfg(all(feature = "v5", feature = "v4"))]
        {
            match protocol {
                ProtocolVersion::V4 => Self::connect_v4(ns),
                ProtocolVersion::V5 => Self::connect_v5(ns, sid),
            }
        }
    }

    /// Sends a connect packet without payload.
    #[cfg(feature = "v4")]
    fn connect_v4(ns: String) -> Self {
        Self {
            inner: PacketData::Connect(None),
            ns,
        }
    }

    /// Sends a connect packet with payload.
    #[cfg(feature = "v5")]
    fn connect_v5(ns: String, sid: Sid) -> Self {
        let val = serde_json::to_value(ConnectPacket {
            sid: sid.to_string(),
        })
        .unwrap();
        Self {
            inner: PacketData::Connect(Some(val)),
            ns,
        }
    }

    pub fn disconnect(ns: String) -> Self {
        Self {
            inner: PacketData::Disconnect,
            ns,
        }
    }
}

impl Packet {
    pub fn invalid_namespace(ns: String) -> Self {
        Self {
            inner: PacketData::ConnectError(ConnectErrorPacket {
                message: "Invalid namespace".to_string(),
            }),
            ns,
        }
    }

    pub fn event(ns: String, e: String, data: Value) -> Self {
        Self {
            inner: PacketData::Event(e, data, None),
            ns,
        }
    }

    pub fn bin_event(ns: String, e: String, data: Value, bin: Vec<Vec<u8>>) -> Self {
        debug_assert!(!bin.is_empty());

        let packet = BinaryPacket::outgoing(data, bin);
        Self {
            inner: PacketData::BinaryEvent(e, packet, None),
            ns,
        }
    }

    pub fn ack(ns: String, data: Value, ack: i64) -> Self {
        Self {
            inner: PacketData::EventAck(data, ack),
            ns,
        }
    }
    pub fn bin_ack(ns: String, data: Value, bin: Vec<Vec<u8>>, ack: i64) -> Self {
        debug_assert!(!bin.is_empty());
        let packet = BinaryPacket::outgoing(data, bin);
        Self {
            inner: PacketData::BinaryAck(packet, ack),
            ns,
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
    Connect(Option<Value>),
    Disconnect,
    Event(String, Value, Option<i64>),
    EventAck(Value, i64),
    ConnectError(ConnectErrorPacket),
    BinaryEvent(String, BinaryPacket, Option<i64>),
    BinaryAck(BinaryPacket, i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryPacket {
    pub data: Value,
    pub bin: Vec<Vec<u8>>,
    payload_count: usize,
}

impl PacketData {
    fn index(&self) -> u8 {
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
    pub fn add_payload(&mut self, payload: Vec<u8>) {
        self.bin.push(payload);
    }
    pub fn is_complete(&self) -> bool {
        self.payload_count == self.bin.len()
    }
}

impl TryInto<String> for Packet {
    type Error = serde_json::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        let mut res = self.inner.index().to_string();
        if !self.ns.is_empty() && self.ns != "/" {
            res.push_str(&format!("{},", self.ns));
        }

        match self.inner {
            PacketData::Connect(data) => {
                if let Some(payload) = data {
                    res.push_str(&serde_json::to_string(&payload)?);
                }
            }
            PacketData::Disconnect => (),
            PacketData::Event(event, data, ack) => {
                if let Some(ack) = ack {
                    res.push_str(&ack.to_string());
                }
                // Expand the packet if it is an array -> ["event", ...data]
                let packet = match data {
                    Value::Array(mut v) => {
                        v.insert(0, Value::String(event));
                        serde_json::to_string(&v)?
                    }
                    _ => serde_json::to_string(&(event, data))?,
                };
                res.push_str(&packet)
            }
            PacketData::EventAck(data, ack) => {
                res.push_str(&ack.to_string());
                // Enforce that the packet is an array -> [data]
                let packet = match data {
                    Value::Array(_) => data,
                    Value::Null => Value::Array(vec![]),
                    _ => Value::Array(vec![data]),
                };
                let packet = serde_json::to_string(&packet)?;
                res.push_str(&packet)
            }
            PacketData::ConnectError(data) => res.push_str(&serde_json::to_string(&data)?),
            PacketData::BinaryEvent(event, bin, ack) => {
                res.push_str(&bin.payload_count.to_string());
                res.push('-');

                if let Some(ack) = ack {
                    res.push_str(&ack.to_string());
                }
                // Expand the packet if it is an array -> ["event", ...data]
                let array = match bin.data {
                    Value::Array(mut v) => {
                        v.insert(0, Value::String(event));
                        v
                    }
                    _ => vec![Value::String(event), bin.data],
                };

                let packet = serde_json::to_string(&array)?;
                res.push_str(&packet)
            }
            PacketData::BinaryAck(packet, ack) => {
                res.push_str(&packet.payload_count.to_string());
                res.push('-');
                res.push_str(&ack.to_string());
                // Enforce that the packet is an array -> [data]
                let data = match packet.data {
                    Value::Array(_) => packet.data,
                    Value::Null => Value::Array(vec![]),
                    _ => Value::Array(vec![packet.data]),
                };
                let packet = serde_json::to_string(&data)?;
                res.push_str(&packet)
            }
        };
        Ok(res)
    }
}

/// Deserialize an event packet from a string, formated as:
/// ```text
/// ["<event name>", ...<JSON-stringified payload without binary>]
/// ```
fn deserialize_event_packet(data: &str) -> Result<(String, Value), Error> {
    debug!("Deserializing event packet: {:?}", data);
    let packet = match serde_json::from_str::<Value>(data)? {
        Value::Array(packet) => packet,
        _ => return Err(Error::InvalidEventName),
    };

    let event = packet
        .get(0)
        .ok_or(Error::InvalidEventName)?
        .as_str()
        .ok_or(Error::InvalidEventName)?
        .to_string();
    let payload = Value::from_iter(packet.into_iter().skip(1));
    Ok((event, payload))
}

fn deserialize_packet<T: DeserializeOwned>(data: &str) -> Result<Option<T>, Error> {
    debug!("Deserializing packet: {:?}", data);
    let packet = if data.is_empty() {
        None
    } else {
        Some(serde_json::from_str(data)?)
    };
    Ok(packet)
}

/// Deserialize a packet from a string
/// The string should be in the format of:
/// ```text
/// <packet type>[<# of binary attachments>-][<namespace>,][<acknowledgment id>][JSON-stringified payload without binary]
/// + binary attachments extracted
/// ```
impl TryFrom<String> for Packet {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        let index = chars.next().ok_or(Error::InvalidPacketType)?;

        let attachments: u8 = if index == '5' || index == '6' {
            chars
                .take_while_ref(|c| *c != '-')
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        } else {
            0
        };

        // If there are attachments, skip the `-` separator
        chars.peeking_next(|c| attachments > 0 && !c.is_ascii_digit());

        let mut ns: String = chars
            .take_while_ref(|c| *c != ',' && *c != '{' && *c != '[' && !c.is_ascii_digit())
            .collect();

        // If there is a namespace, skip the `,` separator
        if !ns.is_empty() {
            chars.next();
        }
        if !ns.starts_with('/') {
            ns.insert(0, '/');
        }

        let ack: Option<i64> = chars
            .take_while_ref(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok();

        let data = chars.as_str();
        let inner = match index {
            '0' => PacketData::Connect(deserialize_packet(data)?.unwrap_or_default()),
            '1' => PacketData::Disconnect,
            '2' => {
                let (event, payload) = deserialize_event_packet(data)?;
                PacketData::Event(event, payload, ack)
            }
            '3' => {
                let packet = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::EventAck(packet, ack.ok_or(Error::InvalidPacketType)?)
            }
            '4' => {
                let payload = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::ConnectError(payload)
            }
            '5' => {
                let (event, payload) = deserialize_event_packet(data)?;
                PacketData::BinaryEvent(event, BinaryPacket::incoming(payload), ack)
            }
            '6' => {
                let packet = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::BinaryAck(
                    BinaryPacket::incoming(packet),
                    ack.ok_or(Error::InvalidPacketType)?,
                )
            }
            _ => return Err(Error::InvalidPacketType),
        };

        Ok(Self { inner, ns })
    }
}

impl TryInto<EnginePacket> for Packet {
    type Error = serde_json::Error;
    fn try_into(self) -> Result<EnginePacket, Self::Error> {
        Ok(EnginePacket::Message(self.try_into()?))
    }
}
/// Connect packet sent by the client
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectPacket {
    sid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectErrorPacket {
    message: String,
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    #[test]
    /// This test suite is taken from the explanation section here:
    /// https://github.com/socketio/socket.io-protocol
    fn test_decode() {
        let payload = "0{\"token\":\"123\"}".to_string();
        let packet = Packet::try_from(payload);
        assert!(packet.is_ok());

        assert_eq!(
            Packet {
                ns: "/".to_string(),
                inner: PacketData::Connect(Some(json!({ "token": "123"})))
            },
            packet.unwrap()
        );

        let payload = "{\"token™\":\"123\"}".to_string();
        let utf8_payload = format!("0/admin™,{}", payload);
        let packet = Packet::try_from(utf8_payload);
        assert!(packet.is_ok());

        assert_eq!(
            Packet {
                ns: "/admin™".to_owned(),
                inner: PacketData::Connect(Some(json!({ "token™": "123" })))
            },
            packet.unwrap()
        );
    }
}
