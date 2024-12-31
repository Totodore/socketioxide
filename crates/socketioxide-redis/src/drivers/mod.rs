use std::{future::Future, pin::Pin, task};

use futures_core::Stream;
use pin_project_lite::pin_project;
use tokio::sync::mpsc;

/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
pub mod redis;

pin_project! {
    /// A stream of raw messages received from a channel.
    /// Messages are encoded with msgpack.
    #[derive(Debug)]
    pub struct MessageStream {
        #[pin]
        rx: mpsc::Receiver<Vec<u8>>,
    }
}

impl MessageStream {
    /// Create a new empty message stream.
    pub fn new_empty() -> Self {
        // mpsc bounded channel requires buffer > 0
        let (_, rx) = mpsc::channel(1);
        Self { rx }
    }
    /// Create a new message stream from a receiver.
    pub fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self { rx }
    }
}

impl Stream for MessageStream {
    type Item = Vec<u8>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx)
    }
}

/// The driver trait can be used to support different pub/sub backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    /// The error type for the driver.
    type Error: std::error::Error + Send + 'static;

    /// Publish a message to a channel.
    fn publish(
        &self,
        chan: String,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Subscribe to a channel with a pattern, it will return a stream of messages.
    fn subscribe(
        &self,
        pat: String,
    ) -> impl Future<Output = Result<MessageStream, Self::Error>> + Send;

    /// Unsubscribe from a channel.
    fn unsubscribe(
        &self,
        pat: String,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static;

    /// Returns the number of socket.io servers.
    fn num_serv(&self, chan: &str) -> impl Future<Output = Result<u16, Self::Error>> + Send;
}
