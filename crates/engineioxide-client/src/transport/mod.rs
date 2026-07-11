use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use engineioxide_core::{Packet, TransportType};
use futures_core::Stream;
use futures_util::Sink;

pub use crate::transport::polling::{PollingSvc, PollingTransport, PollingTransportError};
pub use crate::transport::ws::{
    WebSocket, WsTransport, WsTransportError, noop_impl, tungstenite_impl,
};

mod polling;
mod ws;

pub enum TransportError<S: PollingSvc, WS: WebSocket> {
    Polling(PollingTransportError<S>),
    Websocket(WsTransportError<WS>),
}

impl<S: PollingSvc, WS: WebSocket> fmt::Debug for TransportError<S, WS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}
impl<S: PollingSvc, WS: WebSocket> fmt::Display for TransportError<S, WS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Polling(e) => write!(f, "polling error: {}", e),
            TransportError::Websocket(e) => write!(f, "ws error: {}", e),
        }
    }
}
impl<S: PollingSvc, WS: WebSocket> std::error::Error for TransportError<S, WS> {}

pin_project_lite::pin_project! {
    #[project = TransportProj]
    pub enum Transport<S: PollingSvc, WS> {
        Polling {
            #[pin]
            inner: PollingTransport<S>
        },
        Websocket {
            #[pin]
            inner: WsTransport<WS>
        }
    }
}

impl<S: PollingSvc, WS> Transport<S, WS> {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling { .. } => TransportType::Polling,
            Transport::Websocket { .. } => TransportType::Websocket,
        }
    }
}

impl<S: PollingSvc, WS: WebSocket> Stream for Transport<S, WS> {
    type Item = Result<Packet, TransportError<S, WS>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project() {
            TransportProj::Polling { inner } => {
                inner.poll_next(cx).map_err(TransportError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.poll_next(cx).map_err(TransportError::Websocket)
            }
        }
    }
}
impl<S: PollingSvc, WS: WebSocket> Sink<Packet> for Transport<S, WS> {
    type Error = TransportError<S, WS>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => {
                inner.poll_ready(cx).map_err(TransportError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.poll_ready(cx).map_err(TransportError::Websocket)
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.project() {
            TransportProj::Polling { inner } => {
                inner.start_send(item).map_err(TransportError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.start_send(item).map_err(TransportError::Websocket)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => {
                inner.poll_flush(cx).map_err(TransportError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.poll_flush(cx).map_err(TransportError::Websocket)
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => {
                inner.poll_close(cx).map_err(TransportError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.poll_close(cx).map_err(TransportError::Websocket)
            }
        }
    }
}
