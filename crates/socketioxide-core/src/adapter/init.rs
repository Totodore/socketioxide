//! Generic init future for remote adapters.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::future::BoxFuture;

use crate::adapter::Spawnable;

/// The result of an adapter init future.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct InitRes<E>(BoxFuture<'static, Result<(), E>>);

impl<E> InitRes<E> {
    /// Create a new init future from a boxed future.
    pub fn new(fut: BoxFuture<'static, Result<(), E>>) -> Self {
        Self(fut)
    }
}

impl<E> Future for InitRes<E> {
    type Output = Result<(), E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

impl<E: fmt::Display + Send + 'static> Spawnable for InitRes<E> {
    fn spawn(self) {
        tokio::spawn(async move {
            if let Err(e) = self.0.await {
                tracing::error!("error initializing adapter: {e}");
            }
        });
    }
}
