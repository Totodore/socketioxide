use engineioxide::sid_generator::Sid;
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
}
