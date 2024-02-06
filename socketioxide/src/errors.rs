use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use std::fmt::{Debug, Display};
use tokio::{sync::mpsc::error::TrySendError, time::error::Elapsed};

/// Error type for socketio
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error serializing json packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

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

/// Error type for ack operations.
#[derive(thiserror::Error, Debug)]
pub enum AckError<T> {
    /// The ack response cannot be parsed
    #[error("cannot deserialize json packet from ack response: {0:?}")]
    Serde(#[from] serde_json::Error),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout,

    /// An error happened while broadcasting to other socket.io nodes
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),

    /// Error sending/receiving data through the engine.io socket
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(#[from] SocketError<T>),
}

/// Error type for broadcast operations.
#[derive(thiserror::Error)]
pub enum BroadcastError<T> {
    /// An error occurred while sending packets.
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(Vec<SocketError<T>>),

    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
}
impl Debug for BroadcastError<()> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Socket(errs) => {
                f.write_str("BroadcastError::Socket(")?;
                Debug::fmt(errs, f)?;
                f.write_str(")")
            }
            Self::Serialize(err) => {
                f.write_str("BroadcastError::Serialize(")?;
                Debug::fmt(err, f)?;
                f.write_str(")")
            }
            Self::Adapter(err) => {
                f.write_str("BroadcastError::Adapter(")?;
                Debug::fmt(err, f)?;
                f.write_str(")")
            }
        }
    }
}

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError<T> {
    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// Error sending/receiving data through the engine.io socket
    #[error("Error sending data through the engine.io socket: {0:?}")]
    Socket(#[from] SocketError<T>),
}

/// Error type when using the underlying engine.io socket
#[derive(thiserror::Error, Debug)]
pub enum SocketError<T> {
    /// The socket channel is full.
    /// You might need to increase the channel size with the [`SocketIoBuilder::max_buffer_size`] method.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    #[error("internal channel full error")]
    InternalChannelFull(T),

    /// The socket is already closed
    #[error("socket closed")]
    Closed(T),
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
pub struct AdapterError(#[from] pub Box<dyn std::error::Error + Send>);
impl Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl<T> From<TrySendError<T>> for SocketError<()> {
    fn from(value: TrySendError<T>) -> Self {
        match value {
            TrySendError::Full(_) => Self::InternalChannelFull(()),
            TrySendError::Closed(_) => Self::Closed(()),
        }
    }
}

impl<T> From<Vec<SocketError<T>>> for BroadcastError<T> {
    /// Converts a vector of `SendError` into a `BroadcastError`.
    ///
    /// # Arguments
    ///
    /// * `value` - A vector of `SendError` representing the sending errors.
    ///
    /// # Returns
    ///
    /// A `BroadcastError` containing the sending errors.
    fn from(value: Vec<SocketError<T>>) -> Self {
        Self::Socket(value)
    }
}

impl<T> From<Elapsed> for AckError<T> {
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
            Error::Serialize(_) | Error::InvalidPacketType | Error::InvalidEventName => {
                Some(PacketParsingError)
            }
            Error::Adapter(_) | Error::InvalidNamespace => None,
        }
    }
}
