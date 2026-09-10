use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{OpenPacket, Packet, ProtocolVersion, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use http::{
    Request, Uri,
    uri::{PathAndQuery, Scheme},
};

use crate::{
    EngineIoClientConfig,
    errors::ClientError,
    flavors::{PollingBody, TransportSvc},
    transport::{polling::PollingTransport, ws::WsTransport},
};

pub use polling::{PollingError, ProtocolError};
use upgrading::Side;
pub use upgrading::{UpgradeError, UpgradingTransport};
pub use ws::WsError;

mod polling;
mod upgrading;
mod ws;

pub(crate) enum Transport<S: TransportSvc> {
    Polling {
        inner: PollingTransport<S>,
    },
    Upgrading {
        inner: UpgradingTransport<S>,
    },
    Websocket {
        inner: WsTransport<S>,
    },
    /// Transient placeholder installed while switching variants. It is
    /// replaced within the same call and should never be observed.
    Switching,
}

impl<S: TransportSvc> Transport<S> {
    pub async fn connect_polling(
        svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Self, OpenPacket), ClientError<S>> {
        let (inner, packet) = PollingTransport::connect(svc, config).await?;
        Ok((Self::Polling { inner }, packet))
    }

    pub async fn connect_ws(
        svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Self, OpenPacket), ClientError<S>> {
        let (inner, packet) = WsTransport::connect(svc, config).await?;
        Ok((Self::Websocket { inner }, packet))
    }

    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling { .. } | Transport::Upgrading { .. } => TransportType::Polling,
            Transport::Websocket { .. } => TransportType::Websocket,
            Transport::Switching => unreachable!("transport observed mid-switch"),
        }
    }

    /// Start the websocket upgrade: connect a probe websocket for the
    /// current session while the polling transport keeps running alongside
    /// it until the probe settles.
    ///
    /// Must only be called on the polling transport.
    pub(crate) fn start_upgrade(&mut self, config: &EngineIoClientConfig, sid: Sid) {
        let Transport::Polling { inner } = self else {
            unreachable!("an upgrade can only be started from the polling transport");
        };
        let websocket = WsTransport::connect_with_upgrade(inner.svc.clone(), config, sid);
        let Transport::Polling { inner } = std::mem::replace(self, Transport::Switching) else {
            unreachable!();
        };
        *self = Transport::Upgrading {
            inner: UpgradingTransport::new(inner, websocket),
        };
    }

    /// Replace the `Upgrading` variant with the transport chosen by `next`.
    fn settle_upgrade(&mut self, next: impl FnOnce(UpgradingTransport<S>) -> Transport<S>) {
        match std::mem::replace(self, Transport::Switching) {
            Transport::Upgrading { inner } => *self = next(inner),
            _ => unreachable!("settle_upgrade is only called on the upgrading transport"),
        }
    }

    /// Replace the `Upgrading` variant with the transport the probe selected.
    fn settle_upgrade_to(&mut self, upgraded: bool) {
        if upgraded {
            tracing::debug!("upgrade done, switching to the websocket transport");
            self.settle_upgrade(|upgrading| Transport::Websocket {
                inner: upgrading.into_next(),
            });
        } else {
            self.settle_upgrade(|upgrading| Transport::Polling {
                inner: upgrading.into_prev(),
            });
        }
    }

    /// Tear the transport down: the session is over, nothing must be sent
    /// or received anymore. Closing afterwards is a no-op.
    pub(crate) fn terminate(&mut self) {
        match self {
            Transport::Polling { inner } => Pin::new(inner).terminate(),
            Transport::Upgrading { inner } => Pin::new(inner).terminate(),
            Transport::Websocket { inner } => Pin::new(inner).terminate(),
            Transport::Switching => {}
        }
    }
}

impl<S: TransportSvc> Stream for Transport<S> {
    type Item = Result<Packet, ClientError<S>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match &mut *this {
            Transport::Polling { inner } => {
                Pin::new(inner).poll_next(cx).map_err(ClientError::Polling)
            }
            Transport::Upgrading { inner } => match ready!(Pin::new(inner).poll_next(cx)) {
                // the upgrade packet signals the completed handshake
                Some(Ok(Packet::Upgrade)) => {
                    this.settle_upgrade_to(true);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(Ok(packet)) => Poll::Ready(Some(Ok(packet))),
                Some(Err(UpgradeError::Recoverable(err))) => {
                    tracing::debug!("upgrade failed ({err}), falling back to polling");
                    this.settle_upgrade_to(false);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(Err(UpgradeError::Unrecoverable(err))) => Poll::Ready(Some(Err(err))),
                None => Poll::Ready(None),
            },
            Transport::Websocket { inner } => Pin::new(inner)
                .poll_next(cx)
                .map_err(ClientError::Websocket),
            Transport::Switching => Poll::Ready(None),
        }
    }
}
impl<S: TransportSvc> Sink<Packet> for Transport<S> {
    type Error = ClientError<S>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        loop {
            match this {
                Transport::Polling { inner } => {
                    return Pin::new(inner).poll_ready(cx).map_err(ClientError::Polling);
                }
                Transport::Upgrading { inner } => {
                    // The sink drives the probe too: a write issued while
                    // upgrading completes once the probe settles, over the
                    // transport it selected, without waiting for the stream
                    // to be polled.
                    let upgraded = match ready!(Pin::new(&mut *inner).poll_upgrade(cx, Side::Sink))
                    {
                        Ok(upgraded) => upgraded,
                        Err(err) => return Poll::Ready(Err(err)),
                    };
                    if inner.has_inbound() {
                        // Packets from the last poll are still to be yielded:
                        // the stream switches the transport once they are out.
                        // Meanwhile writes already go over the selected one.
                        return Pin::new(inner).poll_ready(cx);
                    }
                    this.settle_upgrade_to(upgraded);
                }
                Transport::Websocket { inner } => {
                    return Pin::new(inner)
                        .poll_ready(cx)
                        .map_err(ClientError::Websocket);
                }
                Transport::Switching => return Poll::Ready(Err(ClientError::TransportClosed)),
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.get_mut() {
            Transport::Polling { inner } => Pin::new(inner)
                .start_send(item)
                .map_err(ClientError::Polling),
            Transport::Upgrading { inner } => Pin::new(inner).start_send(item),
            Transport::Websocket { inner } => Pin::new(inner)
                .start_send(item)
                .map_err(ClientError::Websocket),
            Transport::Switching => Err(ClientError::TransportClosed),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Transport::Polling { inner } => {
                Pin::new(inner).poll_flush(cx).map_err(ClientError::Polling)
            }
            Transport::Upgrading { inner } => Pin::new(inner).poll_flush(cx),
            Transport::Websocket { inner } => Pin::new(inner)
                .poll_flush(cx)
                .map_err(ClientError::Websocket),
            Transport::Switching => Poll::Ready(Ok(())),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Transport::Polling { inner } => {
                Pin::new(inner).poll_close(cx).map_err(ClientError::Polling)
            }
            Transport::Upgrading { inner } => Pin::new(inner).poll_close(cx),
            Transport::Websocket { inner } => Pin::new(inner)
                .poll_close(cx)
                .map_err(ClientError::Websocket),
            Transport::Switching => Poll::Ready(Ok(())),
        }
    }
}

impl<S: TransportSvc> fmt::Debug for Transport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Polling { inner } => f.debug_struct("Polling").field("inner", inner).finish(),
            Self::Upgrading { inner } => f.debug_struct("Upgrading").field("inner", inner).finish(),
            Self::Websocket { inner } => f.debug_struct("Websocket").field("inner", inner).finish(),
            Self::Switching => f.write_str("Switching"),
        }
    }
}

fn build_connect_req(base_uri: &Uri, transport: TransportType) -> Request<PollingBody> {
    let uri = with_mandatory_query(base_uri, transport, None);

    Request::builder()
        .method(http::Method::GET)
        .uri(uri)
        .body(PollingBody::new_empty())
        .unwrap()
}

/// Merges the user-provided `base_uri` (scheme + authority + path, and any
/// pre-existing query) with the query parameters engine.io mandates on every
/// request: the protocol version (`EIO`) and the `transport` in use.
fn with_mandatory_query(base_uri: &Uri, transport: TransportType, sid: Option<Sid>) -> Uri {
    let secure = is_uri_secure(base_uri);
    let mut parts = base_uri.clone().into_parts();

    parts.scheme = match (transport, secure) {
        (TransportType::Polling, Some(true)) => Some(Scheme::HTTPS),
        (TransportType::Websocket, Some(true)) => Some("wss".parse().unwrap()),
        (TransportType::Polling, Some(false)) => Some(Scheme::HTTP),
        (TransportType::Websocket, Some(false)) => Some("ws".parse().unwrap()),
        (_, None) => None,
    };

    let path = parts
        .path_and_query
        .as_ref()
        .map(|pq| pq.path())
        .unwrap_or("/")
        .to_owned();

    let existing_query = parts
        .path_and_query
        .as_ref()
        .and_then(|pq| pq.query())
        .filter(|q| !q.is_empty());

    let protocol = format_args!("EIO={}&transport={transport}", ProtocolVersion::V4);

    let path = match (existing_query, sid) {
        (Some(existing), Some(sid)) => format!("{path}?sid={sid}&{protocol}&{existing}"),
        (Some(existing), None) => format!("{path}?{protocol}&{existing}"),
        (None, Some(sid)) => format!("{path}?sid={sid}&{protocol}"),
        (None, None) => format!("{path}?{protocol}"),
    };

    parts.path_and_query =
        Some(PathAndQuery::try_from(path).expect("base uri path should be valid"));

    Uri::from_parts(parts).expect("base uri should produce a valid uri")
}

//TODO: invalid scheme err
fn is_uri_secure(uri: &Uri) -> Option<bool> {
    match uri.scheme_str()? {
        "http" | "ws" => Some(false),
        "https" | "wss" => Some(true),
        _ => None,
    }
}
