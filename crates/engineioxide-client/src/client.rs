use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, Waker, ready},
};

use engineioxide_core::{OpenPacket, Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use tracing::Level;

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
        sink_waker: Option<Waker>,

        open_packet: OpenPacket,
        available_transports: Vec<TransportType>,
        state: ClientState,
        should_send_pong: bool,
        should_flush: bool,
    }
}

#[derive(Debug)]
enum ClientState {
    Open,      // connected; owe the caller a Connect event
    Upgrading, // driving the ws upgrade handshake
    Running,   // steady state
    Closing,   // draining the transport toward close
    Closed,
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
        let (inner, open_packet) = WsTransport::connect(svc.clone()).await?;
        let transport = Transport::Websocket { inner };
        let client = Client {
            transport,
            sink_waker: None,
            state: ClientState::Open,
            open_packet,
            available_transports: vec![], // move to a client config

            //TODO: move upgrade to transport.
            should_flush: false,
            should_send_pong: false,
        };

        Ok(client)
    }

    pub async fn connect(
        svc: impl Into<S>,
        transports: &[TransportType],
    ) -> Result<Self, PollingTransportError<S>> {
        let svc = svc.into();
        let (inner, open_packet) = PollingTransport::connect(svc).await?;
        let transport = Transport::Polling { inner };
        let client = Client {
            transport,
            open_packet,
            sink_waker: None,
            state: ClientState::Open,
            available_transports: transports.to_vec(),

            should_flush: false,
            should_send_pong: false,
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

impl<S: TransportSvc> Client<S> {
    pub fn sid(&self) -> Sid {
        self.open_packet.sid
    }

    fn should_upgrade(&self) -> bool {
        self.open_packet
            .upgrades
            .contains(&TransportType::Websocket)
            && self
                .available_transports
                .contains(&TransportType::Websocket)
    }
}

impl<S: TransportSvc> Stream for Client<S> {
    type Item = Result<EioEvent, TransportError<S>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.should_send_pong {
            ready!(self.as_mut().heartbeat(cx))?;
        }

        if self.should_flush {
            ready!(self.as_mut().flush(cx))?;
        }

        match self.state {
            ClientState::Open => {
                *self.as_mut().project().state = if self.should_upgrade() {
                    ClientState::Upgrading
                } else {
                    ClientState::Running
                };
                Poll::Ready(Some(Ok(EioEvent::Connect(self.open_packet.sid))))
            }
            ClientState::Upgrading => self.poll_upgrade(cx),
            ClientState::Running => self.poll_transport(cx),
            ClientState::Closing => {
                ready!(self.as_mut().project().transport.poll_close(cx))?;
                *self.as_mut().project().state = ClientState::Closed;
                Poll::Ready(None)
            }
            ClientState::Closed => Poll::Ready(None),
        }
    }
}

impl<S: TransportSvc> Client<S> {
    fn poll_transport(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, TransportError<S>>>> {
        let proj = self.as_mut().project();
        match ready!(proj.transport.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                *proj.should_send_pong = true;
                self.poll_next(cx)
            }
            Some(Ok(Packet::Close)) => {
                *proj.state = ClientState::Closing;
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

    fn poll_upgrade(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, TransportError<S>>>> {
        let sid = self.sid();
        let proj = self.as_mut().project();
        match ready!(proj.transport.upgrade(cx, sid)) {
            Some(Ok(())) => {
                tracing::debug!(%sid, "websocket transport upgraded");
                *proj.state = ClientState::Running;
                if let Some(waker) = proj.sink_waker.take() {
                    tracing::debug!("waking up sink after end of upgrade");
                    waker.wake();
                }

                cx.waker().wake_by_ref();
                Poll::Ready(Some(Ok(EioEvent::Upgrade(self.transport()))))
            }
            //TODO: handle upgrade failures gracefully
            Some(Err(e)) => {
                *proj.state = ClientState::Closed;
                Poll::Ready(Some(Err(e)))
            }
            None => {
                *proj.state = ClientState::Closed;
                Poll::Ready(None)
            } // TODO: fallback to polling if upgrade fails,
        }
    }
}

impl<S: TransportSvc> Sink<EioEvent> for Client<S> {
    type Error = <Transport<S> as Sink<Packet>>::Error;

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Open | ClientState::Upgrading => {
                // save waker to wake the task when client is running.
                self.project().sink_waker.replace(cx.waker().clone());
                Poll::Pending
            }
            ClientState::Running => self.project().transport.poll_ready(cx),
            ClientState::Closing => todo!("err, transport is closing"),
            ClientState::Closed => todo!("err, transport closed"),
        }
    }

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn start_send(self: Pin<&mut Self>, event: EioEvent) -> Result<(), Self::Error> {
        if let Some(packet) = event.into() {
            self.project().transport.start_send(packet)?;
        }
        Ok(())
    }

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport.poll_flush(cx)
    }

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport.poll_close(cx)
    }
}

impl<S: TransportSvc> fmt::Debug for Client<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").finish()
    }
}
