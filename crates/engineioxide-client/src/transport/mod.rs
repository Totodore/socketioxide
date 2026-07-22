use std::{
    convert::Infallible,
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide_core::{Packet, ProtocolVersion, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use http::{
    Request, Uri,
    uri::{PathAndQuery, Scheme},
};
use http_body_util::{Empty, combinators::BoxBody};
use tracing::Level;

use crate::{EngineIoClientConfig, errors::ClientError};

pub use polling::{PollingSvc, PollingTransport};
pub use ws::{WebSocket, WsSvc, WsTransport};

pub mod polling;
pub mod ws;

pub trait TransportSvc: PollingSvc + WsSvc {}
impl<S: PollingSvc + WsSvc> TransportSvc for S {}

pin_project_lite::pin_project! {
    #[project = TransportProj]
    pub enum Transport<S: TransportSvc> {
        Polling {
            #[pin]
            inner: PollingTransport<S>
        },
        Websocket {
            #[pin]
            inner: WsTransport<S>
        }
    }
}

impl<S: TransportSvc> Transport<S> {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling { .. } => TransportType::Polling,
            Transport::Websocket { .. } => TransportType::Websocket,
        }
    }
}
impl<S: TransportSvc> Transport<S> {
    /// Upgrade the current transport to [`WsTransport`].
    ///
    /// Starts by flushing the polling transport, once done switch to websocket
    /// and drive the websocket protocol upgrade.
    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    pub fn upgrade(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        config: &EngineIoClientConfig,
        sid: Sid,
    ) -> Poll<Option<Result<(), ClientError<S>>>> {
        match self.as_mut().project() {
            TransportProj::Polling { mut inner } => {
                // start by flushing polling transport to ensure there is no pending data
                ready!(inner.as_mut().poll_flush(cx))?;
                let svc = inner.svc.clone();

                self.set(Transport::Websocket {
                    inner: WsTransport::connect_with_upgrade(svc, config, sid),
                });
                cx.waker().wake_by_ref();
                Poll::Pending
            }

            TransportProj::Websocket { inner } => match ready!(inner.poll_next(cx)) {
                Some(Ok(Packet::Upgrade)) => Poll::Ready(Some(Ok(()))),
                Some(Ok(p)) => todo!("handle err: {p:?}"),
                Some(Err(err)) => Poll::Ready(Some(Err(err.into()))),
                None => Poll::Ready(None),
            },
        }
    }
}

impl<S: TransportSvc> Stream for Transport<S> {
    type Item = Result<Packet, ClientError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project() {
            TransportProj::Polling { inner } => inner.poll_next(cx).map_err(ClientError::Polling),
            TransportProj::Websocket { inner } => {
                inner.poll_next(cx).map_err(ClientError::Websocket)
            }
        }
    }
}
impl<S: TransportSvc> Sink<Packet> for Transport<S> {
    type Error = ClientError<S>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_ready(cx).map_err(ClientError::Polling),
            TransportProj::Websocket { inner } => {
                inner.poll_ready(cx).map_err(ClientError::Websocket)
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.project() {
            TransportProj::Polling { inner } => {
                inner.start_send(item).map_err(ClientError::Polling)
            }
            TransportProj::Websocket { inner } => {
                inner.start_send(item).map_err(ClientError::Websocket)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_flush(cx).map_err(ClientError::Polling),
            TransportProj::Websocket { inner } => {
                inner.poll_flush(cx).map_err(ClientError::Websocket)
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_close(cx).map_err(ClientError::Polling),
            TransportProj::Websocket { inner } => {
                inner.poll_close(cx).map_err(ClientError::Websocket)
            }
        }
    }
}

impl<S: TransportSvc> From<PollingTransport<S>> for Transport<S> {
    fn from(inner: PollingTransport<S>) -> Self {
        Self::Polling { inner }
    }
}
impl<S: TransportSvc> From<WsTransport<S>> for Transport<S> {
    fn from(inner: WsTransport<S>) -> Self {
        Self::Websocket { inner }
    }
}
impl<S: TransportSvc> fmt::Debug for Transport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Polling { inner } => f.debug_struct("Polling").field("inner", inner).finish(),
            Self::Websocket { inner } => f.debug_struct("Websocket").field("inner", inner).finish(),
        }
    }
}

fn build_connect_req(
    base_uri: &Uri,
    transport: TransportType,
) -> Request<BoxBody<Bytes, Infallible>> {
    let uri = with_mandatory_query(base_uri, transport, None);

    Request::builder()
        .method(http::Method::GET)
        .uri(uri)
        .body(BoxBody::new(Empty::new()))
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
