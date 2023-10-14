//! All transports modules available in engineioxide

use std::str::FromStr;

use crate::errors::Error;

pub mod polling;
pub mod ws;

/// The type of the [`transport`](crate::transport) used by the client.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TransportType {
    Polling = 0x01,
    Websocket = 0x02,
}

impl FromStr for TransportType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "websocket" => Ok(TransportType::Websocket),
            "polling" => Ok(TransportType::Polling),
            _ => Err(Error::UnknownTransport),
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
