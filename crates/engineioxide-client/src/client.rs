use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, Waker, ready},
    time::Instant,
};

use engineioxide_core::{OpenPacket, Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use tracing::Level;

use crate::{
    EngineIoClientConfig,
    config::IntoEngineIoClientConfig,
    errors::{ClientError, ConnectError},
    event::EioEvent,
    flavors,
    transport::{Transport, TransportSvc, WsTransport, polling::PollingTransport},
};

pin_project_lite::pin_project! {
    pub struct Client<S: TransportSvc> {
        #[pin]
        transport: Transport<S>,
        sink_waker: Option<Waker>,
        config: EngineIoClientConfig,

        open_packet: OpenPacket,
        last_ping: Instant,
        state: ClientState,
        pending_pong: bool,
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

impl Client<flavors::hyper_tungstenite::HyperTungsteniteFlavor> {
    pub async fn connect_with_hyper_ws(
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<flavors::hyper_tungstenite::HyperTungsteniteFlavor>> {
        let svc = flavors::hyper_tungstenite::HyperTungsteniteFlavor::new();
        Self::connect(svc, config).await
    }
}

impl<Svc: flavors::testing::EngineSvc> Client<flavors::testing::TestingFlavor<Svc>> {
    pub async fn connect_with_testbed(
        svc: Svc,
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<flavors::testing::TestingFlavor<Svc>>> {
        let svc = flavors::testing::TestingFlavor::new(svc);
        Self::connect(svc, config).await
    }
}

impl<S: TransportSvc> Client<S> {
    pub async fn connect(
        svc: S,
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<S>> {
        let config = config.into_config()?;
        let (transport, open_packet) = Self::connect_inner(svc, &config).await?;

        let client = Client {
            transport,
            open_packet,
            config,
            sink_waker: None,
            last_ping: Instant::now(),
            state: ClientState::Open,
            pending_pong: false,
        };

        Ok(client)
    }

    async fn connect_inner(
        svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Transport<S>, OpenPacket), ClientError<S>> {
        let (transport, packet) = match config.initial_transport() {
            TransportType::Polling => {
                let (transport, open_packet) = PollingTransport::connect(svc, config).await?;
                (transport.into(), open_packet)
            }
            TransportType::Websocket => {
                let (transport, open_packet) = WsTransport::connect(svc, config).await?;
                (transport.into(), open_packet)
            }
        };

        Ok((transport, packet))
    }

    pub fn transport(&self) -> TransportType {
        self.transport.transport_type()
    }
}

impl<S: TransportSvc> Client<S> {
    pub fn sid(&self) -> Sid {
        self.open_packet.sid
    }

    fn should_upgrade(&self) -> bool {
        self.transport() != TransportType::Websocket
            && self
                .open_packet
                .upgrades
                .contains(&TransportType::Websocket)
            && self.config.transports.contains(&TransportType::Websocket)
    }
}

impl<S: TransportSvc> Stream for Client<S> {
    type Item = Result<EioEvent, ClientError<S>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().poll_next_inner(cx)) {
            Some(Ok(item)) => Poll::Ready(Some(Ok(item))),
            Some(Err(err)) if err.should_close() => {
                // hard closing on errors
                *self.project().state = ClientState::Closed;
                Poll::Ready(Some(Err(err)))
            }
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }
}

impl<S: TransportSvc> Client<S> {
    fn poll_next_inner(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, ClientError<S>>>> {
        let _ = self.as_mut().poll_heartbeat(cx);

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
            ClientState::Closed | ClientState::Closing => Poll::Ready(None),
        }
    }

    fn poll_transport(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, ClientError<S>>>> {
        let proj = self.as_mut().project();
        match ready!(proj.transport.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                *proj.pending_pong = true;
                *proj.last_ping = Instant::now();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Ok(Packet::Close)) => {
                *proj.state = ClientState::Closing;
                cx.waker().wake_by_ref(); // wake up to close the transport
                Poll::Ready(Some(Ok(EioEvent::Disconnect)))
            }
            Some(Ok(Packet::Message(v))) => Poll::Ready(Some(Ok(EioEvent::Message(v)))),
            Some(Ok(Packet::Binary(v) | Packet::BinaryV3(v))) => {
                Poll::Ready(Some(Ok(EioEvent::Binary(v))))
            }
            Some(Ok(Packet::Noop)) => {
                // ignore noop packets
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Ok(p)) => Poll::Ready(Some(Err(ClientError::InvalidPacket(p)))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }

    fn poll_upgrade(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, ClientError<S>>>> {
        let sid = self.sid();
        let proj = self.as_mut().project();
        match ready!(proj.transport.upgrade(cx, proj.config, sid)) {
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

    fn poll_heartbeat(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), ClientError<S>>> {
        if self.last_ping.elapsed()
            >= self.open_packet.ping_interval + self.open_packet.ping_timeout
        {
            todo!("error + closing + better wake");
            // self.close();
            // Err()
        }

        let mut proj = self.project();
        if *proj.pending_pong {
            ready!(proj.transport.as_mut().poll_ready(cx))?;
            *proj.pending_pong = false;
            proj.transport.as_mut().start_send(Packet::Pong)?;
        }

        // idempotent: continues an in-flight flush, or Ready immediately if clean
        // TODO: check this assertion
        proj.transport.poll_flush(cx)
    }
}

impl<S: TransportSvc> Sink<EioEvent> for Client<S> {
    type Error = ClientError<S>;

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Upgrading => {
                // save waker to wake the task when transport is migrated.
                self.project().sink_waker.replace(cx.waker().clone());
                Poll::Pending
            }
            ClientState::Open | ClientState::Running => self
                .project()
                .transport
                .poll_ready(cx)
                .map_err(ClientError::from),
            ClientState::Closing => Poll::Ready(Err(ClientError::TransportClosed)),
            ClientState::Closed => Poll::Ready(Err(ClientError::TransportClosed)),
        }
    }

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn start_send(self: Pin<&mut Self>, event: EioEvent) -> Result<(), Self::Error> {
        if let Some(packet) = event.into() {
            self.project().transport.start_send(packet)?;
        }
        Ok(())
    }

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .transport
            .poll_flush(cx)
            .map_err(ClientError::from)
    }

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let proj = self.project();
        match *proj.state {
            ClientState::Open | ClientState::Upgrading | ClientState::Running => {
                *proj.state = ClientState::Closing;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            ClientState::Closing => {
                ready!(proj.transport.poll_close(cx))?;
                *proj.state = ClientState::Closed;
                Poll::Ready(Ok(()))
            }
            ClientState::Closed => Poll::Ready(Ok(())),
        }
    }
}

impl<S: TransportSvc> fmt::Debug for Client<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("transport", &self.transport)
            .field("sink_waker", &self.sink_waker)
            .field("config", &self.config)
            .field("open_packet", &self.open_packet)
            .field("last_ping", &self.last_ping)
            .field("state", &self.state)
            .field("pending_pong", &self.pending_pong)
            .finish()
    }
}
