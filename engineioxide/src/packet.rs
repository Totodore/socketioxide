use base64::{engine::general_purpose, Engine};
use serde::{de::Error, Deserialize, Serialize};

use crate::config::EngineIoConfig;
use crate::sid::Sid;
use crate::TransportType;

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
    Message(String),
    /// Upgrade packet to upgrade the connection from polling to websocket
    Upgrade,

    /// Noop packet used to send something to a opened polling connection so it gracefully closes to allow the client to upgrade to websocket
    Noop,

    /// Binary packet used to send binary data to the client
    /// Converts to a String using base64 encoding when using polling connection
    /// Or to a websocket binary frame when using websocket connection
    ///
    /// When receiving, it is only used with polling connection, websocket use binary frame
    Binary(Vec<u8>), // Not part of the protocol, used internally

    /// Binary packet used to send binary data to the client
    /// Converts to a String using base64 encoding when using polling connection
    /// Or to a websocket binary frame when using websocket connection
    ///
    /// When receiving, it is only used with polling connection, websocket use binary frame
    ///
    /// This is a special packet, excepionally specific to the V3 protocol.
    BinaryV3(Vec<u8>), // Not part of the protocol, used internally
}

impl Packet {
    /// Check if the packet is a binary packet
    pub fn is_binary(&self) -> bool {
        matches!(self, Packet::Binary(_) | Packet::BinaryV3(_))
    }

    /// If the packet is a message packet (text), it returns the message
    pub(crate) fn into_message(self) -> String {
        match self {
            Packet::Message(msg) => msg,
            _ => panic!("Packet is not a message"),
        }
    }

    /// If the packet is a binary packet, it returns the binary data
    pub(crate) fn into_binary(self) -> Vec<u8> {
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
    pub(crate) fn get_size_hint(&self, b64: bool) -> usize {
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
                    1 + ((data.len() as f64) / 3.).ceil() as usize * 4
                } else {
                    1 + data.len()
                }
            }
            Packet::BinaryV3(data) => {
                if b64 {
                    2 + ((data.len() as f64) / 3.).ceil() as usize * 4
                } else {
                    1 + data.len()
                }
            }
        }
    }
}

/// Serialize a [Packet] to a [String] according to the Engine.IO protocol
impl TryInto<String> for Packet {
    type Error = crate::errors::Error;
    fn try_into(self) -> Result<String, Self::Error> {
        let res = match self {
            Packet::Open(open) => {
                "0".to_string() + &serde_json::to_string(&open).map_err(Self::Error::from)?
            }
            Packet::Close => "1".to_string(),
            Packet::Ping => "2".to_string(),
            Packet::Pong => "3".to_string(),
            Packet::PingUpgrade => "2probe".to_string(),
            Packet::PongUpgrade => "3probe".to_string(),
            Packet::Message(msg) => "4".to_string() + &msg,
            Packet::Upgrade => "5".to_string(),
            Packet::Noop => "6".to_string(),
            Packet::Binary(data) => "b".to_string() + &general_purpose::STANDARD.encode(data),
            Packet::BinaryV3(data) => "b4".to_string() + &general_purpose::STANDARD.encode(data),
        };
        Ok(res)
    }
}
/// Deserialize a [Packet] from a [String] according to the Engine.IO protocol
impl TryFrom<&str> for Packet {
    type Error = crate::errors::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        let packet_type = chars
            .next()
            .ok_or_else(|| serde_json::Error::custom("Packet type not found in packet string"))?;
        let packet_data = chars.as_str();
        let is_upgrade = packet_data.starts_with("probe");
        let res = match packet_type {
            '0' => Packet::Open(serde_json::from_str(packet_data)?),
            '1' => Packet::Close,
            '2' => {
                if is_upgrade {
                    Packet::PingUpgrade
                } else {
                    Packet::Ping
                }
            }
            '3' => {
                if is_upgrade {
                    Packet::PongUpgrade
                } else {
                    Packet::Pong
                }
            }
            '4' => Packet::Message(packet_data.to_string()),
            '5' => Packet::Upgrade,
            '6' => Packet::Noop,
            'b' if value.starts_with("b4") => {
                Packet::BinaryV3(general_purpose::STANDARD.decode(packet_data[1..].as_bytes())?)
            }
            'b' => Packet::Binary(general_purpose::STANDARD.decode(packet_data.as_bytes())?),
            c => Err(serde_json::Error::custom(
                "Invalid packet type ".to_string() + &c.to_string(),
            ))?,
        };
        Ok(res)
    }
}

impl TryFrom<String> for Packet {
    type Error = crate::errors::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Packet::try_from(value.as_str())
    }
}

/// An OpenPacket is used to initiate a connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket {
    sid: Sid,
    upgrades: Vec<String>,
    ping_interval: u64,
    ping_timeout: u64,
    max_payload: u64,
}

impl OpenPacket {
    /// Create a new [OpenPacket]
    /// If the current transport is polling, the server will always allow the client to upgrade to websocket
    pub fn new(transport: TransportType, sid: Sid, config: &EngineIoConfig) -> Self {
        let upgrades = if transport == TransportType::Polling {
            vec!["websocket".to_string()]
        } else {
            vec![]
        };
        OpenPacket {
            sid,
            upgrades,
            ping_interval: config.ping_interval.as_millis() as u64,
            ping_timeout: config.ping_timeout.as_millis() as u64,
            max_payload: config.max_payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::EngineIoConfig;

    use super::*;
    use std::{convert::TryInto, time::Duration};

    #[test]
    fn test_open_packet() {
        let sid = Sid::new();
        let packet = Packet::Open(OpenPacket::new(
            TransportType::Polling,
            sid,
            &EngineIoConfig::default(),
        ));
        let packet_str: String = packet.try_into().unwrap();
        assert_eq!(packet_str, format!("0{{\"sid\":\"{sid}\",\"upgrades\":[\"websocket\"],\"pingInterval\":25000,\"pingTimeout\":20000,\"maxPayload\":100000}}"));
    }

    #[test]
    fn test_open_packet_deserialize() {
        let sid = Sid::new();
        let packet_str = format!("0{{\"sid\":\"{sid}\",\"upgrades\":[\"websocket\"],\"pingInterval\":25000,\"pingTimeout\":20000,\"maxPayload\":100000}}");
        let packet = Packet::try_from(packet_str.to_string()).unwrap();
        assert_eq!(
            packet,
            Packet::Open(OpenPacket {
                sid,
                upgrades: vec!["websocket".to_string()],
                ping_interval: 25000,
                ping_timeout: 20000,
                max_payload: 1e5 as u64,
            })
        );
    }

    #[test]
    fn test_message_packet() {
        let packet = Packet::Message("hello".to_string());
        let packet_str: String = packet.try_into().unwrap();
        assert_eq!(packet_str, "4hello");
    }

    #[test]
    fn test_message_packet_deserialize() {
        let packet_str = "4hello".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::Message("hello".to_string()));
    }

    #[test]
    fn test_binary_packet() {
        let packet = Packet::Binary(vec![1, 2, 3]);
        let packet_str: String = packet.try_into().unwrap();
        assert_eq!(packet_str, "bAQID");
    }

    #[test]
    fn test_binary_packet_deserialize() {
        let packet_str = "bAQID".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::Binary(vec![1, 2, 3]));
    }

    #[test]
    fn test_binary_packet_v3() {
        let packet = Packet::BinaryV3(vec![1, 2, 3]);
        let packet_str: String = packet.try_into().unwrap();
        assert_eq!(packet_str, "b4AQID");
    }

    #[test]
    fn test_binary_packet_v3_deserialize() {
        let packet_str = "b4AQID".to_string();
        let packet: Packet = packet_str.try_into().unwrap();
        assert_eq!(packet, Packet::BinaryV3(vec![1, 2, 3]));
    }

    #[test]
    fn test_packet_get_size_hint() {
        // Max serialized packet
        let open = OpenPacket::new(
            TransportType::Polling,
            Sid::new(),
            &EngineIoConfig {
                max_buffer_size: usize::MAX,
                max_payload: u64::MAX,
                ping_interval: Duration::MAX,
                ping_timeout: Duration::MAX,
                transports: TransportType::Polling as u8 | TransportType::Websocket as u8,
                ..Default::default()
            },
        );
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

        let packet = Packet::Message("hello".to_string());
        assert_eq!(packet.get_size_hint(false), 6);

        let packet = Packet::Upgrade;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Noop;
        assert_eq!(packet.get_size_hint(false), 1);

        let packet = Packet::Binary(vec![1, 2, 3]);
        assert_eq!(packet.get_size_hint(false), 4);
        assert_eq!(packet.get_size_hint(true), 5);

        let packet = Packet::BinaryV3(vec![1, 2, 3]);
        assert_eq!(packet.get_size_hint(false), 4);
        assert_eq!(packet.get_size_hint(true), 6);
    }
}
