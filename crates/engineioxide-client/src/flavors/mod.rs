//! Flavors are transport abstractions that provide a common interface for different transport implementations.
//!
//! It is possible to implement a custom transport flavor by implementing the
//! [`Flavor`] subtraits directly.

use core::fmt;
use std::convert::Infallible;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::Sink;
use http::{Request, Response};
use http_body_util::Full;
use pin_project_lite::pin_project;
use tower_service::Service;

use engineioxide_core::{Str, TransportType, payload::BufList};

#[cfg(feature = "flavor-hyper")]
pub mod hyper;

#[cfg(feature = "flavor-tungstenite")]
pub mod hyper_tungstenite;

#[cfg(feature = "flavor-testing")]
pub mod testing;

/// Trait alias for a transport service.
pub trait TransportSvc: PollingSvc + WsSvc + Flavor {}
impl<S> TransportSvc for S where S: PollingSvc + WsSvc + Flavor {}

/// A trait that represents a flavor of the engineioxide client.
pub trait Flavor {
    /// The list of supported transport types for this flavor.
    const SUPPORTED_TRANSPORTS: &'static [TransportType];
}

/// Trait alias for a polling service.
pub trait PollingSvc:
    Service<
        Request<PollingBody>,
        Response = Response<Self::Body>,
        Error = <Self as PollingSvc>::Error,
        Future: Unpin, // Unpin bound so we can move transports around when upgrading
    >
{
    /// Response body type for the polling service.
    type Body: http_body::Body<Error = Self::ResBodyError> + 'static;
    /// Error type for the polling service.
    type Error: fmt::Debug + std::error::Error;
    /// Response body error type for the polling service.
    type ResBodyError: fmt::Debug + std::error::Error + 'static;
}

impl<B, S> PollingSvc for S
where
    S: Service<Request<PollingBody>, Response = Response<B>>,
    <S as Service<Request<PollingBody>>>::Future: Unpin,
    <S as Service<Request<PollingBody>>>::Error: fmt::Debug + std::error::Error,
    B: http_body::Body + 'static,
    <B as http_body::Body>::Error: fmt::Debug + std::error::Error + 'static,
    <B as http_body::Body>::Data: Send + fmt::Debug + 'static,
{
    type Body = B;
    type Error = <S as Service<Request<PollingBody>>>::Error;
    type ResBodyError = <B as http_body::Body>::Error;
}

/// Trait alias for a websocket service.
pub trait WsSvc:
    Service<
        http::Request<()>,
        Response = Self::WebSocket,
        Error = <Self as WsSvc>::Error,
        Future: Unpin, // Unpin bound so we can move transports around when upgrading
    > + Clone
{
    /// Error type for the websocket service.
    type Error: fmt::Debug + std::error::Error;
    /// The WebSocket type that this service uses.
    type WebSocket: WebSocket<Error = <Self as WsSvc>::Error>;
}

impl<S, WS> WsSvc for S
where
    S: Service<http::Request<()>, Response = WS, Future: Unpin> + Clone,
    WS: WebSocket<Error = <S as Service<http::Request<()>>>::Error>,
    <S as Service<http::Request<()>>>::Error: fmt::Debug + std::error::Error,
{
    type Error = <S as Service<http::Request<()>>>::Error;
    type WebSocket = WS;
}

/// Trait alias for a WebSocket.
pub trait WebSocket:
    Stream<Item = Result<WsMessage, <Self as WebSocket>::Error>>
    + Sink<WsMessage, Error = <Self as WebSocket>::Error>
    + Sized
    + Unpin
{
    /// Error type for the WebSocket.
    type Error: fmt::Debug + std::error::Error;
}

impl<St, E> WebSocket for St
where
    St: Stream<Item = Result<WsMessage, E>> + Sink<WsMessage, Error = E> + Sized + Unpin,
    E: fmt::Debug + std::error::Error,
{
    type Error = E;
}

/// A WebSocket message.
pub enum WsMessage {
    /// A text message.
    Text(Str),
    /// A binary message.
    Binary(Bytes),
    /// A close message.
    Close,
}

pin_project! {
    /// The body of a polling request: the queued packets and their separators,
    /// sent as one frame straight from the [`BufList`] they were queued in.
    /// hyper writes the list vectored, so nothing is copied into a single
    /// buffer.
    #[derive(Debug, Default)]
    pub struct PollingBody {
        #[pin]
        inner: Full<BufList<Bytes>>,
    }
}

impl PollingBody {
    /// A body made of the given buffers.
    pub(crate) fn new(inner: BufList<Bytes>) -> Self {
        Self {
            inner: Full::new(inner),
        }
    }

    /// An empty body (e.g. for polling GET requests).
    pub(crate) fn new_empty() -> Self {
        Self::default()
    }
}

impl http_body::Body for PollingBody {
    type Data = BufList<Bytes>;
    type Error = Infallible;

    #[inline]
    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.project().inner.poll_frame(cx)
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    #[inline]
    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

/// A no-op WebSocket implementation to satisfy the [`WebSocket`] trait when implementing
/// a flavor that does not support WebSocket connections.
pub mod noop {
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures_core::Stream;
    use futures_util::Sink;

    /// A stub, no-op WebSocket implementation for use in flavors that do not
    /// support WebSocket connections.
    #[derive(Debug, Default, Clone)]
    pub struct NoopWebSocket;

    impl Stream for NoopWebSocket {
        type Item = Result<super::WsMessage, Infallible>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(None)
        }
    }

    impl Sink<super::WsMessage> for NoopWebSocket {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _item: super::WsMessage) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::IoSlice, pin::pin, ptr};

    use bytes::Buf;
    use futures_util::FutureExt;
    use http_body::Body;
    use http_body_util::BodyExt;

    use super::*;

    fn payload(chunks: &[&'static str]) -> BufList<Bytes> {
        let mut payload = BufList::new();
        for chunk in chunks {
            payload.push(Bytes::from_static(chunk.as_bytes()));
        }
        payload
    }

    /// The queued buffers are sent as a single frame that still points to
    /// the original buffers: nothing is copied.
    #[test]
    fn polling_body_yields_queued_buffers_without_copy() {
        let chunks = ["4hello", "\x1e", "4world"];
        let mut body = pin!(PollingBody::new(payload(&chunks)));
        assert_eq!(body.size_hint().exact(), Some(13));
        assert!(!body.is_end_stream());

        let frame = body
            .as_mut()
            .frame()
            .now_or_never()
            .expect("the body is always ready")
            .expect("one frame")
            .unwrap();
        let data = frame.into_data().unwrap();
        assert_eq!(data.remaining(), 13);

        let mut slices = [IoSlice::new(&[]); 4];
        assert_eq!(
            data.chunks_vectored(&mut slices),
            3,
            "one slice per queued buffer"
        );
        for (slice, chunk) in slices.iter().zip(chunks) {
            assert_eq!(&slice[..], chunk.as_bytes());
            assert!(
                ptr::eq(slice.as_ptr(), chunk.as_ptr()),
                "the frame must reuse the queued buffer"
            );
        }

        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
        assert!(body.as_mut().frame().now_or_never().unwrap().is_none());
    }

    #[test]
    fn polling_body_collects_to_the_full_payload() {
        let body = PollingBody::new(payload(&["4a", "\x1e", "4b", "\x1e", "4c"]));
        let collected = body.collect().now_or_never().unwrap().unwrap().to_bytes();
        assert_eq!(collected, "4a\x1e4b\x1e4c");
    }

    #[test]
    fn empty_polling_body_ends_immediately() {
        for mut body in [pin!(PollingBody::new_empty()), pin!(Bytes::new().into())] {
            assert!(body.is_end_stream());
            assert_eq!(body.size_hint().exact(), Some(0));
            assert!(body.as_mut().frame().now_or_never().unwrap().is_none());
        }
    }
}
