use std::fmt;

use base64::{Engine, engine::general_purpose};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{Sid, Str};

/// A Packet type to use when receiving and sending data from the client
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Packet {
    /// Open packet used to initiate a connection
    Open(OpenPacket),
    /// Close packet used to close a connection
    Close,
    /// Ping packet used to check if the connection is still alive
    /// The client never sends this packet, it is only used by the server
    Ping,
    /// Pong packet used to respond to a Ping packet
    /// The server never sends this packet, it is only used by the client
    Pong,

    /// Special Ping packet used to initiate a connection
    PingUpgrade,
    /// Special Pong packet used to respond to a PingUpgrade packet and upgrade the connection
    PongUpgrade,

    /// Message packet used to send a message to the client
    Message(Str),
    /// Upgrade packet to upgrade the connection from polling to websocket
    Upgrade,

    /// Noop packet used to send something to a opened polling connection so it gracefully closes to allow the client to upgrade to websocket
    Noop,

    /// Binary packet used to send binary data to the client
    /// Converts to a String using base64 encoding when using polling connection
    /// Or to a websocket binary frame when using websocket connection
    ///
    /// When receiving, it is only used with polling connection, websocket use binary frame
    Binary(Bytes), // Not part of the protocol, used internally

    /// Binary packet used to send binary data to the client
    /// Converts to a String using base64 encoding when using polling connection
    /// Or to a websocket binary frame when using websocket connection
    ///
    /// When receiving, it is only used with polling connection, websocket use binary frame
    ///
    /// This is a special packet, excepionally specific to the V3 protocol.
    BinaryV3(Bytes), // Not part of the protocol, used internally
}

/// An error that occurs when parsing a packet.
#[derive(Debug)]
pub enum PacketParseError {
    /// Invalid connect packet
    InvalidConnectPacket(serde_json::Error),
    /// The packet type is invalid.
    InvalidPacketType(Option<char>),
    /// The packet payload is invalid.
    InvalidPacketPayload,
    /// The packet length is invalid.
    InvalidPacketLen,
    /// The packet chunk is invalid
    InvalidUtf8Boundary(std::str::Utf8Error),
    /// The base64 decoding failed.
    Base64Decode(base64::DecodeError),
    /// The payload is too large.
    PayloadTooLarge {
        /// The maximum allowed payload size.
        max: u64,
    },
}
impl fmt::Display for PacketParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketParseError::InvalidConnectPacket(e) => write!(f, "invalid connect packet: {e}"),
            PacketParseError::InvalidPacketType(c) => write!(f, "invalid packet type: {c:?}"),
            PacketParseError::InvalidPacketPayload => write!(f, "invalid packet payload"),
            PacketParseError::InvalidPacketLen => write!(f, "invalid packet length"),
            PacketParseError::InvalidUtf8Boundary(err) => write!(
                f,
                "invalid utf8 boundary when parsing payload into packet chunks: {err}"
            ),
            PacketParseError::Base64Decode(err) => write!(f, "base64 decode error: {err}"),
            PacketParseError::PayloadTooLarge { max } => {
                write!(f, "payload too large: max {max}")
            }
        }
    }
}
impl From<base64::DecodeError> for PacketParseError {
    fn from(err: base64::DecodeError) -> Self {
        PacketParseError::Base64Decode(err)
    }
}
impl From<std::string::FromUtf8Error> for PacketParseError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        PacketParseError::InvalidUtf8Boundary(err.utf8_error())
    }
}
impl From<std::str::Utf8Error> for PacketParseError {
    fn from(err: std::str::Utf8Error) -> Self {
        PacketParseError::InvalidUtf8Boundary(err)
    }
}
impl From<serde_json::Error> for PacketParseError {
    fn from(err: serde_json::Error) -> Self {
        PacketParseError::InvalidConnectPacket(err)
    }
}
impl std::error::Error for PacketParseError {}

impl Packet {
    /// Check if the packet is a binary packet
    pub fn is_binary(&self) -> bool {
        matches!(self, Packet::Binary(_) | Packet::BinaryV3(_))
    }

    /// If the packet is a message packet (text), it returns the message
    pub fn into_message(self) -> Str {
        match self {
            Packet::Message(msg) => msg,
            _ => panic!("Packet is not a message"),
        }
    }

    /// If the packet is a binary packet, it returns the binary data
    pub fn into_binary(self) -> Bytes {
        match self {
            Packet::Binary(data) => data,
            Packet::BinaryV3(data) => data,
            _ => panic!("Packet is not a binary"),
        }
    }

    /// Get the max size the packet could have when serialized
    ///
    ///  If b64 is true, it returns the max size when serialized to base64
    ///
    /// The base64 max size factor is `ceil(n / 3) * 4`
    pub fn get_size_hint(&self, b64: bool) -> usize {
        match self {
            Packet::Open(_) => 156, // max possible size for the open packet serialized
            Packet::Close => 1,
            Packet::Ping => 1,
            Packet::Pong => 1,
            Packet::PingUpgrade => 6,
            Packet::PongUpgrade => 6,
            Packet::Message(msg) => 1 + msg.len(),
            Packet::Upgrade => 1,
            Packet::Noop => 1,
            Packet::Binary(data) => {
                if b64 {
                    1 + base64::encoded_len(data.len(), true).unwrap_or(usize::MAX - 1)
                } else {
                    1 + data.len()
                }
            }
            Packet::BinaryV3(data) => {
                if b64 {
                    2 + base64::encoded_len(data.len(), true).unwrap_or(usize::MAX - 2)
                } else {
                    1 + data.len()
                }
            }
        }
    }
}

impl From<Packet> for Bytes {
    fn from(value: Packet) -> Self {
        String::from(value).into()
    }
}

/// Serialize a [Packet] to a [String] according to the Engine.IO protocol
impl From<Packet> for String {
    fn from(packet: Packet) -> String {
        let len = packet.get_size_hint(true);
        let mut buffer = String::with_capacity(len);
        match packet {
            Packet::Open(open) => {
                buffer.push('0');
                buffer.push_str(&serde_json::to_string(&open).unwrap());
            }
            Packet::Close => buffer.push('1'),
            Packet::Ping => buffer.push('2'),
            Packet::Pong => buffer.push('3'),
            Packet::PingUpgrade => buffer.push_str("2probe"),
            Packet::PongUpgrade => buffer.push_str("3probe"),
            Packet::Message(msg) => {
                buffer.push('4');
                buffer.push_str(&msg);
            }
            Packet::Upgrade => buffer.push('5'),
            Packet::Noop => buffer.push('6'),
            Packet::Binary(data) => {
                buffer.push('b');
                general_purpose::STANDARD.encode_string(data, &mut buffer);
            }
            Packet::BinaryV3(data) => {
                buffer.push_str("b4");
                general_purpose::STANDARD.encode_string(data, &mut buffer);
            }
        };
        buffer
    }
}
<<<<<<< HEAD:crates/engineioxide-core/src/packet.rs

=======
impl From<Packet> for tokio_tungstenite::tungstenite::Utf8Bytes {
    fn from(value: Packet) -> Self {
        String::from(value).into()
    }
}
impl From<Packet> for Bytes {
    fn from(value: Packet) -> Self {
        String::from(value).into()
    }
}
>>>>>>> origin/main:crates/engineioxide/src/packet.rs
/// Deserialize a [Packet] from a [String] according to the Engine.IO protocol
impl TryFrom<Str> for Packet {
    type Error = PacketParseError;
    fn try_from(value: Str) -> Result<Self, Self::Error> {
        let packet_type = value
            .as_bytes()
            .first()
            .ok_or(PacketParseError::InvalidPacketType(None))?;
        let is_upgrade = value.len() == 6 && &value[1..6] == "probe";
        let res = match packet_type {
            b'0' => Packet::Open(serde_json::from_str(value.slice(1..).as_str())?),
            b'1' => Packet::Close,
            b'2' if is_upgrade => Packet::PingUpgrade,
            b'2' => Packet::Ping,
            b'3' if is_upgrade => Packet::PongUpgrade,
            b'3' => Packet::Pong,
            b'4' => Packet::Message(value.slice(1..)),
            b'5' => Packet::Upgrade,
            b'6' => Packet::Noop,
            b'b' if value.as_bytes().get(1) == Some(&b'4') => Packet::BinaryV3(
                general_purpose::STANDARD
                    .decode(value.slice(2..).as_bytes())?
                    .into(),
            ),
            b'b' => Packet::Binary(
                general_purpose::STANDARD
                    .decode(value.slice(1..).as_bytes())?
                    .into(),
            ),
            c => Err(PacketParseError::InvalidPacketType(Some(*c as char)))?,
        };
        Ok(res)
    }
}

impl TryFrom<String> for Packet {
    type Error = PacketParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Packet::try_from(Str::from(value))
    }
}

/// An OpenPacket is used to initiate a connection
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket {
    /// The session ID.
    pub sid: Sid,
    /// The list of available transport upgrades.
    pub upgrades: Vec<String>,
    /// The ping interval, used in the heartbeat mechanism (in milliseconds).
    pub ping_interval: u64,
    /// The ping timeout, used in the heartbeat mechanism (in milliseconds).
    pub ping_timeout: u64,
    /// The maximum number of bytes per chunk, used by the client to
    /// aggregate packets into payloads.
    pub max_payload: u64,
}

/// This default implementation should only be used for testing purposes.
impl Default for OpenPacket {
    fn default() -> Self {
        Self {
            sid: Sid::ZERO,
            upgrades: vec!["websocket".to_string()],
            ping_interval: 25000,
            ping_timeout: 20000,
            max_payload: 100000,
        }
    }
}

/// Buffered packets to send to the client.
/// It is used to ensure atomicity when sending multiple packets to the client.
///
/// The [`PacketBuf`] stack size will impact the dynamically allocated buffer
/// of the internal mpsc channel.
pub type PacketBuf = SmallVec<[Packet; 2]>;

#[cfg(test)]
mod tests {

    use super::*;
    use std::{convert::TryInto, time::Duration};

    #[test]
    fn test_open_packet() {
        let sid = Sid::new();
        let packet = Packet::Open(OpenPacket {
            sid,
            upgrades: vec!["websocket".to_string()],
            ping_interval: Duration::from_millis(25000).as_millis() as u64,
            ping_timeout: Duration::from_millis(20000).as_millis() as u64,
            max_payload: 100000,
        });
        let packet_str: String = packet.into();
        assert_eq!(
            packet_str,
            format!(
                "0{{\"sid\":\"{sid}\",\"upgrades\":[\"websocket\"],\"pingInterval\":25000,\"pingTimeout\":20000,\"maxPayload\":100000}}"
            )
        );
    }

    #[test]
    fn test_open_packet_deserialize() {
        let sid = Sid::new();
        let ref_packet = OpenPacket {
            sid,
            upgrades: vec!["websocket".to_string()],
            ping_interval: Duration::from_millis(25000).as_millis() as u64,
            ping_timeout: Duration::from_millis(20000).as_millis() as u64,
            max_payload: 100000,
        };
        let packet_str = format!(
            "0{{\"sid\":\"{sid}\",\"upgrades\":[\"websocket\"],\"pingInterval\":25000,\"pingTimeout\":20000,\"maxPayload\":100000}}"
        );
        let packet = Packet::try_from(packet_str).unwrap();
        assert!(matches!(packet, Packet::Open(p) if p == ref_packet));
    }

    #[test]
    fn test_message_packet() {
        let packet = Packet::Message("hello".into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "4hello");
    }

    #[test]
    fn test_message_packet_deserialize() {
        let packet_str = "4hello".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::Message("hello".into()));
    }

    #[test]
    fn test_binary_packet() {
        let packet = Packet::Binary(vec![1, 2, 3].into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "bAQID");
    }

    #[test]
    fn test_binary_packet_deserialize() {
        let packet_str = "bAQID".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::Binary(vec![1, 2, 3].into()));
    }

    #[test]
    fn test_binary_packet_v3() {
        let packet = Packet::BinaryV3(vec![1, 2, 3].into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "b4AQID");
    }

    #[test]
    fn test_binary_packet_v3_deserialize() {
        let packet_str = "b4AQID".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::BinaryV3(vec![1, 2, 3].into()));
    }

    #[test]
    fn test_packet_get_size_hint() {
        // Max serialized packet
        let open = OpenPacket {
            sid: Sid::new(),
            ping_interval: u64::MAX,
            ping_timeout: u64::MAX,
            max_payload: u64::MAX,
            upgrades: vec!["websocket".to_string()],
        };
        let size = serde_json::to_string(&open).unwrap().len();
        let packet = Packet::Open(open);
        assert_eq!(packet.get_size_hint(false), size);

        let packet = Packet::Close;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Ping;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Pong;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::PingUpgrade;
        assert_eq!(packet.get_size_hint(false), 6);

        let packet = Packet::PongUpgrade;
        assert_eq!(packet.get_size_hint(false), 6);

        let packet = Packet::Message("hello".into());
        assert_eq!(packet.get_size_hint(false), 6);

        let packet = Packet::Upgrade;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Noop;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Binary(vec![1, 2, 3].into());
        assert_eq!(packet.get_size_hint(false), 4);
        assert_eq!(packet.get_size_hint(true), 5);

        let packet = Packet::BinaryV3(vec![1, 2, 3].into());
        assert_eq!(packet.get_size_hint(false), 4);
        assert_eq!(packet.get_size_hint(true), 6);
    }
}
