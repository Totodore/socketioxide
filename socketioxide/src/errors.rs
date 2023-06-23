use crate::retryer::Retryer;
use engineioxide::sid_generator::Sid;
use std::fmt::Debug;
use tokio::sync::oneshot;

/// Error type for socketio
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error serializing json packet: {0:?}")]
    SerializeError(#[from] serde_json::Error),

    #[error("invalid packet type")]
    InvalidPacketType,

    #[error("invalid event name")]
    InvalidEventName,

    #[error("cannot find socketio engine")]
    EngineGone,

    #[error("cannot find socketio socket")]
    SocketGone(Sid),

    /// An engineio error
    #[error("engineio error: {0}")]
    EngineIoError(#[from] engineioxide::errors::Error),
}

/// Error type for ack responses
#[derive(thiserror::Error, Debug)]
pub enum AckError {
    /// The ack response cannot be parsed
    #[error("error serializing/deserializing json packet: {0:?}")]
    SerdeError(#[from] serde_json::Error),

    /// The ack response cannot be received correctly
    #[error("ack receive error")]
    AckReceiveError(#[from] oneshot::error::RecvError),

    /// The ack response timed out
    #[error("ack timeout error")]
    AckTimeoutError(#[from] tokio::time::error::Elapsed),

    /// Internal error
    #[error("internal error: {0}")]
    InternalError(#[from] Error),

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
    /// An error occurred during the retry process in the `Retryer`.
    #[error("Send error: {0:?}")]
    RetryerError(#[from] RetryerError),
}

/// Error type for the `Retryer` struct indicating various failure scenarios during the retry process.
#[derive(thiserror::Error, Debug)]
pub enum RetryerError {
    /// The packet was sent to a closed socket channel.
    #[error("Sent to a closed socket channel, sid: {sid}")]
    SocketClosed { sid: Sid },
    /// There are remaining packets to be sent, indicating that the socket channel is full.
    #[error("Sent to a full socket channel")]
    Remaining(Retryer),
}
