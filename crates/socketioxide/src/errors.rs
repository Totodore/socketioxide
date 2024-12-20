use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use serde::{Deserialize, Serialize};
use socketioxide_core::errors::{AdapterError, SocketError};
use std::fmt::Debug;
use tokio::time::error::Elapsed;

pub use matchit::InsertError as NsInsertError;

pub use crate::parser::ParserError;

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

/// Error type for broadcast operations.
#[derive(thiserror::Error, Debug)]
pub enum BroadcastError {
    /// An error occurred while sending packets.
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(Vec<SocketError>),

    /// An error occurred while serializing the packet.
    #[error("Error serializing packet: {0:?}")]
    Serialize(#[from] ParserError),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
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

// impl<T> From<TrySendError<T>> for SocketError {
//     fn from(value: TrySendError<T>) -> Self {
//         match value {
//             TrySendError::Full(_) => Self::InternalChannelFull,
//             TrySendError::Closed(_) => Self::Closed,
//         }
//     }
// }

impl From<Vec<SocketError>> for BroadcastError {
    /// Converts a vector of `SendError` into a `BroadcastError`.
    ///
    /// # Arguments
    ///
    /// * `value` - A vector of `SendError` representing the sending errors.
    ///
    /// # Returns
    ///
    /// A `BroadcastError` containing the sending errors.
    fn from(value: Vec<SocketError>) -> Self {
        Self::Socket(value)
    }
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
