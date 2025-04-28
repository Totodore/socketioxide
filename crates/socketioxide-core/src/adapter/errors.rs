//! All the errors that can be returned by the crate. Mostly when using the [adapter](crate::adapter) module.
use std::{convert::Infallible, fmt};

use serde::{Deserialize, Serialize};

use crate::parser::ParserError;

/// Error type when using the underlying engine.io socket
#[derive(Debug, thiserror::Error, Serialize, Deserialize, Clone)]
pub enum SocketError {
    /// The socket channel is full.
    /// You might need to increase the channel size with the [`SocketIoBuilder::max_buffer_size`] method.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: https://docs.rs/socketioxide/latest/socketioxide/struct.SocketIoBuilder.html#method.max_buffer_size
    #[error("internal channel full error")]
    InternalChannelFull,

    /// The socket is already closed
    #[error("socket closed")]
    Closed,
}

/// Error type for the [`CoreAdapter`](crate::adapter::CoreAdapter) trait.
#[derive(Debug, thiserror::Error)]
pub struct AdapterError(#[from] pub Box<dyn std::error::Error + Send>);
impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl From<Infallible> for AdapterError {
    fn from(_: Infallible) -> Self {
        panic!("Infallible should never be constructed, this is a bug")
    }
}

/// Error type for broadcast operations.
#[derive(thiserror::Error, Debug)]
pub enum BroadcastError {
    // This type should never constructed with an empty vector!
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

impl From<Vec<SocketError>> for BroadcastError {
    fn from(value: Vec<SocketError>) -> Self {
        assert!(
            !value.is_empty(),
            "Cannot construct a BroadcastError from an empty vec of SocketError"
        );
        Self::Socket(value)
    }
}
