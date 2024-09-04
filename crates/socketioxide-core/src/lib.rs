pub mod packet;
pub mod parser;

pub use engineioxide::{sid::Sid, Str};

/// Represent a socket.io payload that can be sent over an engine.io connection
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A string payload that will be sent as a string engine.io packet.
    /// It can also contain adjacent binary payloads.
    Str((Str, Option<Vec<bytes::Bytes>>)),
    /// A binary payload that will be sent as a binary engine.io packet
    Bytes(bytes::Bytes),
}

impl Value {
    pub fn as_str(&self) -> Option<&Str> {
        match self {
            Value::Str((data, _)) => Some(data),
            Value::Bytes(_) => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&bytes::Bytes> {
        match self {
            Value::Str(_) => None,
            Value::Bytes(data) => Some(data),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Value::Str((data, _)) => data.len(),
            Value::Bytes(data) => data.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
