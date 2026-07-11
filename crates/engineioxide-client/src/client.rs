use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, Sid};
use futures_core::Stream;
use futures_util::{
    Sink, StreamExt,
    stream::{SplitSink, SplitStream},
};

use crate::transport::{
    PollingSvc, PollingTransport, PollingTransportError, Transport, TransportError, WebSocket,
    WsTransport, WsTransportError, noop_impl::NoopWebSocket,
};

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc, WS > {
        #[pin]
        pub transport_rx: SplitStream<Transport<S, WS>>,
        #[pin]
        pub transport_tx: SplitSink<Transport<S, WS>, Packet>,

        should_send_pong: bool,
        should_flush: bool,
        pub sid: Sid,
    }
}

impl<S: PollingSvc, WS: WebSocket> Client<S, WS> {
    pub async fn connect_ws(ws: impl Into<WS>) -> Result<Self, WsTransportError<WS>> {
        let (inner, open) = WsTransport::connect(ws).await?;
        let transport = Transport::Websocket { inner };
        let (transport_tx, transport_rx) = transport.split();
        let client = Client {
            transport_tx,
            transport_rx,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
        };

        Ok(client)
    }
}
impl<S: PollingSvc> Client<S, NoopWebSocket> {
    pub async fn connect(svc: S) -> Result<Self, PollingTransportError<S>> {
        let (inner, open) = PollingTransport::connect(svc).await?;
        let transport = Transport::Polling { inner };
        let (transport_tx, transport_rx) = transport.split();
        let client = Client {
            transport_tx,
            transport_rx,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
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
        ready!(proj.transport_tx.as_mut().poll_ready(cx))?;
        proj.transport_tx.as_mut().start_send(Packet::Pong)?;

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
        ready!(proj.transport_tx.as_mut().poll_flush(cx))?;
        *proj.should_flush = false;
        Poll::Ready(Ok(()))
    }
}

impl<S: PollingSvc, WS: WebSocket> Stream for Client<S, WS> {
    type Item = Result<Packet, TransportError<S, WS>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.should_send_pong {
            ready!(self.as_mut().heartbeat(cx))?;
        }

        if self.should_flush {
            ready!(self.as_mut().flush(cx))?;
        }

        match ready!(self.as_mut().project().transport_rx.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                if self.as_mut().heartbeat(cx).is_pending() {
                    self.should_send_pong = true;
                }
                self.poll_next(cx)
            }
            Some(Ok(packet)) => Poll::Ready(Some(Ok(packet))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

impl<S: PollingSvc, WS: WebSocket> Sink<Packet> for Client<S, WS> {
    type Error = <Transport<S, WS> as Sink<Packet>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        self.project().transport_tx.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_close(cx)
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
