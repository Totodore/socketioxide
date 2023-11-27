use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason};
use std::{
    fmt::{Debug, Display},
    sync::Arc,
};
use tokio::sync::{mpsc::error::TrySendError, oneshot};

use crate::{
    adapter::{Adapter, LocalAdapter},
    socket::Socket,
};

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

/// Error type for ack responses
#[derive(thiserror::Error, Debug)]
pub enum AckError<T> {
    /// The ack response cannot be parsed
    #[error("cannot deserializing json packet from ack response: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// The ack response cannot be received correctly
    #[error("ack receive error")]
    AckReceive(#[from] oneshot::error::RecvError),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout(#[from] tokio::time::error::Elapsed),

    /// An error occurred while sending packets.
    #[error("Socket error: {0}")]
    SocketError(#[from] SocketError<T>),
}

/// Error type for broadcast operations.
#[derive(Debug, thiserror::Error)]
pub enum BroadcastError<T> {
    /// An error occurred while sending packets.
    #[error("Socket error: {0}")]
    SocketError(#[from] SocketError<T>),

    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
}

impl<T> From<Vec<SendError<T>>> for BroadcastError<T> {
    /// Converts a vector of `SendError` into a `BroadcastError`.
    ///
    /// # Arguments
    ///
    /// * `value` - A vector of `SendError` representing the sending errors.
    ///
    /// # Returns
    ///
    /// A `BroadcastError` containing the sending errors.
    fn from(value: Vec<SendError<T>>) -> Self {
        for error in value {
            match error {
                SendError::Serialize(e) => return Self::Serialize(e),
                SendError::Adapter(e) => return Self::Adapter(e),
                _ => {}
            }
        }
        Self::SocketError(
            value
                .into_iter()
                .map(|e| match e {
                    SendError::Socket(e) => e,
                    _ => unreachable!(),
                })
                .collect(),
        )
    }
}

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError<T> {
    /// An error occurred while serializing the JSON packet.
    #[error("Error serializing JSON packet: {0:?}")]
    Serialize(#[from] serde_json::Error),

    /// An error occured while broadcasting to other nodes.
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),

    #[error("Socket error: {0}")]
    Socket(#[from] SocketError<T>),
}

#[derive(thiserror::Error, Debug)]
pub enum SocketError<T> {
    #[error("internal channel full error")]
    InternalChannelFull(T),

    #[error("socket closed")]
    SocketClosed(T),
}

impl SocketError<()> {
    pub fn with_data<T>(self, data: T) -> SocketError<T> {
        match self {
            Self::InternalChannelFull(_) => SocketError::InternalChannelFull(data),
            Self::SocketClosed(_) => SocketError::SocketClosed(data),
        }
    }
}

impl<T> From<TrySendError<T>> for SocketError<()> {
    fn from(value: TrySendError<T>) -> Self {
        match value {
            TrySendError::Full(data) => Self::InternalChannelFull(()),
            TrySendError::Closed(data) => Self::SocketClosed(()),
        }
    }
}
#[derive(thiserror::Error, Debug)]
pub enum AckSenderError<T, A: Adapter = LocalAdapter> {
    #[error("Failed to send ack message")]
    Socket {
        /// The specific error that occurred while sending the message.
        send_error: SocketError<T>,
        /// The socket associated with the error.
        socket: Arc<Socket<A>>,
    },
}

/// Error type for the [`Adapter`] trait.
#[derive(Debug, thiserror::Error)]
pub struct AdapterError(#[from] pub Box<dyn std::error::Error + Send>);
impl Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
