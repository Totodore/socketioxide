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
    pub(crate) fn new_empty() -> Self {
        // mpsc bounded channel requires buffer > 0
        let (_, rx) = mpsc::channel(1);
        Self { rx }
    }
    pub(crate) fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
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

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
pub mod __test_harness {
    use std::{
        collections::HashMap,
        future::Future,
        sync::{Arc, RwLock},
    };

    use tokio::sync::mpsc;

    use super::MessageStream;

    type ChanItem = (String, Vec<u8>);
    #[derive(Debug, Clone)]
    pub struct StubDriver {
        tx: mpsc::Sender<ChanItem>,
        handlers: Arc<RwLock<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
        num_serv: u16,
    }
    async fn pipe_handlers(
        mut rx: mpsc::Receiver<ChanItem>,
        handlers: Arc<RwLock<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
    ) {
        while let Some((chan, data)) = rx.recv().await {
            let _handlers = handlers.read().unwrap().keys().cloned().collect::<Vec<_>>();
            tracing::debug!(?_handlers, "received data to broadcast {}", chan);
            if let Some(tx) = handlers.read().unwrap().get(&chan) {
                tx.try_send(data).unwrap();
            }
        }
    }
    impl StubDriver {
        pub fn new(num_serv: u16) -> (Self, mpsc::Receiver<ChanItem>, mpsc::Sender<ChanItem>) {
            let (tx, rx) = mpsc::channel(255); // driver emitter
            let (tx1, rx1) = mpsc::channel(255); // driver receiver
            let handlers = Arc::new(RwLock::new(HashMap::<_, mpsc::Sender<Vec<u8>>>::new()));

            tokio::spawn(pipe_handlers(rx1, handlers.clone()));

            let driver = Self {
                tx,
                num_serv,
                handlers,
            };
            (driver, rx, tx1)
        }
        pub fn has_handler(&self, pat: &str) -> bool {
            self.handlers.read().unwrap().contains_key(pat)
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "__test_harness")]
    impl super::Driver for StubDriver {
        type Error = std::convert::Infallible;

        fn publish(
            &self,
            chan: String,
            val: Vec<u8>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            self.tx.try_send((chan, val)).unwrap();
            async move { Ok(()) }
        }

        async fn subscribe(&self, pat: String) -> Result<MessageStream, Self::Error> {
            let (tx, rx) = mpsc::channel(255);
            self.handlers.write().unwrap().insert(pat, tx);
            Ok(MessageStream::new(rx))
        }

        fn unsubscribe(
            &self,
            pat: String,
        ) -> impl Future<Output = Result<(), Self::Error>> + 'static {
            self.handlers.write().unwrap().remove(&pat);
            async move { Ok(()) }
        }

        async fn num_serv(&self, _chan: &str) -> Result<u16, Self::Error> {
            Ok(self.num_serv)
        }
    }
}
