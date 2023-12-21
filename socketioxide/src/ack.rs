//! Acknowledgement related types and functions.
//!
//! There are two main types:
//! - [`AckStream`]: A [`Stream`]/[`Future`] of [`AckResponse`] received from the client.
//! - [`AckResponse`]: An acknowledgement sent by the client.
use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::FusedFuture,
    stream::{FusedStream, FuturesUnordered},
    Future, Stream,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::{sync::oneshot::Receiver, time::Timeout};

use crate::{
    adapter::Adapter, errors::AckError, extract::SocketRef, packet::Packet, BroadcastError,
};

/// An acknowledgement sent by the client.
/// It contains the data sent by the client and the binary payloads if there are any.
#[derive(Debug)]
pub struct AckResponse<T> {
    /// The data returned by the client
    pub data: T,
    /// Optional binary payloads
    ///
    /// If there is no binary payload, the `Vec` will be empty
    pub binary: Vec<Vec<u8>>,
}

pin_project_lite::pin_project! {
    /// A [`Stream`]/[`Future`] of [`AckResponse`] received from the client.
    ///
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] received from the client.
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    ///
    /// It can be created from:
    /// * The [`SocketRef::emit_with_ack`](extract::SocketRef) method, in this case there will be only one [`AckResponse`].
    /// * The [`Operator::broadcast_with_ack`](operators::Operators) method, in this case there will be as many [`AckResponse`]
    /// as there are selected sockets.
    /// * The [`SocketIo::broadcast_with_ack`](crate::SocketIo) method, in this case there will be as many [`AckResponse`]
    /// as there are sockets in the namespace.
    ///
    /// # Example
    /// ```rust
    /// # use socketioxide::extract::SocketRef;
    /// # use socketioxide::ack::AckStream;
    /// # use futures::StreamExt;
    /// # use socketioxide::SocketIo;
    /// let (svc, io) = SocketIo::new_svc();
    /// io.ns("/test", move |socket: SocketRef| async move {
    ///     // We wait for the acknowledgement of the first emit (only one in this case)
    ///     let ack = socket.emit_with_ack::<String>("test", "test").unwrap().await.unwrap();
    ///     println!("Ack: {:?}", ack);
    ///
    ///     // We apply the `for_each` StreamExt fn to the AckStream
    ///     socket.broadcast().emit_with_ack::<String>("test", "test")
    ///         .unwrap()
    ///         .for_each(|ack| async move { println!("Ack: {:?}", ack); }).await;
    /// });
    #[must_use = "futures and streams do nothing unless you `.await` or poll them"]
    pub struct AckStream<T> {
        #[pin]
        inner: AckInnerStream,
        _marker: std::marker::PhantomData<T>,
    }
}

pin_project_lite::pin_project! {
    #[allow(missing_docs)]
    #[project = InnerProj]
    /// An internal stream used by [`AckStream`]. It should not be used directly except when implementing the
    /// [`Adapter`](crate::adapter::Adapter) trait.
    pub enum AckInnerStream {
        Stream {
            #[pin]
            rxs: FuturesUnordered<Timeout<Receiver<AckResponse<Value>>>>,
        },

        Fut {
            #[pin]
            rx: Timeout<Receiver<AckResponse<Value>>>,
            polled: bool,
        },
    }
}

// ==== impl AckInnerStream ====
impl AckInnerStream {
    /// Creates a new [`AckInnerStream`] from a [`Packet`] and a list of sockets.
    /// The [`Packet`] is sent to all the sockets and the [`AckInnerStream`] will wait
    /// for an acknowledgement from each socket.
    ///
    /// The [`AckInnerStream`] will wait for the default timeout specified in the config
    /// (5s by default) if no custom timeout is specified.
    pub fn broadcast(
        packet: Packet<'static>,
        sockets: Vec<SocketRef<impl Adapter>>,
        duration: Option<Duration>,
    ) -> Result<Self, BroadcastError> {
        assert!(!sockets.is_empty());

        let rxs = FuturesUnordered::new();
        let mut errs = Vec::new();
        let duration = duration.unwrap_or_else(|| sockets.first().unwrap().config.ack_timeout);
        for socket in sockets {
            match socket.send_with_ack(packet.clone()) {
                Ok(rx) => rxs.push(tokio::time::timeout(duration, rx)),
                Err(e) => errs.push(e),
            }
        }
        if errs.is_empty() {
            Ok(AckInnerStream::Stream { rxs })
        } else {
            Err(errs.into())
        }
    }

    /// Creates a new [`AckInnerStream`] from a [`oneshot::Receiver`](tokio) corresponding to the acknowledgement
    /// of a single socket.
    pub fn send(rx: Receiver<AckResponse<Value>>, duration: Duration) -> Self {
        AckInnerStream::Fut {
            rx: tokio::time::timeout(duration, rx),
            polled: false,
        }
    }
}

impl Stream for AckInnerStream {
    type Item = Result<AckResponse<Value>, AckError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project() {
            InnerProj::Stream { rxs, .. } => match rxs.poll_next(cx) {
                Poll::Ready(Some(Ok(Ok(v)))) => Poll::Ready(Some(Ok(v))),
                Poll::Ready(Some(Err(_))) => Poll::Ready(Some(Err(AckError::Timeout))),
                Poll::Ready(Some(Ok(Err(e)))) => Poll::Ready(Some(Err(e.into()))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
            InnerProj::Fut { polled, .. } if *polled => Poll::Ready(None),
            InnerProj::Fut { rx, .. } => match rx.poll(cx) {
                Poll::Ready(Ok(Ok(v))) => Poll::Ready(Some(Ok(v))),
                Poll::Ready(Ok(Err(e))) => Poll::Ready(Some(Err(e.into()))),
                Poll::Ready(Err(_)) => Poll::Ready(Some(Err(AckError::Timeout))),
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            AckInnerStream::Stream { rxs, .. } => rxs.size_hint(),
            AckInnerStream::Fut { .. } => (1, Some(1)),
        }
    }
}
impl FusedStream for AckInnerStream {
    fn is_terminated(&self) -> bool {
        match self {
            AckInnerStream::Stream { rxs, .. } => rxs.is_terminated(),
            AckInnerStream::Fut { polled, .. } => *polled,
        }
    }
}
impl Future for AckInnerStream {
    type Output = Result<AckResponse<Value>, AckError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().poll_next(cx) {
            Poll::Ready(Some(v)) => Poll::Ready(v),
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                unreachable!("stream should at least yield 1 value")
            }
        }
    }
}
impl FusedFuture for AckInnerStream {
    fn is_terminated(&self) -> bool {
        match self {
            AckInnerStream::Stream { rxs, .. } => rxs.is_terminated(),
            AckInnerStream::Fut { polled, .. } => *polled,
        }
    }
}

// ==== impl AckStream ====

impl<T: DeserializeOwned> Stream for AckStream<T> {
    type Item = Result<AckResponse<T>, AckError>;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project()
            .inner
            .poll_next(cx)
            .map(|v| v.map(map_ack_response))
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T: DeserializeOwned> FusedStream for AckStream<T> {
    #[inline(always)]
    fn is_terminated(&self) -> bool {
        FusedStream::is_terminated(&self.inner)
    }
}

impl<T: DeserializeOwned> Future for AckStream<T> {
    type Output = Result<AckResponse<T>, AckError>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(map_ack_response)
    }
}

impl<T: DeserializeOwned> FusedFuture for AckStream<T> {
    #[inline(always)]
    fn is_terminated(&self) -> bool {
        FusedFuture::is_terminated(&self.inner)
    }
}

impl<T> From<AckInnerStream> for AckStream<T> {
    fn from(inner: AckInnerStream) -> Self {
        Self {
            inner,
            _marker: std::marker::PhantomData,
        }
    }
}

fn map_ack_response<T: DeserializeOwned>(
    ack: Result<AckResponse<Value>, AckError>,
) -> Result<AckResponse<T>, AckError> {
    ack.and_then(|v| {
        serde_json::from_value(v.data)
            .map(|data| AckResponse {
                data,
                binary: v.binary,
            })
            .map_err(|e| e.into())
    })
}
