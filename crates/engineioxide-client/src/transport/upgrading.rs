use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::Packet;
use futures_core::Stream;
use futures_util::Sink;
use pin_project_lite::pin_project;
use tracing::Level;

use crate::{
    ClientError,
    transport::{PollingTransport, TransportSvc, WsTransport, ws::WsError},
};

pin_project! {

    /// Combines the (still running) polling transport with the websocket probe
    /// while an upgrade is in flight.
    ///
    /// Packets keep flowing over polling for the whole probe so nothing is lost
    /// while upgrading. Once the probe settles, the [`Stream`] emits
    /// [`Packet::Upgrade`] on success or an [`UpgradeError`] and the caller
    /// is expected to switch to [`into_next`](Self::into_next) or
    /// [`into_prev`](Self::into_prev) respectively.
    pub struct UpgradingTransport<S: TransportSvc> {
        #[pin]
        polling: PollingTransport<S>,
        #[pin]
        websocket: WsTransport<S>,

        upgrade: UpgradeHandshakeState,
        // cause of the probe failure, emitted once the websocket is closed
        probe_error: Option<ClientError<S>>,
    }

}

#[derive(Debug, Clone, Copy)]
enum UpgradeHandshakeState {
    ShouldSendPingUpgrade,
    ShouldFlushPingUpgrade,
    WaitingPong,
    ShouldSendUpgrade,
    ShouldFlushUpgrade,
    Done,
    // the probe failed: gracefully close the websocket before falling back
    ClosingWs,
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
            probe_error: None,
        }
    }

    /// The upgrade succeeded: keep the websocket and drop the polling
    /// transport. Its held poll request is simply canceled: the server
    /// already released the session to the websocket.
    pub(super) fn into_next(self) -> WsTransport<S> {
        self.websocket
    }

    /// The upgrade failed: keep the (never interrupted) polling transport
    /// and drop the closed websocket probe.
    pub(super) fn into_prev(self) -> PollingTransport<S> {
        self.polling
    }

    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_handshake(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<UpgradeHandshakeState, ClientError<S>>> {
        let upgrade = self.upgrade;
        let mut ws = self.project().websocket;
        match upgrade {
            UpgradeHandshakeState::ShouldSendPingUpgrade => {
                ready!(ws.as_mut().poll_ready(cx))?;
                ws.start_send(Packet::PingUpgrade)?;
                Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushPingUpgrade))
            }
            UpgradeHandshakeState::ShouldFlushPingUpgrade => {
                ready!(ws.poll_flush(cx))?;
                Poll::Ready(Ok(UpgradeHandshakeState::WaitingPong))
            }
            UpgradeHandshakeState::WaitingPong => match ready!(ws.poll_next(cx)) {
                Some(Ok(Packet::PongUpgrade)) => {
                    Poll::Ready(Ok(UpgradeHandshakeState::ShouldSendUpgrade))
                }
                Some(Ok(p)) => {
                    Poll::Ready(Err(ClientError::expected_packet(Packet::PongUpgrade, p)))
                }
                Some(Err(err)) => Poll::Ready(Err(err.into())),
                None => Poll::Ready(Err(WsError::Closed.into())),
            },
            UpgradeHandshakeState::ShouldSendUpgrade => {
                ready!(ws.as_mut().poll_ready(cx))?;
                ws.start_send(Packet::Upgrade)?;
                Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushUpgrade))
            }
            UpgradeHandshakeState::ShouldFlushUpgrade => {
                ready!(ws.poll_flush(cx))?;
                Poll::Ready(Ok(UpgradeHandshakeState::Done))
            }
            UpgradeHandshakeState::Done | UpgradeHandshakeState::ClosingWs => {
                unreachable!("the handshake is not driven once settled")
            }
        }
    }
}

impl<S: TransportSvc> Stream for UpgradingTransport<S> {
    type Item = Result<Packet, UpgradeError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // polling stays active during the whole probe: any packet takes
        // priority over the handshake progress so nothing is lost.
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

        if let UpgradeHandshakeState::ClosingWs = self.upgrade {
            // best-effort close: the probe is abandoned either way
            if let Err(err) = ready!(self.as_mut().project().websocket.poll_close(cx)) {
                tracing::debug!("error while closing the failed websocket probe: {err}");
            }
            let err = self
                .project()
                .probe_error
                .take()
                .expect("the probe error must be set when closing the websocket");
            return Poll::Ready(Some(Err(UpgradeError::Recoverable(err))));
        }

        match ready!(self.as_mut().poll_handshake(cx)) {
            // the upgrade packet signals the completed handshake
            Ok(UpgradeHandshakeState::Done) => Poll::Ready(Some(Ok(Packet::Upgrade))),
            Ok(next) => {
                *self.project().upgrade = next;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(err) => {
                // a failed probe never kills the session: close the
                // websocket before falling back to polling.
                tracing::warn!("websocket upgrade probe failed: {err}");
                let this = self.project();
                *this.upgrade = UpgradeHandshakeState::ClosingWs;
                *this.probe_error = Some(err);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

/// While upgrading, everything (user packets, heartbeats) keeps flowing over
/// the polling transport: the websocket only carries the probe handshake
/// until the upgrade is confirmed.
impl<S: TransportSvc> Sink<Packet> for UpgradingTransport<S> {
    type Error = ClientError<S>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .polling
            .poll_ready(cx)
            .map_err(ClientError::Polling)
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        self.project()
            .polling
            .start_send(item)
            .map_err(ClientError::Polling)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .polling
            .poll_flush(cx)
            .map_err(ClientError::Polling)
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
            .field("probe_error", &self.probe_error)
            .finish()
    }
}
