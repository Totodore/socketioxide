//! Flavors are transport abstractions that provide a common interface for different transport implementations.
//!
//! It is possible to implement a custom transport flavor by implementing the
//! [`Flavor`] subtraits directly.

use core::fmt;
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use futures_core::Stream;
use futures_util::Sink;
use http::{Request, Response};
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

/// Body of any polling request
pub struct PollingBody {
    /// Queued packets and their separators,
    /// streamed as they were queued without copying them into a single buffer.
    ///
    /// Every frame is one buffer of the list (a packet or a separator), handed
    /// out by reference count.
    inner: BufList<Bytes>,
    /// Bytes left to yield, kept aside so the size hint and end-of-stream
    /// checks are O(1) instead of walking the list.
    remaining: u64,
}

impl PollingBody {
    pub(crate) fn new(inner: BufList<Bytes>) -> Self {
        let remaining = inner.remaining() as u64;
        Self { inner, remaining }
    }
    pub(crate) fn new_empty() -> Self {
        Self::new(BufList::default())
    }
}

impl http_body::Body for PollingBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        // the chunk is the whole front buffer: taking exactly its length pops
        // it from the list without copying (`Bytes::copy_to_bytes` splits).
        let len = this.inner.chunk().len();
        if len == 0 {
            debug_assert_eq!(this.remaining, 0);
            return Poll::Ready(None);
        }
        let chunk = this.inner.copy_to_bytes(len);
        this.remaining -= len as u64;
        Poll::Ready(Some(Ok(http_body::Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.remaining)
    }
}

impl fmt::Debug for PollingBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollingBody")
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
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
    use std::{pin::pin, ptr};

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

    /// Each queued buffer is yielded as one frame, in order, without copying.
    #[test]
    fn polling_body_yields_queued_buffers_without_copy() {
        let list = payload(&["4hello", "\x1e", "4world"]);
        let ptrs: Vec<*const u8> = ["4hello", "\x1e", "4world"]
            .iter()
            .map(|c| c.as_ptr())
            .collect();
        let mut body = pin!(PollingBody::new(list));
        assert_eq!(body.size_hint().exact(), Some(13));
        assert!(!body.is_end_stream());

        let mut yielded = Vec::new();
        while let Some(frame) = body
            .as_mut()
            .frame()
            .now_or_never()
            .expect("the body is always ready")
        {
            let data = frame.unwrap().into_data().unwrap();
            assert_eq!(
                body.size_hint().exact(),
                Some(13 - yielded.iter().map(Bytes::len).sum::<usize>() as u64 - data.len() as u64)
            );
            yielded.push(data);
        }
        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
        assert_eq!(yielded.len(), 3, "one frame per queued buffer");
        for (data, ptr) in yielded.iter().zip(ptrs) {
            assert!(
                ptr::eq(data.as_ptr(), ptr),
                "frame must reuse the queued buffer"
            );
        }
        assert_eq!(yielded.concat(), b"4hello\x1e4world");
    }

    #[test]
    fn polling_body_collects_to_the_full_payload() {
        let body = PollingBody::new(payload(&["4a", "\x1e", "4b", "\x1e", "4c"]));
        let collected = body.collect().now_or_never().unwrap().unwrap().to_bytes();
        assert_eq!(collected, "4a\x1e4b\x1e4c");
    }

    #[test]
    fn empty_polling_body_ends_immediately() {
        let mut body = pin!(PollingBody::new(BufList::new()));
        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
        assert!(body.as_mut().frame().now_or_never().unwrap().is_none());
    }
}
