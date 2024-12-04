use std::{future::Future, pin::Pin, task};

use futures_core::Stream;
use pin_project_lite::pin_project;
use socketioxide_core::{errors::AdapterError, Str};
use tokio::sync::mpsc;

mod redis;

pin_project! {
    #[derive(Debug)]
    pub struct MessageStream {
        #[pin]
        rx: mpsc::UnboundedReceiver<String>,
    }
}

impl Stream for MessageStream {
    type Item = String;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx)
    }
}

pub trait Driver: Send + Sync + 'static {
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;
    async fn publish(&self, chan: &str, val: &str) -> Result<(), Self::Error>;
    fn subscribe(
        &self,
        chan: Str,
    ) -> impl Future<Output = Result<MessageStream, Self::Error>> + Send;
    fn unsubscribe(&self, chan: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn num_serv(&self, chan: &str) -> impl Future<Output = Result<u16, Self::Error>> + Send;
}
