use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, Sid};
use futures_core::Stream;
use futures_util::Sink;

use crate::{
    EioEvent,
    transport::{
        PollingSvc, PollingTransport, PollingTransportError, Transport, TransportError, WebSocket,
        WsTransport, WsTransportError, noop_impl::NoopWebSocket,
    },
};

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc, WS > {
        #[pin]
        pub transport: Transport<S, WS>,

        should_send_pong: bool,
        should_flush: bool,
        closing: bool,
        pub sid: Sid,
    }
}

impl<S: PollingSvc, WS: WebSocket> Client<S, WS> {
    pub async fn connect_ws(ws: impl Into<WS>) -> Result<Self, WsTransportError<WS>> {
        let (inner, open) = WsTransport::connect(ws).await?;
        let transport = Transport::Websocket { inner };
        let client = Client {
            transport,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
            closing: false,
        };

        Ok(client)
    }
}
impl<S: PollingSvc> Client<S, NoopWebSocket> {
    pub async fn connect(svc: S) -> Result<Self, PollingTransportError<S>> {
        let (inner, open) = PollingTransport::connect(svc).await?;
        let transport = Transport::Polling { inner };
        let client = Client {
            transport,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
            closing: false,
        };

        Ok(client)
    }
}

impl<S: PollingSvc, WS: WebSocket> Client<S, WS> {
    #[tracing::instrument(skip(cx))]
    fn heartbeat(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError<S, WS>>> {
        let mut proj = self.project();
        ready!(proj.transport.as_mut().poll_ready(cx))?;
        proj.transport.as_mut().start_send(Packet::Pong)?;

        *proj.should_send_pong = false;
        *proj.should_flush = true;
        Poll::Ready(Ok(()))
    }

    #[tracing::instrument(skip(cx))]
    fn flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError<S, WS>>> {
        let mut proj = self.project();
        ready!(proj.transport.as_mut().poll_flush(cx))?;
        *proj.should_flush = false;
        Poll::Ready(Ok(()))
    }
}

impl<S: PollingSvc, WS: WebSocket> Stream for Client<S, WS> {
    type Item = Result<EioEvent, TransportError<S, WS>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closing {
            ready!(self.project().transport.poll_close(cx))?;
            return Poll::Ready(None);
        }

        if self.should_send_pong {
            ready!(self.as_mut().heartbeat(cx))?;
        }

        if self.should_flush {
            ready!(self.as_mut().flush(cx))?;
        }

        match ready!(self.as_mut().project().transport.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                *self.as_mut().project().should_send_pong = true;
                self.poll_next(cx)
            }
            Some(Ok(Packet::Close)) => {
                *self.as_mut().project().closing = true;
                cx.waker().wake_by_ref(); // wake up to close the transport
                Poll::Ready(Some(Ok(EioEvent::Disconnect)))
            }
            Some(Ok(Packet::Message(v))) => Poll::Ready(Some(Ok(EioEvent::Message(v)))),
            Some(Ok(Packet::Binary(v))) => Poll::Ready(Some(Ok(EioEvent::Binary(v)))),
            Some(Ok(v)) => unreachable!("unexpected msg {v:?}"),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

impl<S: PollingSvc, WS: WebSocket> Sink<EioEvent> for Client<S, WS> {
    type Error = <Transport<S, WS> as Sink<Packet>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, event: EioEvent) -> Result<(), Self::Error> {
        if let Some(packet) = event.into() {
            self.project().transport.start_send(packet)?;
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport.poll_close(cx)
    }
}

impl<S: PollingSvc, WS: WebSocket> fmt::Debug for Client<S, WS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("should_send_pong", &self.should_send_pong)
            .field("should_flush", &self.should_flush)
            .field("sid", &self.sid)
            .finish()
    }
}
