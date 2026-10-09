pub use socketioxide_core::adapter::errors::SocketError;
pub use socketioxide_core::parser::ParserError;
use tokio::sync::oneshot;
use tokio::time;

/// Error type for sending operations.
#[derive(thiserror::Error, Debug)]
pub enum SendError {
    /// An error occurred while serializing the packet.
    #[error("Error serializing packet: {0:?}")]
    Serialize(#[from] ParserError),

    /// Error sending/receiving data through the engine.io socket
    #[error("socket is closed")]
    SocketClosed,
}

/// Error type for ack operations.
#[derive(thiserror::Error, Debug)]
pub enum AckError {
    /// The ack response cannot be parsed
    #[error("cannot deserialize packet from ack response: {0:?}")]
    Decode(#[from] ParserError),

    /// The ack response timed out
    #[error("ack timeout error")]
    Timeout,

    #[error("socket is being closed")]
    SocketClosed,
}

impl From<oneshot::error::RecvError> for AckError {
    fn from(_: oneshot::error::RecvError) -> Self {
        Self::SocketClosed
    }
}
impl From<time::error::Elapsed> for AckError {
    fn from(_: time::error::Elapsed) -> Self {
        Self::Timeout
    }
}
