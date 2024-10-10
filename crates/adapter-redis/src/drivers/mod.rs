use std::{future::Future, pin::Pin, task};

use futures_core::Stream;
use pin_project_lite::pin_project;
use socketioxide_core::errors::AdapterError;
use tokio::sync::mpsc;

/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
pub mod redis;

pin_project! {
    /// A stream of messages received from a channel.
    ///
    /// Messages are encoded with msgpack.
    #[derive(Debug)]
    pub struct MessageStream {
        #[pin]
        rx: mpsc::Receiver<Vec<u8>>,
    }
}

impl MessageStream {
    pub(crate) fn new_empty() -> Self {
        let (_, rx) = mpsc::channel(0);
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
pub trait Driver: Send + Sync + 'static {
    /// The error type for the driver.
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;

    /// Publish a message to a channel.
    fn publish<'a>(
        &self,
        chan: &'a str,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Subscribe to a channel with a pattern, it will return a stream of messages.
    fn subscribe(
        &self,
        pat: String,
    ) -> impl Future<Output = Result<MessageStream, Self::Error>> + Send;

    /// Unsubscribe from a channel.
    fn unsubscribe(&self, pat: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns the number of socket.io servers.
    fn num_serv(&self, chan: &str) -> impl Future<Output = Result<u16, Self::Error>> + Send;
}
