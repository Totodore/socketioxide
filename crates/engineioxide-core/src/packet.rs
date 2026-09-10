use std::{fmt, time::Duration};

use base64::{Engine, engine::general_purpose};
use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smallvec::SmallVec;

use crate::{ProtocolVersion, Sid, Str, TransportType};

/// A Packet type to use when receiving and sending data from the client
#[derive(Debug, Clone, PartialEq)]
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
impl From<Packet> for Str {
    fn from(value: Packet) -> Self {
        Str::from(String::from(value))
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

/// Deserialize a [Packet] from a [String] according to the Engine.IO protocol
impl Packet {
    /// Parses a packet from a string value using the specified protocol version.
    pub fn parse(
        protocol: ProtocolVersion,
        value: impl Into<Str>,
    ) -> Result<Self, PacketParseError> {
        let value = value.into();
        let packet_type = value
            .as_bytes()
            .first()
            .ok_or(PacketParseError::InvalidPacketType(None))?;
        let is_upgrade = value.len() == 6 && &value[1..6] == "probe";
        let res = match packet_type {
            b'0' => Packet::Open(serde_json::from_slice(&value.as_bytes()[1..])?),
            b'1' => Packet::Close,
            b'2' if is_upgrade => Packet::PingUpgrade,
            b'2' => Packet::Ping,
            b'3' if is_upgrade => Packet::PongUpgrade,
            b'3' => Packet::Pong,
            b'4' => Packet::Message(value.slice(1..)),
            b'5' => Packet::Upgrade,
            b'6' => Packet::Noop,
            b'b' if protocol == ProtocolVersion::V3 => Packet::BinaryV3(
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

/// An OpenPacket is used to initiate a connection
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket {
    /// The session ID.
    pub sid: Sid,

    /// The list of available transport upgrades.
    pub upgrades: SmallVec<[TransportType; 1]>,

    /// The ping interval, used in the heartbeat mechanism.
    #[serde(
        serialize_with = "serialize_duration_millis",
        deserialize_with = "deserialize_duration_from_millis"
    )]
    pub ping_interval: Duration,

    /// The ping timeout, used in the heartbeat mechanism.
    #[serde(
        serialize_with = "serialize_duration_millis",
        deserialize_with = "deserialize_duration_from_millis"
    )]
    pub ping_timeout: Duration,

    /// The maximum number of bytes per chunk, used by the client to
    /// aggregate packets into payloads.
    pub max_payload: u64,
}

/// Helper to serialize a duration as milliseconds
pub fn serialize_duration_millis<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // serialize_u128 is not supported so we need to cast it to u64, see https://github.com/serde-rs/json/issues/846
    serializer.serialize_u64(duration.as_millis() as u64)
}

/// Helper to deserialize a duration from milliseconds
pub fn deserialize_duration_from_millis<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let millis = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(millis))
}

/// This default implementation should only be used for testing purposes.
impl Default for OpenPacket {
    fn default() -> Self {
        Self {
            sid: Sid::ZERO,
            upgrades: smallvec::smallvec![TransportType::Websocket],
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
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
    use std::time::Duration;

    #[test]
    fn test_open_packet() {
        let sid = Sid::new();
        let packet = Packet::Open(OpenPacket {
            sid,
            upgrades: smallvec::smallvec![TransportType::Websocket],
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
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
    fn test_message_packet() {
        let packet = Packet::Message("hello".into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "4hello");
    }

    #[test]
    fn test_binary_packet() {
        let packet = Packet::Binary(vec![1, 2, 3].into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "bAQID");
    }

    #[test]
    fn test_binary_packet_v4_deserialize_payload_starting_with_4() {
        let data = vec![0xE0, 0xE1, 0xE2];
        // Sanity check: the base64 encoding indeed starts with '4'.
        let packet_str: String = Packet::Binary(data.clone().into()).into();
        assert_eq!(packet_str, "b4OHi");

        let packet = Packet::parse(ProtocolVersion::V4, packet_str).unwrap();
        assert_eq!(packet, Packet::Binary(data.into()));
    }

    #[test]
    fn test_binary_packet_v3() {
        let packet = Packet::BinaryV3(vec![1, 2, 3].into());
        let packet_str: String = packet.into();
        assert_eq!(packet_str, "b4AQID");
    }

    #[test]
    fn test_packet_get_size_hint() {
        // Max serialized packet
        let open = OpenPacket {
            sid: Sid::new(),
            ping_interval: Duration::MAX,
            ping_timeout: Duration::MAX,
            max_payload: u64::MAX,
            upgrades: smallvec::smallvec![TransportType::Websocket],
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
