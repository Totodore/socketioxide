use engineioxide::sid_generator::Sid;
use std::fmt::Debug;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::warn;

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

    #[error("send channel error: {0:?}")]
    SendChannel(#[from] SendError<engineioxide::SendPacket>),
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
    SendChannel(#[from] SendError<engineioxide::SendPacket>),
}

/// Error type for ack responses
#[derive(thiserror::Error, Debug)]
pub enum SendError<T: Debug> {
    #[error("sent to full socket chan, sid: {sid}, packet: {packet:?}")]
    SocketFull { sid: Sid, packet: T },
    #[error("sent to closed socket chan, sid: {sid}, packet: {packet:?}")]
    SocketClosed { sid: Sid, packet: T },
    #[error("error serializing json packet: {0:?}")]
    Serialize(#[from] serde_json::Error),
}

impl<T: Debug> From<(TrySendError<T>, Sid)> for SendError<T> {
    fn from((err, sid): (TrySendError<T>, Sid)) -> Self {
        match err {
            TrySendError::Closed(packet) => {
                warn!("try to send to closed socket, sid: {sid}, packet: {packet:?}");
                Self::SocketClosed { sid, packet }
            }
            TrySendError::Full(packet) => {
                warn!("try to send to full socket, sid: {sid}, packet: {packet:?}");
                Self::SocketFull { sid, packet }
            }
        }
    }
}
