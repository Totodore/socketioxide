use std::{
    collections::VecDeque,
    fmt,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    task::{Context, Poll, Waker, ready},
};

use engineioxide_core::Packet;
use futures_core::Stream;
use futures_util::{
    Sink,
    task::{ArcWake, waker_ref},
};
use pin_project_lite::pin_project;
use tracing::Level;

use crate::{
    ClientError,
    transport::{PollingError, PollingTransport, TransportSvc, WsTransport, ws::WsError},
};

pin_project! {

    /// Combines the (still running) polling transport with the websocket probe
    /// while an upgrade is in flight.
    ///
    /// Packets keep flowing over polling until the probe is acknowledged.
    /// Polling is then paused (reference `pause()`): no new poll is issued,
    /// the in-flight poll is awaited (the server releases it with a `noop`)
    /// and its packets are buffered, and every write queued or in flight
    /// over polling is flushed. Only then is the upgrade packet sent, and
    /// later writes go over the websocket.
    ///
    /// The probe is driven both by the [`Stream`] and by
    /// [`poll_upgrade`](Self::poll_upgrade), so a write issued while
    /// upgrading completes on its own instead of waiting for the stream to
    /// be polled. Once the probe settles, the [`Stream`] emits
    /// [`Packet::Upgrade`] on success or an [`UpgradeError`] and the caller
    /// is expected to switch to [`into_next`](Self::into_next) or
    /// [`into_prev`](Self::into_prev) respectively.
    pub struct UpgradingTransport<S: TransportSvc> {
        #[pin]
        polling: PollingTransport<S>,
        #[pin]
        websocket: WsTransport<S>,

        upgrade: UpgradeHandshakeState,

        // An upgrade error that must be yielded after
        // closing the websocket transport.
        upgrade_error: Option<ClientError<S>>,

        // Packets received from the last poll while pausing: yielded by the
        // stream before the upgrade is reported.
        inbound: VecDeque<Packet>,

        wakers: Arc<Wakers>,
    }

}

/// Which side of the transport drives the probe.
#[derive(Debug, Clone, Copy)]
pub(super) enum Side {
    Stream = 0,
    Sink = 1,
}

/// The probe is driven from both the stream side and the sink side, which
/// may live on different tasks once the client is split. Inner futures only
/// remember the last waker they were polled with, so both sides register
/// here and are woken together.
#[derive(Default)]
struct Wakers {
    sides: Mutex<[Option<Waker>; 2]>,
}

impl Wakers {
    fn register(&self, side: Side, waker: &Waker) {
        let mut sides = self.sides.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut sides[side as usize] {
            Some(current) if current.will_wake(waker) => {}
            slot => *slot = Some(waker.clone()),
        }
    }
}

impl ArcWake for Wakers {
    fn wake_by_ref(this: &Arc<Self>) {
        let sides = this.sides.lock().unwrap_or_else(PoisonError::into_inner);
        let wakers: Vec<Waker> = sides.iter().flatten().cloned().collect();
        drop(sides);
        for waker in wakers {
            waker.wake();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum UpgradeHandshakeState {
    ShouldSendPingUpgrade,
    ShouldFlushPingUpgrade,
    WaitingPong,
    /// The probe is acknowledged: stop polling, wait for the in-flight poll
    /// (buffering its packets) and drain the polling write buffer.
    Pausing,
    ShouldSendUpgrade,
    /// The upgrade packet is handed to the websocket: every write now goes
    /// over the websocket.
    ShouldFlushUpgrade,
    /// The upgrade is confirmed.
    Done,
    /// The probe failed: gracefully close the websocket before falling back.
    ClosingWs,
    /// The probe failed and its websocket is closed: the session continues
    /// over polling.
    Failed,
}

/// Error emitted by the [`UpgradingTransport`] stream when the upgrade
/// cannot complete.
#[derive(thiserror::Error)]
pub enum UpgradeError<S: TransportSvc> {
    /// The websocket probe failed: the session is unaffected and keeps
    /// running over the polling transport.
    #[error("recoverable upgrade error: {0}")]
    Recoverable(ClientError<S>),
    /// The polling transport failed while probing: the session is over.
    #[error("unrecoverable upgrade error: {0}")]
    Unrecoverable(ClientError<S>),
}

impl<S: TransportSvc> fmt::Debug for UpgradeError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpgradeError::Recoverable(e) => f.debug_tuple("Recoverable").field(e).finish(),
            UpgradeError::Unrecoverable(e) => f.debug_tuple("Unrecoverable").field(e).finish(),
        }
    }
}

impl<S: TransportSvc> UpgradingTransport<S> {
    pub(super) fn new(polling: PollingTransport<S>, websocket: WsTransport<S>) -> Self {
        Self {
            polling,
            websocket,
            upgrade: UpgradeHandshakeState::ShouldSendPingUpgrade,
            upgrade_error: None,
            inbound: VecDeque::new(),
            wakers: Arc::default(),
        }
    }

    /// The upgrade succeeded: keep the websocket and drop the polling
    /// transport. Polling was paused before the upgrade packet was sent:
    /// its last poll completed, its write buffer was drained, so nothing is
    /// left behind.
    ///
    /// Must only be called once [`has_inbound`](Self::has_inbound) is false.
    pub(super) fn into_next(self) -> WsTransport<S> {
        debug_assert!(self.inbound.is_empty(), "buffered packets would be lost");
        self.websocket
    }

    /// The upgrade failed: resume the polling transport and drop the closed
    /// websocket probe.
    pub(super) fn into_prev(self) -> PollingTransport<S> {
        let Self {
            mut polling,
            upgrade_error,
            ..
        } = self;
        if let Some(err) = upgrade_error {
            tracing::debug!("upgrade failed ({err}), falling back to polling");
        }
        polling.resume();
        polling
    }

    /// Packets received from the last poll while pausing are still to be
    /// yielded by the stream: the transport must not be switched yet.
    pub(super) fn has_inbound(&self) -> bool {
        !self.inbound.is_empty()
    }

    /// Tear both transports down.
    pub(super) fn terminate(self: Pin<&mut Self>) {
        let this = self.project();
        this.polling.terminate();
        this.websocket.terminate();
        this.inbound.clear();
    }

    /// Queue the heartbeat pong over the transport writes currently go to,
    /// outside of the [`Sink`] (see [`PollingTransport::queue_pong`]).
    pub(super) fn poll_queue_pong(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), ClientError<S>>> {
        let upgrade_sent = self.upgrade_sent();
        let this = self.project();
        if upgrade_sent {
            let mut ws = this.websocket;
            ready!(ws.as_mut().poll_ready(cx)).map_err(ClientError::Websocket)?;
            Poll::Ready(ws.start_send(Packet::Pong).map_err(ClientError::Websocket))
        } else {
            Poll::Ready(this.polling.queue_pong().map_err(ClientError::Polling))
        }
    }

    /// Once the upgrade packet is handed to the websocket, every later
    /// write goes over the websocket, ordered after the upgrade packet, so
    /// nothing is left behind in the polling transport.
    fn upgrade_sent(&self) -> bool {
        matches!(
            self.upgrade,
            UpgradeHandshakeState::ShouldFlushUpgrade | UpgradeHandshakeState::Done
        )
    }

    /// Drives the probe until it settles, without yielding anything.
    ///
    /// * `Ready(Ok(true))`: the upgrade is confirmed, switch with
    ///   [`into_next`](Self::into_next).
    /// * `Ready(Ok(false))`: the probe failed and its websocket is closed,
    ///   switch back with [`into_prev`](Self::into_prev).
    /// * `Ready(Err(_))`: the polling transport failed, the session is over.
    ///
    /// Both outcomes are stable: the settled probe can be polled again from
    /// either side until the caller switches transport.
    pub(super) fn poll_upgrade(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        side: Side,
    ) -> Poll<Result<bool, ClientError<S>>> {
        self.wakers.register(side, cx.waker());
        let wakers = self.wakers.clone();
        let waker = waker_ref(&wakers);
        let mut cx = Context::from_waker(&waker);

        loop {
            match self.upgrade {
                UpgradeHandshakeState::Done => return Poll::Ready(Ok(true)),
                UpgradeHandshakeState::Failed => return Poll::Ready(Ok(false)),
                _ => {}
            }

            let next = match ready!(self.as_mut().poll_handshake(&mut cx)) {
                Ok(next) => next,
                Err(UpgradeError::Unrecoverable(err)) => {
                    // the other side may be parked on the dead polling transport
                    ArcWake::wake_by_ref(&wakers);
                    return Poll::Ready(Err(err));
                }
                Err(UpgradeError::Recoverable(err)) => {
                    // a failed upgrade handshake never kills the session: close
                    // the websocket before falling back to polling.
                    tracing::warn!("websocket upgrade probe failed: {err}");
                    *self.as_mut().project().upgrade_error = Some(err);
                    UpgradeHandshakeState::ClosingWs
                }
            };
            tracing::trace!(?next, "upgrade handshake step");
            *self.as_mut().project().upgrade = next;
            // the other side may be waiting on the step that just completed
            ArcWake::wake_by_ref(&wakers);
        }
    }

    /// Performs a single step of the handshake.
    ///
    /// A websocket failure is [`UpgradeError::Recoverable`] (the session
    /// continues over polling), a polling failure is
    /// [`UpgradeError::Unrecoverable`] (the session is over).
    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_handshake(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<UpgradeHandshakeState, UpgradeError<S>>> {
        fn recoverable<S: TransportSvc>(err: impl Into<ClientError<S>>) -> UpgradeError<S> {
            UpgradeError::Recoverable(err.into())
        }
        fn unrecoverable<S: TransportSvc>(err: impl Into<ClientError<S>>) -> UpgradeError<S> {
            UpgradeError::Unrecoverable(err.into())
        }

        let upgrade = self.upgrade;
        let this = self.project();
        let mut ws = this.websocket;
        let mut polling = this.polling;
        match upgrade {
            UpgradeHandshakeState::ShouldSendPingUpgrade => {
                ready!(ws.as_mut().poll_ready(cx)).map_err(recoverable)?;
                ws.start_send(Packet::PingUpgrade).map_err(recoverable)?;
                Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushPingUpgrade))
            }
            UpgradeHandshakeState::ShouldFlushPingUpgrade => {
                ready!(ws.poll_flush(cx)).map_err(recoverable)?;
                Poll::Ready(Ok(UpgradeHandshakeState::WaitingPong))
            }
            UpgradeHandshakeState::WaitingPong => match ready!(ws.poll_next(cx)) {
                Some(Ok(Packet::PongUpgrade)) => Poll::Ready(Ok(UpgradeHandshakeState::Pausing)),
                Some(Ok(p)) => Poll::Ready(Err(recoverable(ClientError::expected_packet(
                    Packet::PongUpgrade,
                    p,
                )))),
                Some(Err(err)) => Poll::Ready(Err(recoverable(err))),
                None => Poll::Ready(Err(recoverable(WsError::Closed))),
            },
            UpgradeHandshakeState::Pausing => {
                // Reference `pause()`: the polling transport is dropped right
                // after the switch and the server refuses any request once
                // upgraded, so the in-flight poll (which the server releases
                // with a noop) and every queued or in-flight write must
                // complete *before* the upgrade packet is sent.
                polling.as_mut().pause();
                while !polling.is_idle() {
                    match ready!(polling.as_mut().poll_next(cx)) {
                        // the noop releasing the poll carries nothing
                        Some(Ok(Packet::Noop)) => {}
                        Some(Ok(packet)) => this.inbound.push_back(packet),
                        Some(Err(err)) => return Poll::Ready(Err(unrecoverable(err))),
                        None => {
                            return Poll::Ready(Err(unrecoverable(PollingError::<S>::Closed)));
                        }
                    }
                }
                ready!(polling.poll_flush(cx)).map_err(unrecoverable)?;
                Poll::Ready(Ok(UpgradeHandshakeState::ShouldSendUpgrade))
            }
            UpgradeHandshakeState::ShouldSendUpgrade => {
                ready!(ws.as_mut().poll_ready(cx)).map_err(recoverable)?;
                ws.start_send(Packet::Upgrade).map_err(recoverable)?;
                Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushUpgrade))
            }
            UpgradeHandshakeState::ShouldFlushUpgrade => {
                ready!(ws.poll_flush(cx)).map_err(recoverable)?;
                Poll::Ready(Ok(UpgradeHandshakeState::Done))
            }
            UpgradeHandshakeState::ClosingWs => {
                // best-effort close: the probe is abandoned either way
                if let Err(err) = ready!(ws.poll_close(cx)) {
                    tracing::debug!("error while closing the failed websocket probe: {err}");
                }
                Poll::Ready(Ok(UpgradeHandshakeState::Failed))
            }
            UpgradeHandshakeState::Done | UpgradeHandshakeState::Failed => {
                unreachable!("the handshake is not driven once settled")
            }
        }
    }
}

impl<S: TransportSvc> Stream for UpgradingTransport<S> {
    type Item = Result<Packet, UpgradeError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // packets buffered from the last poll while pausing come first
        if let Some(packet) = self.as_mut().project().inbound.pop_front() {
            return Poll::Ready(Some(Ok(packet)));
        }

        // polling stays active until paused: any packet takes priority
        // over the handshake progress so nothing is lost.
        match self.as_mut().project().polling.poll_next(cx) {
            Poll::Ready(Some(Ok(packet))) => return Poll::Ready(Some(Ok(packet))),
            Poll::Ready(Some(Err(err))) => {
                // polling failed mid-upgrade: the session is over
                return Poll::Ready(Some(Err(UpgradeError::Unrecoverable(err.into()))));
            }
            // polling ended mid-upgrade: the session is over
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }

        let upgraded = match ready!(self.as_mut().poll_upgrade(cx, Side::Stream)) {
            Ok(upgraded) => upgraded,
            Err(err) => return Poll::Ready(Some(Err(UpgradeError::Unrecoverable(err)))),
        };

        // pausing may just have buffered packets: yield them before settling
        if self.has_inbound() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        if upgraded {
            // the upgrade packet signals the completed handshake
            Poll::Ready(Some(Ok(Packet::Upgrade)))
        } else {
            let err = self
                .project()
                .upgrade_error
                .take()
                .expect("the upgrade error must be set when the probe failed");
            Poll::Ready(Some(Err(UpgradeError::Recoverable(err))))
        }
    }
}

/// While upgrading, everything (user packets, heartbeats) keeps flowing over
/// the polling transport: the websocket only carries the probe handshake
/// until the upgrade packet is sent. From then on writes go over the
/// websocket, right behind the upgrade packet.
impl<S: TransportSvc> Sink<Packet> for UpgradingTransport<S> {
    type Error = ClientError<S>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let upgrade_sent = self.upgrade_sent();
        let this = self.project();
        if upgrade_sent {
            this.websocket
                .poll_ready(cx)
                .map_err(ClientError::Websocket)
        } else {
            this.polling.poll_ready(cx).map_err(ClientError::Polling)
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        let upgrade_sent = self.upgrade_sent();
        let this = self.project();
        if upgrade_sent {
            this.websocket
                .start_send(item)
                .map_err(ClientError::Websocket)
        } else {
            this.polling.start_send(item).map_err(ClientError::Polling)
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let upgrade_sent = self.upgrade_sent();
        let this = self.project();
        if upgrade_sent {
            this.websocket
                .poll_flush(cx)
                .map_err(ClientError::Websocket)
        } else {
            this.polling.poll_flush(cx).map_err(ClientError::Polling)
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // closing mid-upgrade: abort the probe first, then close the
        // session over its current (polling) transport.
        if let Err(err) = ready!(self.as_mut().project().websocket.poll_close(cx)) {
            tracing::debug!("error while closing the websocket probe: {err}");
        }
        self.project()
            .polling
            .poll_close(cx)
            .map_err(ClientError::Polling)
    }
}

impl<S: TransportSvc> fmt::Debug for UpgradingTransport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpgradingTransport")
            .field("polling", &self.polling)
            .field("websocket", &self.websocket)
            .field("upgrade", &self.upgrade)
            .field("upgrade_error", &self.upgrade_error)
            .field("inbound", &self.inbound)
            .finish()
    }
}
