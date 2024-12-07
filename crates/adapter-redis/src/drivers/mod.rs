use std::{future::Future, pin::Pin, task};

use futures_core::Stream;
use pin_project_lite::pin_project;
use socketioxide_core::errors::AdapterError;
use tokio::sync::mpsc;

pub mod redis;

pin_project! {
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

pub trait Driver: Send + Sync + 'static {
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;
    fn publish(
        &self,
        chan: String,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn subscribe(
        &self,
        chan: String,
    ) -> impl Future<Output = Result<MessageStream, Self::Error>> + Send;
    fn unsubscribe(&self, chan: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn num_serv(&self, chan: &str) -> impl Future<Output = Result<u16, Self::Error>> + Send;
}
