pub mod packet;
pub mod parser;

pub use engineioxide::{sid::Sid, Str};

/// Represent a socket.io payload that can be sent over an engine.io connection
#[derive(Debug, Clone, PartialEq)]
pub enum SocketIoValue {
    /// A string payload that will be sent as a string engine.io packet.
    /// It can also contain adjacent binary payloads.
    Str((Str, Option<Vec<bytes::Bytes>>)),
    /// A binary payload that will be sent as a binary engine.io packet
    Bytes(bytes::Bytes),
}

impl SocketIoValue {
    pub fn as_str(&self) -> Option<&Str> {
        match self {
            SocketIoValue::Str((data, _)) => Some(data),
            SocketIoValue::Bytes(_) => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&bytes::Bytes> {
        match self {
            SocketIoValue::Str(_) => None,
            SocketIoValue::Bytes(data) => Some(data),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            SocketIoValue::Str((data, _)) => data.len(),
            SocketIoValue::Bytes(data) => data.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
