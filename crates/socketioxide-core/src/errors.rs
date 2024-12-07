//! All the errors that can be returned by the library. Mostly when using the [adapter](crate::adapter) module.
use std::{convert::Infallible, fmt};

use serde::{Deserialize, Serialize};
/// Error type when using the underlying engine.io socket
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
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

/// Error type for the [`Adapter`](crate::adapter::Adapter) trait.
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
