use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use std::fmt::{Debug, Display};
use tokio::{sync::mpsc::error::TrySendError, time::error::Elapsed};

pub use matchit::InsertError as NsInsertError;

pub use crate::parser::{DecodeError, EncodeError};

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
#[derive(thiserror::Error, Debug)]
pub enum AckError {
    /// The ack response cannot be parsed
    #[error("cannot deserialize packet from ack response: {0:?}")]
    Decode(#[from] DecodeError),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout,

    /// An error happened while broadcasting to other socket.io nodes
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),

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

    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing packet: {0:?}")]
    Serialize(#[from] EncodeError),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
}
/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError {
    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing packet: {0:?}")]
    Serialize(#[from] EncodeError),

    /// Error sending/receiving data through the engine.io socket
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(#[from] SocketError),
}

/// Error type when using the underlying engine.io socket
#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    /// The socket channel is full.
    /// You might need to increase the channel size with the [`SocketIoBuilder::max_buffer_size`] method.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    #[error("internal channel full error")]
    InternalChannelFull,

    /// The socket is already closed
    #[error("socket closed")]
    Closed,
}

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum DisconnectError {
    /// The socket channel is full.
    /// You might need to increase the channel size with the [`SocketIoBuilder::max_buffer_size`] method.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    #[error("internal channel full error")]
    InternalChannelFull,

    /// An error occured while broadcasting to other nodes.
    #[error("adapter error: {0:?}")]
    Adapter(#[from] AdapterError),
}

/// Error type for the [`Adapter`](crate::adapter::Adapter) trait.
#[derive(Debug, thiserror::Error)]
pub struct AdapterError(#[from] pub Box<dyn std::error::Error + Send + Sync>);
impl Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl<T> From<TrySendError<T>> for SocketError {
    fn from(value: TrySendError<T>) -> Self {
        match value {
            TrySendError::Full(_) => Self::InternalChannelFull,
            TrySendError::Closed(_) => Self::Closed,
        }
    }
}

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
