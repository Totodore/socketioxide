use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tokio::time::error::Elapsed;

pub use matchit::InsertError as NsInsertError;

pub use crate::parser::ParserError;
pub use socketioxide_core::errors::{AdapterError, BroadcastError, SocketError};

/// Error type for socketio
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid packet type")]
    InvalidPacketType,

    #[error("invalid event name")]
    InvalidEventName,

    #[error("invalid namespace")]
    InvalidNamespace,

    #[error("cannot find socketio socket")]
    SocketGone(Sid),

    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),
}

pub(crate) struct ConnectFail;

/// Error type for ack operations.
#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
pub enum AckError {
    /// The ack response cannot be parsed
    #[error("cannot deserialize packet from ack response: {0:?}")]
    Decode(#[from] ParserError),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout,

    /// Error sending/receiving data through the engine.io socket
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(#[from] SocketError),
}

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError {
    /// An error occurred while serializing the packet.
    #[error("Error serializing packet: {0:?}")]
    Serialize(#[from] ParserError),

    /// Error sending/receiving data through the engine.io socket
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(#[from] SocketError),
}

/// Error type for the [`emit_with_ack`](crate::operators::BroadcastOperators::emit_with_ack) method.
#[derive(thiserror::Error, Debug)]
pub enum EmitWithAckError {
    /// An error occurred while encoding the data.
    #[error("Error encoding data: {0:?}")]
    Encode(#[from] ParserError),
    /// An error occurred while broadcasting to other nodes.
    #[error("Adapter error: {0:?}")]
    Adapter(#[from] Box<dyn std::error::Error + Send>),
}

impl From<Elapsed> for AckError {
    fn from(_: Elapsed) -> Self {
        Self::Timeout
    }
}

/// Convert an [`Error`] to an [`EIoDisconnectReason`] if possible
///
/// If the error cannot be converted to a [`EIoDisconnectReason`] it means that the error was not fatal
/// and the engine `Socket` can be kept alive
impl From<&Error> for Option<EIoDisconnectReason> {
    fn from(value: &Error) -> Self {
        use EIoDisconnectReason::*;
        match value {
            Error::SocketGone(_) => Some(TransportClose),
            Error::InvalidPacketType | Error::InvalidEventName => Some(PacketParsingError),
            Error::Adapter(_) | Error::InvalidNamespace => None,
        }
    }
}
