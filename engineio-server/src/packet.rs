use serde::{de::Error, Deserialize, Serialize};

use crate::engine::EngineIoConfig;

#[derive(Debug, PartialEq, PartialOrd)]
pub enum Packet {
    Open(OpenPacket),
    Close,
    Ping,
    Pong,
    Message(String),
    Upgrade,
    Noop,
}

/**
 * Serialize a Packet to a String according to Engine.IO protocol
 */
impl TryInto<String> for Packet {
    type Error = serde_json::Error;
    fn try_into(self) -> Result<String, Self::Error> {
        let res = match self {
            Packet::Open(open) => "0".to_string() + &serde_json::to_string(&open)?,
            Packet::Close => "1".to_string(),
            Packet::Ping => "2".to_string(),
            Packet::Pong => "3".to_string(),
            Packet::Message(msg) => "4".to_string() + &msg,
            Packet::Upgrade => "5".to_string(),
            Packet::Noop => "6".to_string(),
        };
        Ok(res)
    }
}

impl TryFrom<String> for Packet {
    type Error = serde_json::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        let packet_type = chars.next().ok_or(serde_json::Error::custom(
            "Packet type not found in packet string",
        ))?;
        let packet_data = chars.as_str();
        match packet_type {
            '0' => Ok(Packet::Open(serde_json::from_str(packet_data)?)),
            '1' => Ok(Packet::Close),
            '2' => Ok(Packet::Ping),
            '3' => Ok(Packet::Pong),
            '4' => Ok(Packet::Message(serde_json::from_str(packet_data)?)),
            '5' => Ok(Packet::Upgrade),
            '6' => Ok(Packet::Noop),
            _ => Err(serde_json::Error::custom("Invalid packet type")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket {
    sid: String,
    upgrades: Vec<String>,
    ping_interval: u32,
    ping_timeout: u32,
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
            ping_interval: config.ping_interval,
            ping_timeout: config.ping_timeout,
            max_payload: 1000000,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TransportType {
    Websocket,
    Polling,
}
