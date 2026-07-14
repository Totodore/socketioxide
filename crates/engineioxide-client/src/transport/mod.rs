use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use tracing::Level;

use crate::transport::{polling::PollingTransportError, ws::WsTransportError};

pub use polling::{PollingSvc, PollingTransport};
pub use ws::{WebSocket, WsSvc, WsTransport};

pub mod polling;
pub mod ws;

pub enum TransportError<S: TransportSvc> {
    Polling(PollingTransportError<S>),
    Websocket(WsTransportError<S>),
}

impl<S: TransportSvc> fmt::Debug for TransportError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}
impl<S: TransportSvc> fmt::Display for TransportError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Polling(e) => write!(f, "polling error: {}", e),
            TransportError::Websocket(e) => write!(f, "ws error: {}", e),
        }
    }
}
impl<S: TransportSvc> std::error::Error for TransportError<S> {}

pub trait TransportSvc: PollingSvc + WsSvc {}
impl<S: PollingSvc + WsSvc> TransportSvc for S {}

pin_project_lite::pin_project! {
    #[project = TransportProj]
    pub enum Transport<S: TransportSvc> {
        Polling {
            #[pin]
            inner: PollingTransport<S>
        },
        Websocket {
            #[pin]
            inner: WsTransport<S>
        }
    }
}

impl<S: TransportSvc> Transport<S> {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling { .. } => TransportType::Polling,
            Transport::Websocket { .. } => TransportType::Websocket,
        }
    }
}
impl<S: TransportSvc> Transport<S> {
    /// Upgrade the current transport to [`WsTransport`].
    ///
    /// Starts by flushing the polling transport, once done switch to websocket
    /// and drive the websocket protocol upgrade.
    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    pub fn upgrade(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        sid: Sid,
    ) -> Poll<Option<Result<(), TransportError<S>>>> {
        match self.as_mut().project() {
            TransportProj::Polling { mut inner } => {
                // start by flushing polling transport to ensure there is no pending data
                ready!(inner.as_mut().poll_flush(cx)).map_err(TransportError::Polling)?;
                let svc = inner.svc.clone();

                self.set(Transport::Websocket {
                    inner: WsTransport::connect_with_upgrade(svc, sid),
                });
                cx.waker().wake_by_ref();
                Poll::Pending
            }

            TransportProj::Websocket { inner } => match ready!(inner.poll_next(cx)) {
                Some(Ok(Packet::Upgrade)) => Poll::Ready(Some(Ok(()))),
                Some(Ok(p)) => todo!("handle err: {p:?}"),
                Some(Err(err)) => Poll::Ready(Some(Err(TransportError::Websocket(err)))),
                None => Poll::Ready(None),
            },
        }
    }
}

impl<S: TransportSvc> Stream for Transport<S> {
    type Item = Result<Packet, TransportError<S>>;

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
impl<S: TransportSvc> Sink<Packet> for Transport<S> {
    type Error = TransportError<S>;

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
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

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
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

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
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

impl<S: TransportSvc> fmt::Debug for Transport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Polling { inner } => f.debug_struct("Polling").field("inner", inner).finish(),
            Self::Websocket { inner } => f.debug_struct("Websocket").field("inner", inner).finish(),
        }
    }
}
