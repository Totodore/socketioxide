use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, Waker, ready},
    time::Instant,
};

use engineioxide_core::{OpenPacket, Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::{Sink, SinkExt};
use http::uri;
use thiserror::Error;
use tracing::Level;

use crate::{
    EngineIoClientConfig,
    config::IntoEngineIoClientConfig,
    event::EioEvent,
    flavors,
    transport::{Transport, TransportError, TransportSvc, WsTransport, polling::PollingTransport},
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

#[derive(Debug, Error)]
pub enum ConnectError<S: TransportSvc> {
    #[error(transparent)]
    Transport(TransportError<S>),
    #[error("failed to build client, invalid uri: {0}")]
    Config(#[from] uri::InvalidUri),
}
impl<S: TransportSvc> From<TransportError<S>> for ConnectError<S> {
    fn from(value: TransportError<S>) -> Self {
        Self::Transport(value)
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
    ) -> Result<(Transport<S>, OpenPacket), TransportError<S>> {
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
    type Item = Result<EioEvent, TransportError<S>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
    ) -> Poll<Result<(), TransportError<S>>> {
        if self.last_ping.elapsed()
            <= self.open_packet.ping_interval + self.open_packet.ping_timeout
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
        proj.transport.poll_flush(cx)
    }
}

impl<S: TransportSvc> Sink<EioEvent> for Client<S> {
    type Error = <Transport<S> as Sink<Packet>>::Error;

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Upgrading => {
                // save waker to wake the task when transport is migrated.
                self.project().sink_waker.replace(cx.waker().clone());
                Poll::Pending
            }
            ClientState::Open | ClientState::Running => self.project().transport.poll_ready(cx),
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
        f.debug_struct("Client")
            .field("sink_waker", &self.sink_waker)
            .field("state", &self.state)
            .field("open_packet", &self.open_packet)
            .field("config", &self.config)
            .finish()
    }
}
