pub use socketioxide_core::adapter::errors::SocketError;
pub use socketioxide_core::parser::ParserError;

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
