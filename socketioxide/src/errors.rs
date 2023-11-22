use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use std::fmt::{Debug, Display};
use tokio::sync::{mpsc::error::TrySendError, oneshot};

/// Error type for socketio
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error serializing json packet: {0:?}")]
    SerializeError(#[from] serde_json::Error),

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

/// Convert an [`Error`] to an [`EIoDisconnectReason`] if possible
///
/// If the error cannot be converted to a [`EIoDisconnectReason`] it means that the error was not fatal
/// and the engine `Socket` can be kept alive
impl From<&Error> for Option<EIoDisconnectReason> {
    fn from(value: &Error) -> Self {
        use EIoDisconnectReason::*;
        match value {
            Error::SocketGone(_) => Some(TransportClose),
            Error::SerializeError(_) | Error::InvalidPacketType | Error::InvalidEventName => {
                Some(PacketParsingError)
            }
            Error::Adapter(_) | Error::InvalidNamespace => None,
        }
    }
}

/// Error type for ack responses
#[derive(thiserror::Error, Debug)]
pub enum AckError {
    /// The ack response cannot be parsed
    #[error("cannot deserializing json packet from ack response: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// The ack response cannot be received correctly
    #[error("ack receive error")]
    AckReceive(#[from] oneshot::error::RecvError),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout(#[from] tokio::time::error::Elapsed),

    /// The emit payload cannot be sent
    #[error("send channel error: {0:?}")]
    SendChannel(#[from] SendError),
}

/// Error type for broadcast operations.
#[derive(Debug, thiserror::Error)]
pub enum BroadcastError {
    /// An error occurred while sending packets.
    #[error("Sending error: {0:?}")]
    SendError(Vec<SendError>),

    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
}

impl From<Vec<SendError>> for BroadcastError {
    /// Converts a vector of `SendError` into a `BroadcastError`.
    ///
    /// # Arguments
    ///
    /// * `value` - A vector of `SendError` representing the sending errors.
    ///
    /// # Returns
    ///
    /// A `BroadcastError` containing the sending errors.
    fn from(value: Vec<SendError>) -> Self {
        Self::SendError(value)
    }
}

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError {
    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    AdapterError(#[from] AdapterError),

    /// The socket channel is full.
    /// You might need to increase the channel size with the [`SocketIoBuilder::max_buffer_size`](crate::SocketIoBuilder) method.
    #[error("internal channel full error")]
    InternalChannelFull,
}

impl<T> From<TrySendError<T>> for SendError {
    fn from(value: TrySendError<T>) -> Self {
        match value {
            TrySendError::Full(_) => Self::InternalChannelFull,
            TrySendError::Closed(_) => panic!("internal channel closed"),
        }
    }
}

/// Error type for the [`Adapter`] trait.
#[derive(Debug, thiserror::Error)]
pub struct AdapterError(#[from] pub Box<dyn std::error::Error + Send>);
impl Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
