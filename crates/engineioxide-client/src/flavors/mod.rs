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
use http_body_util::combinators::BoxBody;
use tower_service::Service;

use engineioxide_core::{Str, TransportType};

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
        Request<BoxBody<Bytes, Infallible>>,
        Response = Response<Self::Body>,
        Error = <Self as PollingSvc>::Error,
        Future: Unpin, // Unpin bound so we can move transports around when upgrading
    >
{
    type Body: http_body::Body<Error = Self::ResBodyError> + 'static;
    type Error: fmt::Debug + std::error::Error;
    type ResBodyError: fmt::Debug + std::error::Error + 'static;
}

impl<B, S> PollingSvc for S
where
    S: Service<Request<BoxBody<Bytes, Infallible>>, Response = Response<B>>,
    <S as Service<Request<BoxBody<Bytes, Infallible>>>>::Future: Unpin,
    <S as Service<Request<BoxBody<Bytes, Infallible>>>>::Error: fmt::Debug + std::error::Error,
    B: http_body::Body + 'static,
    <B as http_body::Body>::Error: fmt::Debug + std::error::Error + 'static,
    <B as http_body::Body>::Data: Send + fmt::Debug + 'static,
{
    type Body = B;
    type Error = <S as Service<Request<BoxBody<Bytes, Infallible>>>>::Error;
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
    Text(Str),
    Binary(Bytes),
    Close,
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
