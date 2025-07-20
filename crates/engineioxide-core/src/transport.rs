use std::str::FromStr;

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

/// Cannot determine the transport type to connect to the client/server.
#[derive(Debug, Copy, Clone)]
pub struct UnknownTransportError;
impl std::fmt::Display for UnknownTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown transport type")
    }
}
impl std::error::Error for UnknownTransportError {}

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
