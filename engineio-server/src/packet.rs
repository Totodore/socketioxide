use base64::{engine::general_purpose, Engine};
use bytes::Bytes;
use serde::{de::Error, Deserialize, Serialize};

use crate::layer::EngineIoConfig;

#[derive(Debug, PartialEq, PartialOrd)]
pub enum Packet {
    Open(OpenPacket),
    Close,
    Ping,
    Pong,
    PingUpgrade,
    PongUpgrade,
    Message(String),
    Upgrade,
    Noop,

    Binary(Vec<u8>), // Not part of the protocol, used internally
    Abort,           // Not part of the protocol, used internally
}

/**
 * Serialize a Packet to a String according to Engine.IO protocol
 */
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
            Packet::Binary(data) => "b".to_string() + &general_purpose::STANDARD.encode(&data),
            _ => {
                return Err(Self::Error::SerializeError(serde_json::Error::custom(
                    "invalid packet type",
                )))
            }
        };
        Ok(res)
    }
}

impl TryFrom<String> for Packet {
    type Error = crate::errors::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        let packet_type =
            chars
                .next()
                .ok_or(Self::Error::DeserializeError(serde_json::Error::custom(
                    "Packet type not found in packet string",
                )))?;
        let packet_data = chars.as_str();
        let is_upgrade = packet_data.starts_with("probe");
        match packet_type {
            '0' => Ok(Packet::Open(serde_json::from_str(packet_data)?)),
            '1' => Ok(Packet::Close),
            '2' => Ok(if is_upgrade {
                Packet::PingUpgrade
            } else {
                Packet::Ping
            }),
            '3' => Ok(if is_upgrade {
                Packet::PongUpgrade
            } else {
                Packet::Pong
            }),
            '4' => Ok(Packet::Message(packet_data.to_string())),
            '5' => Ok(Packet::Upgrade),
            '6' => Ok(Packet::Noop),
            'b' => Ok(Packet::Binary(
                general_purpose::STANDARD.decode(packet_data.as_bytes())?,
            )),
            c => Err(Self::Error::DeserializeError(serde_json::Error::custom(
                "Invalid packet type ".to_string() + &c.to_string(),
            ))),
        }
    }
}

impl TryFrom<Vec<u8>> for Packet {
    type Error = crate::errors::Error;
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let value = String::from_utf8(value).map_err(Self::Error::from)?;
        Packet::try_from(value)
    }
}

impl TryFrom<Bytes> for Packet {
    type Error = crate::errors::Error;
    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        let value = String::from_utf8(value.to_vec()).map_err(Self::Error::from)?;
        Packet::try_from(value)
    }
}
#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket {
    sid: String,
    upgrades: Vec<String>,
    ping_interval: u64,
    ping_timeout: u64,
    max_payload: u64,
}

impl OpenPacket {
    pub fn new(transport: TransportType, sid: i64, config: &EngineIoConfig) -> Self {
        let upgrades = if transport == TransportType::Polling {
            vec!["websocket".to_string()]
        } else {
            vec![]
        };
        OpenPacket {
            sid: sid.to_string(),
            upgrades,
            ping_interval: config.ping_interval.as_millis() as u64,
            ping_timeout: config.ping_timeout.as_millis() as u64,
            max_payload: 1000000,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TransportType {
    Websocket,
    Polling,
}
