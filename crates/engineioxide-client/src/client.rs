use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;

use crate::{
    event::EioEvent,
    flavors::hyper_tungstenite::HyperTungsteniteFlavor,
    transport::{
        Transport, TransportError, TransportSvc,
        polling::{PollingTransport, PollingTransportError},
        ws::{WsTransport, WsTransportError},
    },
};

pin_project_lite::pin_project! {
    pub struct Client<S: TransportSvc> {
        #[pin]
        transport: Transport<S>,

        should_send_pong: bool,
        should_flush: bool,
        closing: bool,
        should_upgrade: bool,
        pub sid: Sid,
    }
}

impl Client<HyperTungsteniteFlavor> {
    pub async fn connect_with_hyper() -> Result<Self, PollingTransportError<HyperTungsteniteFlavor>>
    {
        let svc = HyperTungsteniteFlavor::new();
        Self::connect_default(svc).await
    }
}

impl<S: TransportSvc> Client<S> {
    pub async fn connect_default(svc: S) -> Result<Self, PollingTransportError<S>> {
        Self::connect(svc, &[TransportType::Polling, TransportType::Websocket]).await
    }
    pub async fn connect_ws(svc: S) -> Result<Self, WsTransportError<S>> {
        let (inner, open) = WsTransport::connect(svc.clone()).await?;
        let transport = Transport::Websocket { inner };
        let client = Client {
            transport,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
            should_upgrade: false,
            closing: false,
        };

        Ok(client)
    }

    pub async fn connect(
        svc: impl Into<S>,
        transports: &[TransportType],
    ) -> Result<Self, PollingTransportError<S>> {
        let svc = svc.into();
        let (inner, open) = PollingTransport::connect(svc).await?;
        let transport = Transport::Polling { inner };
        let client = Client {
            transport,
            sid: open.sid,

            should_flush: false,
            should_send_pong: false,
            closing: false,
            should_upgrade: transports.contains(&TransportType::Websocket)
                && open.upgrades.contains(&TransportType::Websocket),
        };

        Ok(client)
    }

    pub fn transport(&self) -> TransportType {
        self.transport.transport_type()
    }
}

impl<S: TransportSvc> Client<S> {
    pub async fn connect_polling(svc: S) -> Result<Self, PollingTransportError<S>> {
        Self::connect(svc, &[TransportType::Polling]).await
    }
}

impl<S: TransportSvc> Client<S> {
    #[tracing::instrument(skip(cx))]
    fn heartbeat(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError<S>>> {
        let mut proj = self.project();
        ready!(proj.transport.as_mut().poll_ready(cx))?;
        proj.transport.as_mut().start_send(Packet::Pong)?;

        *proj.should_send_pong = false;
        *proj.should_flush = true;
        Poll::Ready(Ok(()))
    }

    #[tracing::instrument(skip(cx))]
    fn flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), TransportError<S>>> {
        let mut proj = self.project();
        ready!(proj.transport.as_mut().poll_flush(cx))?;
        *proj.should_flush = false;
        Poll::Ready(Ok(()))
    }
}

impl<S: TransportSvc> Stream for Client<S> {
    type Item = Result<EioEvent, TransportError<S>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closing {
            ready!(self.project().transport.poll_close(cx))?;
            return Poll::Ready(None);
        }

        if self.should_upgrade {
            let sid = self.sid;
            return match ready!(self.as_mut().project().transport.upgrade(cx, sid)) {
                Some(Ok(())) => {
                    tracing::debug!(%sid, "websocket transport upgraded");
                    *self.as_mut().project().should_upgrade = false;
                    cx.waker().wake_by_ref();
                    Poll::Ready(Some(Ok(EioEvent::Upgrade(self.transport()))))
                }
                Some(Err(e)) => Poll::Ready(Some(Err(e))),
                None => {
                    *self.as_mut().project().should_upgrade = false;
                    Poll::Ready(None)
                } // TODO: fallback to polling if upgrade fails,
            };
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

impl<S: TransportSvc> Sink<EioEvent> for Client<S> {
    type Error = <Transport<S> as Sink<Packet>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.should_upgrade || self.closing {
            return Poll::Pending;
        }

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

impl<S: TransportSvc> fmt::Debug for Client<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("should_send_pong", &self.should_send_pong)
            .field("should_flush", &self.should_flush)
            .field("sid", &self.sid)
            .finish()
    }
}
