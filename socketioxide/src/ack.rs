//! Acknowledgement related types and functions.
//!
//! There are two main types:
//!
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
    adapter::{Adapter, LocalAdapter},
    errors::AckError,
    extract::SocketRef,
    packet::Packet,
    SocketError,
};

/// An acknowledgement sent by the client.
/// It contains the data sent by the client and the binary payloads if there are any.
#[derive(Debug)]
pub struct AckResponse<T, A: Adapter = LocalAdapter> {
    /// The data returned by the client
    pub data: T,
    /// Optional binary payloads.
    /// If there is no binary payload, the `Vec` will be empty
    pub binary: Vec<Vec<u8>>,

    /// The socket that sent the acknowledgement
    pub socket: SocketRef<A>,
}

pub(crate) type AckResult<T = Value, A = LocalAdapter> = Result<AckResponse<T, A>, AckError>;

pin_project_lite::pin_project! {
    /// A [`Stream`]/[`Future`] of [`AckResponse`] received from the client.
    ///
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] received from the client.
    /// It can useful when broadcasting to multiple sockets and therefore expecting
    /// more than one acknowledgement.
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    /// Useful when expecting only one acknowledgement.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `T`,
    /// an [`AckError::Serialize`] will be yielded.
    ///
    /// It can be created from:
    /// * The [`SocketRef::emit_with_ack`] method, in this case there will be only one [`AckResponse`].
    /// * The [`Operator::emit_with_ack`] method, in this case there will be as many [`AckResponse`]
    /// as there are selected sockets.
    /// * The [`SocketIo::emit_with_ack`] method, in this case there will be as many [`AckResponse`]
    /// as there are sockets in the namespace.
    ///
    /// [`SocketRef::emit_with_ack`]: crate::extract::SocketRef#method.emit_with_ack
    /// [`Operator::emit_with_ack`]: crate::operators::Operators#method.emit_with_ack
    /// [`SocketIo::emit_with_ack`]: crate::SocketIo#method.emit_with_ack
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
    ///     let ack = socket.emit_with_ack::<String>("test", "test").await.unwrap();
    ///     println!("Ack: {:?}", ack);
    ///
    ///     // We apply the `for_each` StreamExt fn to the AckStream
    ///     socket.broadcast().emit_with_ack::<String>("test", "test")
    ///         .for_each(|ack| async move { println!("Ack: {:?}", ack); }).await;
    /// });
    /// ```
    #[must_use = "futures and streams do nothing unless you `.await` or poll them"]
    pub struct AckStream<T, A: Adapter = LocalAdapter> {
        #[pin]
        inner: AckInnerStream<A>,
        _marker: std::marker::PhantomData<T>,
    }
}

pin_project_lite::pin_project! {
    #[allow(missing_docs)]
    #[project = InnerProj]
    /// An internal stream used by [`AckStream`]. It should not be used directly except when implementing the
    /// [`Adapter`](crate::adapter::Adapter) trait.
    pub enum AckInnerStream<A: Adapter> {
        Stream {
            #[pin]
            rxs: FuturesUnordered<Timeout<Receiver<AckResult<Value, A>>>>,
        },

        Fut {
            #[pin]
            rx: Timeout<Receiver<AckResult<Value, A>>>,
            polled: bool,
        },

        /// Used to track a serialization error that might occur
        /// before sending to any socket
        EncodingError {
            error: Option<serde_json::Error>,
            polled: bool,
        }
    }
}

// ==== impl AckInnerStream ====
impl<A: Adapter> AckInnerStream<A> {
    /// Creates a new [`AckInnerStream`] from a [`Packet`] and a list of sockets.
    /// The [`Packet`] is sent to all the sockets and the [`AckInnerStream`] will wait
    /// for an acknowledgement from each socket.
    ///
    /// The [`AckInnerStream`] will wait for the default timeout specified in the config
    /// (5s by default) if no custom timeout is specified.
    pub fn broadcast(
        packet: Packet<'static>,
        sockets: Vec<SocketRef<A>>,
        duration: Option<Duration>,
    ) -> Self {
        let rxs = FuturesUnordered::new();

        if sockets.is_empty() {
            return AckInnerStream::Stream { rxs };
        }

        let duration = duration.unwrap_or_else(|| sockets.first().unwrap().config.ack_timeout);
        for socket in sockets {
            let rx = socket.send_with_ack(packet.clone());
            rxs.push(tokio::time::timeout(duration, rx));
        }
        AckInnerStream::Stream { rxs }
    }

    /// Creates a new [`AckInnerStream`] from a [`oneshot::Receiver`](tokio) corresponding to the acknowledgement
    /// of a single socket.
    pub fn send(rx: Receiver<AckResult<Value, A>>, duration: Duration) -> Self {
        AckInnerStream::Fut {
            polled: false,
            rx: tokio::time::timeout(duration, rx),
        }
    }
}

impl<A: Adapter> Stream for AckInnerStream<A> {
    type Item = AckResult<Value, A>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use InnerProj::*;

        debug_assert!(match self.as_mut().project() {
            EncodingError { polled, error } => error.is_some() || *polled,
            Stream { .. } | Fut { .. } => true,
        });

        match self.project() {
            Fut { polled, .. } | EncodingError { polled, .. } if *polled => Poll::Ready(None),
            Stream { rxs } => match rxs.poll_next(cx) {
                Poll::Ready(Some(Ok(Ok(Ok(v))))) => Poll::Ready(Some(Ok(v))),
                Poll::Ready(Some(Ok(Ok(Err(e))))) => Poll::Ready(Some(Err(e))),
                Poll::Ready(Some(Ok(Err(_)))) => {
                    Poll::Ready(Some(Err(AckError::Socket(SocketError::Closed))))
                }
                Poll::Ready(Some(Err(_))) => Poll::Ready(Some(Err(AckError::Timeout))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
            Fut { rx, polled } => match rx.poll(cx) {
                Poll::Ready(val) => {
                    *polled = true;
                    Poll::Ready(match val {
                        Ok(Ok(Ok(v))) => Some(Ok(v)),
                        Ok(Ok(Err(e))) => Some(Err(e)),
                        Ok(Err(_)) => Some(Err(AckError::Socket(SocketError::Closed))),
                        Err(_) => Some(Err(AckError::Timeout)),
                    })
                }
                Poll::Pending => Poll::Pending,
            },
            EncodingError { error, polled } => {
                *polled = true;
                Poll::Ready(Some(Err(AckError::Serde(error.take().unwrap()))))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        use AckInnerStream::*;
        match self {
            Stream { rxs, .. } => rxs.size_hint(),
            Fut { .. } | EncodingError { .. } => (1, Some(1)),
        }
    }
}

impl<A: Adapter> FusedStream for AckInnerStream<A> {
    fn is_terminated(&self) -> bool {
        use AckInnerStream::*;
        match self {
            Stream { rxs, .. } => rxs.is_terminated(),
            Fut { polled, .. } | EncodingError { polled, .. } => *polled,
        }
    }
}

impl<A: Adapter> Future for AckInnerStream<A> {
    type Output = AckResult<Value, A>;

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

impl<A: Adapter> FusedFuture for AckInnerStream<A> {
    fn is_terminated(&self) -> bool {
        use AckInnerStream::*;
        match self {
            Stream { rxs, .. } => rxs.is_terminated(),
            Fut { polled, .. } | EncodingError { polled, .. } => *polled,
        }
    }
}

// ==== impl AckStream ====

impl<T: DeserializeOwned, A: Adapter> Stream for AckStream<T, A> {
    type Item = AckResult<T, A>;

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

impl<T: DeserializeOwned, A: Adapter> FusedStream for AckStream<T, A> {
    #[inline(always)]
    fn is_terminated(&self) -> bool {
        FusedStream::is_terminated(&self.inner)
    }
}

impl<T: DeserializeOwned, A: Adapter> Future for AckStream<T, A> {
    type Output = AckResult<T, A>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(map_ack_response)
    }
}

impl<T: DeserializeOwned, A: Adapter> FusedFuture for AckStream<T, A> {
    #[inline(always)]
    fn is_terminated(&self) -> bool {
        FusedFuture::is_terminated(&self.inner)
    }
}

impl<T, A: Adapter> From<AckInnerStream<A>> for AckStream<T, A> {
    fn from(inner: AckInnerStream<A>) -> Self {
        Self {
            inner,
            _marker: std::marker::PhantomData,
        }
    }
}
impl<T, A: Adapter> From<serde_json::Error> for AckStream<T, A> {
    fn from(error: serde_json::Error) -> Self {
        Self {
            inner: AckInnerStream::EncodingError {
                error: Some(error),
                polled: false,
            },
            _marker: std::marker::PhantomData,
        }
    }
}

fn map_ack_response<T: DeserializeOwned, A: Adapter>(ack: AckResult<Value, A>) -> AckResult<T, A> {
    ack.and_then(|v| {
        serde_json::from_value(v.data)
            .map(|data| AckResponse {
                data,
                binary: v.binary,
                socket: v.socket,
            })
            .map_err(|e| e.into())
    })
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use engineioxide::sid::Sid;
    use futures::StreamExt;

    use crate::{adapter::LocalAdapter, ns::Namespace, socket::Socket};

    use super::*;

    fn create_socket() -> Arc<Socket<LocalAdapter>> {
        let sid = Sid::new();
        let ns = Namespace::<LocalAdapter>::new_dummy([sid]).into();
        let socket = Socket::new_dummy(sid, ns);
        socket.into()
    }

    #[tokio::test]
    async fn broadcast_ack() {
        let socket = create_socket();
        let socket2 = create_socket();
        let mut packet = Packet::event("/", "test", "test".into());
        packet.inner.set_ack_id(1);
        let socks = vec![socket.clone().into(), socket2.clone().into()];
        let stream: AckStream<String> = AckInnerStream::broadcast(packet, socks, None).into();

        let res_packet = Packet::ack("test", "test".into(), 1);
        socket.recv(res_packet.inner.clone()).unwrap();
        socket2.recv(res_packet.inner).unwrap();

        futures::pin_mut!(stream);

        assert!(matches!(stream.next().await.unwrap(), Ok(_)));
        assert!(matches!(stream.next().await.unwrap(), Ok(_)));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_stream() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        tx.send(Ok(AckResponse {
            data: Value::String("test".into()),
            binary: vec![],
            socket: create_socket().into(),
        }))
        .unwrap();

        futures::pin_mut!(stream);

        assert_eq!(
            stream.next().await.unwrap().unwrap().data,
            "test".to_string()
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_fut() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        tx.send(Ok(AckResponse {
            data: Value::String("test".into()),
            binary: vec![],
            socket: create_socket().into(),
        }))
        .unwrap();

        assert_eq!(stream.await.unwrap().data, "test".to_string());
    }

    #[tokio::test]
    async fn broadcast_ack_with_deserialize_error() {
        let socket = create_socket();
        let socket2 = create_socket();
        let mut packet = Packet::event("/", "test", "test".into());
        packet.inner.set_ack_id(1);
        let socks = vec![socket.clone().into(), socket2.clone().into()];
        let stream: AckStream<String> = AckInnerStream::broadcast(packet, socks, None).into();

        let res_packet = Packet::ack("test", 132.into(), 1);
        socket.recv(res_packet.inner.clone()).unwrap();
        socket2.recv(res_packet.inner).unwrap();

        futures::pin_mut!(stream);

        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Serde(_)
        ));
        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Serde(_)
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_stream_with_deserialize_error() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        tx.send(Ok(AckResponse {
            data: Value::Bool(true),
            binary: vec![],
            socket: create_socket().into(),
        }))
        .unwrap();
        assert_eq!(stream.size_hint().0, 1);
        assert_eq!(stream.size_hint().1.unwrap(), 1);

        futures::pin_mut!(stream);

        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Serde(_)
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_fut_with_deserialize_error() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        tx.send(Ok(AckResponse {
            data: Value::Bool(true),
            binary: vec![],
            socket: create_socket().into(),
        }))
        .unwrap();

        assert!(matches!(stream.await.unwrap_err(), AckError::Serde(_)));
    }

    #[tokio::test]
    async fn ack_stream_with_serialize_error() {
        let serde_error = serde_json::from_str::<usize>("\"test\"").unwrap_err();
        let stream: AckStream<String> = AckStream::from(serde_error);

        futures::pin_mut!(stream);

        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Serde(_)
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_fut_with_serialize_error() {
        let serde_error = serde_json::from_str::<usize>("\"test\"").unwrap_err();
        let stream: AckStream<String> = AckStream::from(serde_error);

        assert!(matches!(stream.await.unwrap_err(), AckError::Serde(_)));
    }

    #[tokio::test]
    async fn broadcast_ack_with_closed_socket() {
        let socket = create_socket();
        let socket2 = create_socket();
        let mut packet = Packet::event("/", "test", "test".into());
        packet.inner.set_ack_id(1);
        let socks = vec![socket.clone().into(), socket2.clone().into()];
        let stream: AckStream<String> = AckInnerStream::broadcast(packet, socks, None).into();

        let res_packet = Packet::ack("test", "test".into(), 1);
        socket.recv(res_packet.inner.clone()).unwrap();

        futures::pin_mut!(stream);

        assert!(matches!(stream.next().await.unwrap(), Ok(_)));

        socket2.disconnect().unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Socket(SocketError::Closed)
        ));
        assert!(stream.next().await.is_none());
    }
    #[tokio::test]
    async fn ack_stream_with_closed_socket() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        drop(tx);

        futures::pin_mut!(stream);

        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Socket(SocketError::Closed)
        ));
    }

    #[tokio::test]
    async fn ack_fut_with_closed_socket() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_secs(1)).into();
        drop(tx);

        assert!(matches!(
            stream.await.unwrap_err(),
            AckError::Socket(SocketError::Closed)
        ));
    }

    #[tokio::test]
    async fn broadcast_ack_with_timeout() {
        let socket = create_socket();
        let socket2 = create_socket();
        let mut packet = Packet::event("/", "test", "test".into());
        packet.inner.set_ack_id(1);
        let socks = vec![socket.clone().into(), socket2.clone().into()];
        let stream: AckStream<String> =
            AckInnerStream::broadcast(packet, socks, Some(Duration::from_millis(10))).into();

        socket
            .recv(Packet::ack("test", "test".into(), 1).inner)
            .unwrap();

        futures::pin_mut!(stream);

        assert!(matches!(stream.next().await.unwrap(), Ok(_)));
        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Timeout
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ack_stream_with_timeout() {
        let (_tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_millis(10)).into();

        futures::pin_mut!(stream);

        assert!(matches!(
            stream.next().await.unwrap().unwrap_err(),
            AckError::Timeout
        ));
    }

    #[tokio::test]
    async fn ack_fut_with_timeout() {
        let (_tx, rx) = tokio::sync::oneshot::channel();
        let stream: AckStream<String> = AckInnerStream::send(rx, Duration::from_millis(10)).into();

        assert!(matches!(stream.await.unwrap_err(), AckError::Timeout));
    }
}
