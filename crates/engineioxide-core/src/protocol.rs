use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The type of `transport` used to connect to the client/server.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TransportType {
    /// Polling transport
    Polling = 0x01,
    /// Websocket transport
    Websocket = 0x02,
}

impl From<u8> for TransportType {
    fn from(t: u8) -> Self {
        match t {
            0x01 => TransportType::Polling,
            0x02 => TransportType::Websocket,
            _ => panic!("unknown transport type"),
        }
    }
}

impl FromStr for TransportType {
    type Err = UnknownTransportError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "websocket" => Ok(TransportType::Websocket),
            "polling" => Ok(TransportType::Polling),
            _ => Err(UnknownTransportError),
        }
    }
}
impl From<TransportType> for &'static str {
    fn from(t: TransportType) -> Self {
        match t {
            TransportType::Polling => "polling",
            TransportType::Websocket => "websocket",
        }
    }
}
impl From<TransportType> for String {
    fn from(t: TransportType) -> Self {
        match t {
            TransportType::Polling => "polling".into(),
            TransportType::Websocket => "websocket".into(),
        }
    }
}

impl Serialize for TransportType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str((*self).into())
    }
}

impl<'de> Deserialize<'de> for TransportType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Cannot determine the transport type to connect to the client/server.
#[derive(Debug, Copy, Clone)]
pub struct UnknownTransportError;
impl std::fmt::Display for UnknownTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown transport type")
    }
}
impl std::error::Error for UnknownTransportError {}

/// == ProtocolVersion ==

#[derive(Debug)]
pub struct UnknownProtocolVersionError;
impl std::fmt::Display for UnknownProtocolVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown protocol version")
    }
}
impl std::error::Error for UnknownProtocolVersionError {}

/// The engine.io protocol version
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    /// The protocol version 3
    V3 = 3,
    /// The protocol version 4
    V4 = 4,
}

impl FromStr for ProtocolVersion {
    type Err = UnknownProtocolVersionError;

    #[cfg(feature = "v3")]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(UnknownProtocolVersionError),
        }
    }

    #[cfg(not(feature = "v3"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(UnknownProtocolVersionError),
        }
    }
}
