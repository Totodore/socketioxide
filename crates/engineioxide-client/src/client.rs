use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Instant,
};

use engineioxide_core::{OpenPacket, Packet, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use tracing::Level;

use crate::{
    EioEvent, EngineIoClientConfig,
    config::IntoEngineIoClientConfig,
    errors::{ClientError, ConfigError, ConnectError},
    flavors::TransportSvc,
    transport::Transport,
};

pin_project_lite::pin_project! {
    /// A client for the engine.io protocol.
    /// This is the main struct for interacting with the engine.io server.
    pub struct Client<S: TransportSvc> {
        #[pin]
        transport: Transport<S>,
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
    Upgrading, // probing the websocket upgrade; polling still flows
    Running,   // steady state
    Closing,   // draining the transport toward close
    Closed,
}

#[cfg(feature = "flavor-hyper")]
impl Client<crate::flavors::hyper::HyperFlavor> {
    /// Connects to the engine.io server using the hyper polling transport.
    pub async fn connect_with_hyper_polling(
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<crate::flavors::hyper::HyperFlavor>> {
        let svc = crate::flavors::hyper::HyperFlavor::new();

        // override transports to only use polling, as websocket is not
        // supported for the hyper flavor
        let mut config = config.into_config()?;
        config.transports = vec![TransportType::Polling];
        Self::connect(svc, config).await
    }
}

#[cfg(feature = "flavor-tungstenite")]
impl Client<crate::flavors::hyper_tungstenite::HyperTungsteniteFlavor> {
    /// Connects to the engine.io server using the hyper tungstenite transport.
    pub async fn connect_with_hyper_ws(
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<crate::flavors::hyper_tungstenite::HyperTungsteniteFlavor>> {
        let svc = crate::flavors::hyper_tungstenite::HyperTungsteniteFlavor::new();
        Self::connect(svc, config).await
    }
}

#[cfg(feature = "flavor-testing")]
#[expect(private_bounds)] // EngineSvc is simply a trait alias
impl<Svc: crate::flavors::testing::EngineSvc> Client<crate::flavors::testing::TestingFlavor<Svc>> {
    /// Connects to the engine.io server using the testing transport.
    pub async fn connect_with_testbed(
        svc: Svc,
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<crate::flavors::testing::TestingFlavor<Svc>>> {
        let svc = crate::flavors::testing::TestingFlavor::new(svc);
        Self::connect(svc, config).await
    }
}

impl<S: TransportSvc> Client<S> {
    /// Connects to the engine.io server using the given transport service and config.
    pub async fn connect(
        svc: S,
        config: impl IntoEngineIoClientConfig,
    ) -> Result<Self, ConnectError<S>> {
        let config = config.into_config()?;
        for transport in &config.transports {
            if !S::SUPPORTED_TRANSPORTS.contains(transport) {
                return Err(ConnectError::Config(ConfigError::UnsupportedTransport(
                    *transport,
                )));
            }
        }

        let (transport, open_packet) = Self::connect_inner(svc, &config).await?;

        let client = Client {
            transport,
            open_packet,
            config,
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
            TransportType::Polling => Transport::connect_polling(svc, config).await,
            TransportType::Websocket => Transport::connect_ws(svc, config).await,
        }?;

        Ok((transport, packet))
    }

    /// Returns the transport type of the client.
    pub fn transport(&self) -> TransportType {
        self.transport.transport_type()
    }
}

impl<S: TransportSvc> Client<S> {
    /// Returns the session ID of the client.
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
        // The heartbeat drives the transport sink: it must not run once the
        // session is closing or closed, and its errors must surface.
        if matches!(
            self.state,
            ClientState::Open | ClientState::Upgrading | ClientState::Running
        ) && let Poll::Ready(Err(err)) = self.as_mut().poll_heartbeat(cx)
        {
            return Poll::Ready(Some(Err(err)));
        }

        match self.state {
            ClientState::Open => {
                let sid = self.open_packet.sid;
                if self.should_upgrade() {
                    let proj = self.as_mut().project();
                    proj.transport.get_mut().start_upgrade(proj.config, sid);
                    *proj.state = ClientState::Upgrading;
                } else {
                    *self.as_mut().project().state = ClientState::Running;
                }
                Poll::Ready(Some(Ok(EioEvent::Connect(sid))))
            }
            ClientState::Upgrading => self.poll_upgrading(cx),
            ClientState::Running => self.poll_transport(cx),
            ClientState::Closed | ClientState::Closing => Poll::Ready(None),
        }
    }

    /// While the upgrade probe is in flight the transport keeps delivering
    /// polling packets. The [`Transport`] settles itself to websocket (probe
    /// succeeded) or back to polling (probe failed), from the stream or
    /// from the sink side: detect the switch here to resume the nominal
    /// state.
    fn poll_upgrading(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, ClientError<S>>>> {
        let settled = match self.transport {
            Transport::Websocket { .. } => Some(true),
            Transport::Polling { .. } => Some(false),
            _ => None,
        };
        let Some(upgraded) = settled else {
            // probe still in flight: packets keep flowing over polling
            return self.poll_transport(cx);
        };

        *self.as_mut().project().state = ClientState::Running;

        if upgraded {
            tracing::debug!(sid = %self.sid(), "websocket transport upgraded");
            Poll::Ready(Some(Ok(EioEvent::Upgrade(TransportType::Websocket))))
        } else {
            // a failed probe never kills the session: continue on polling
            tracing::warn!(sid = %self.sid(), "websocket upgrade failed, staying on polling");
            self.poll_transport(cx)
        }
    }

    fn poll_transport(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<EioEvent, ClientError<S>>>> {
        let mut proj = self.as_mut().project();
        match ready!(proj.transport.as_mut().poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                *proj.pending_pong = true;
                *proj.last_ping = Instant::now();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Ok(Packet::Close)) => {
                // The server closed the session: tear the transport down so
                // nothing is written to the dead session (the server has
                // already forgotten it) and a later `close()` is a no-op.
                proj.transport.get_mut().terminate();
                *proj.state = ClientState::Closing;
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
            Some(Ok(p)) => Poll::Ready(Some(Err(ClientError::invalid_packet(p)))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }

    /// A fatal error surfaced by the transport sink means the session is
    /// over: mark the client closed so the stream terminates instead of
    /// driving a dead transport.
    fn close_on_fatal(self: Pin<&mut Self>, err: &ClientError<S>) {
        if err.should_close() {
            *self.project().state = ClientState::Closed;
        }
    }

    fn poll_heartbeat(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), ClientError<S>>> {
        if self.last_ping.elapsed()
            >= self.open_packet.ping_interval + self.open_packet.ping_timeout
        {
            return Poll::Ready(Err(ClientError::HeartbeatTimeout));
        }

        let mut proj = self.project();
        if *proj.pending_pong {
            // never through the sink: its readiness belongs to the user
            ready!(proj.transport.as_mut().poll_queue_pong(cx))?;
            *proj.pending_pong = false;
        }

        // idempotent: continues an in-flight flush, or Ready immediately if clean
        // TODO: check this assertion
        proj.transport.poll_flush(cx)
    }
}

impl<S: TransportSvc> Sink<EioEvent> for Client<S> {
    type Error = ClientError<S>;

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state {
            // while upgrading, the sink drives the probe: the write completes
            // once the transport settles, over the selected transport.
            ClientState::Open | ClientState::Upgrading | ClientState::Running => {
                let res = ready!(self.as_mut().project().transport.poll_ready(cx))
                    .inspect_err(|err| self.close_on_fatal(err));
                Poll::Ready(res)
            }
            ClientState::Closing => Poll::Ready(Err(ClientError::TransportClosed)),
            ClientState::Closed => Poll::Ready(Err(ClientError::TransportClosed)),
        }
    }

    #[tracing::instrument(level = Level::TRACE, ret)]
    fn start_send(mut self: Pin<&mut Self>, event: EioEvent) -> Result<(), Self::Error> {
        if let Some(packet) = event.into() {
            self.as_mut()
                .project()
                .transport
                .start_send(packet)
                .inspect_err(|err| self.close_on_fatal(err))?;
        }
        Ok(())
    }

    #[tracing::instrument(level = Level::TRACE, skip(cx), ret)]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let res = ready!(self.as_mut().project().transport.poll_flush(cx));
        if let Err(err) = &res {
            self.close_on_fatal(err);
        }
        Poll::Ready(res)
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
            ClientState::Closing => match ready!(proj.transport.poll_close(cx)) {
                Ok(()) => {
                    *proj.state = ClientState::Closed;
                    Poll::Ready(Ok(()))
                }
                Err(err) => {
                    if err.should_close() {
                        *proj.state = ClientState::Closed;
                    }
                    Poll::Ready(Err(err))
                }
            },
            ClientState::Closed => Poll::Ready(Ok(())),
        }
    }
}

impl<S: TransportSvc> fmt::Debug for Client<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("transport", &self.transport)
            .field("config", &self.config)
            .field("open_packet", &self.open_packet)
            .field("last_ping", &self.last_ping)
            .field("state", &self.state)
            .field("pending_pong", &self.pending_pong)
            .finish()
    }
}
