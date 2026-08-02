use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide_core::{
    OpenPacket, Packet, PacketParseError, ProtocolVersion, Sid, Str, TransportType,
};
use futures_core::Stream;
use futures_util::{Sink, StreamExt};
use http::Request;
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tracing::Level;

use crate::EngineIoClientConfig;

pin_project! {
    pub struct WsTransport<S: WsSvc> {
        svc: S,

        #[pin]
        state: WsTransportState<S>,
    }
}

pin_project! {
    #[project = WsTransportStateProj]
    enum WsTransportState<S: WsSvc> {
        Connecting {
            #[pin]
            fut: S::Future,
        },
        Stream {
            #[pin]
            stream: S::WebSocket,
        },
        // Terminal state: the connect future / websocket stream is dropped so
        // it can never be polled again after an error or a close.
        Closed,
    }
}

pub enum WsError<S: WsSvc> {
    Websocket(<S as WsSvc>::Error),
    Packet(PacketParseError),
    Closed,
}

impl<S: WsSvc> WsError<S> {
    pub(crate) fn should_close(&self) -> bool {
        matches!(
            self,
            WsError::Closed | WsError::Websocket(_) | WsError::Packet(_)
        )
    }
}
impl<S: WsSvc> From<PacketParseError> for WsError<S> {
    fn from(e: PacketParseError) -> Self {
        WsError::Packet(e)
    }
}
impl<S: WsSvc> fmt::Debug for WsError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::Websocket(e) => f.debug_tuple("Websocket").field(e).finish(),
            WsError::Packet(e) => f.debug_tuple("Packet").field(e).finish(),
            WsError::Closed => f.write_str("Closed"),
        }
    }
}
impl<S: WsSvc> fmt::Display for WsError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::Websocket(e) => write!(f, "websocket error: {}", e),
            WsError::Packet(e) => write!(f, "packet error: {}", e),
            WsError::Closed => write!(f, "websocket closed"),
        }
    }
}
impl<S: WsSvc> std::error::Error for WsError<S> {}

impl<S: WsSvc> WsTransport<S> {
    #[tracing::instrument(skip(svc))]
    pub fn connect_with_upgrade(svc: S, config: &EngineIoClientConfig, sid: Sid) -> Self {
        tracing::trace!("websocket connection with upgrade");
        let uri = super::with_mandatory_query(&config.uri, TransportType::Websocket, Some(sid));
        let req = Request::get(uri)
            .header("Host", "127.0.0.1")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .body(())
            .unwrap();

        let fut = svc.call(req);
        Self {
            svc,
            state: WsTransportState::Connecting { fut },
        }
    }

    #[tracing::instrument(skip(svc))]
    pub async fn connect(
        svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Self, OpenPacket), WsError<S>> {
        tracing::trace!("websocket connection without upgrade");
        let uri = super::with_mandatory_query(&config.uri, TransportType::Websocket, None);

        let req = Request::get(uri)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .body(())
            .unwrap();

        let mut stream = svc.call(req).await.map_err(WsError::Websocket)?;

        tracing::debug!("handshake response received, waiting for open packet");
        let packet = match stream.next().await.ok_or(WsError::Closed)? {
            Ok(msg) => parse_packet(msg)?,
            Err(e) => return Err(WsError::Websocket(e)),
        };
        tracing::debug!("open packet received, switching to streaming");

        let ws = Self {
            svc,
            state: WsTransportState::Stream { stream },
        };

        match packet {
            Packet::Open(open_packet) => Ok((ws, open_packet)),
            _ => Err(WsError::Packet(PacketParseError::InvalidPacketType(None))),
        }
    }
}

pub trait WsSvc:
    HyperSvc<
        http::Request<()>,
        Response = Self::WebSocket,
        Error = <Self as WsSvc>::Error,
        Future: Unpin, // Unpin bound so we can move transports around when upgrading
    > + Clone
{
    type Error: fmt::Debug + std::error::Error;
    type WebSocket: WebSocket<Error = <Self as WsSvc>::Error>;
}

impl<S, WS> WsSvc for S
where
    S: HyperSvc<http::Request<()>, Response = WS, Future: Unpin> + Clone,
    WS: WebSocket<Error = <S as HyperSvc<http::Request<()>>>::Error>,
    <S as HyperSvc<http::Request<()>>>::Error: fmt::Debug + std::error::Error,
{
    type Error = <S as HyperSvc<http::Request<()>>>::Error;
    type WebSocket = WS;
}

pub trait WebSocket:
    Stream<Item = Result<WsMessage, <Self as WebSocket>::Error>>
    + Sink<WsMessage, Error = <Self as WebSocket>::Error>
    + Sized
    + Unpin
{
    type Error: fmt::Debug + std::error::Error;
}

pub enum WsMessage {
    Text(Str),
    Binary(Bytes),
    Close,
}

fn parse_packet<S: WsSvc>(msg: WsMessage) -> Result<Packet, WsError<S>> {
    match msg {
        WsMessage::Text(msg) => {
            let msg_str = unsafe { Str::from_bytes_unchecked(msg.into()) };
            let packet = Packet::parse(ProtocolVersion::V4, msg_str)?;
            Ok(packet)
        }
        WsMessage::Binary(data) => Ok(Packet::Binary(data)),
        // a close frame terminates the transport
        WsMessage::Close => Err(WsError::Closed),
    }
}

impl<S: WsSvc> Stream for WsTransport<S> {
    type Item = Result<Packet, WsError<S>>;

    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().poll_next_inner(cx)) {
            // an error or the end of the stream is terminal: drop the
            // websocket (or the completed connect future) so it can never
            // be polled again.
            Some(Err(err)) if err.should_close() => {
                self.project().state.set(WsTransportState::Closed);
                Poll::Ready(Some(Err(err)))
            }
            None => {
                self.project().state.set(WsTransportState::Closed);
                Poll::Ready(None)
            }
            packet => Poll::Ready(packet),
        }
    }
}

impl<S: WsSvc> WsTransport<S> {
    fn poll_next_inner(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Packet, WsError<S>>>> {
        match self.as_mut().project().state.project() {
            WsTransportStateProj::Connecting { fut } => match ready!(fut.poll(cx)) {
                Ok(stream) => {
                    self.project()
                        .state
                        .set(WsTransportState::Stream { stream });
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Some(Err(WsError::Websocket(e)))),
            },
            WsTransportStateProj::Stream { stream } => match ready!(stream.poll_next(cx)) {
                Some(Ok(msg)) => match parse_packet(msg) {
                    Ok(packet) => Poll::Ready(Some(Ok(packet))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                },
                Some(Err(e)) => Poll::Ready(Some(Err(WsError::Websocket(e)))),
                None => Poll::Ready(None),
            },
            WsTransportStateProj::Closed => Poll::Ready(None),
        }
    }
}

impl<S: WsSvc> Sink<Packet> for WsTransport<S> {
    type Error = WsError<S>;

    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().project().state.project() {
            WsTransportStateProj::Stream { stream } => {
                stream.poll_ready(cx).map_err(WsError::Websocket)
            }
            // drive the pending connection: the sink is ready once the
            // websocket is established.
            WsTransportStateProj::Connecting { fut } => match ready!(fut.poll(cx)) {
                Ok(stream) => {
                    self.project()
                        .state
                        .set(WsTransportState::Stream { stream });
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => {
                    self.project().state.set(WsTransportState::Closed);
                    Poll::Ready(Err(WsError::Websocket(e)))
                }
            },
            WsTransportStateProj::Closed => Poll::Ready(Err(WsError::Closed)),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.project().state.project() {
            WsTransportStateProj::Stream { stream } => {
                let msg = match item {
                    Packet::Binary(bin) => WsMessage::Binary(bin),
                    Packet::Noop => return Ok(()),
                    p => WsMessage::Text(String::from(p).into()),
                };
                stream.start_send(msg).map_err(WsError::Websocket)
            }
            WsTransportStateProj::Closed => Err(WsError::Closed),
            _ => {
                panic!("Sink is not ready")
            }
        }
    }

    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project().state.project() {
            WsTransportStateProj::Stream { stream } => {
                stream.poll_flush(cx).map_err(WsError::Websocket)
            }
            _ => Poll::Ready(Ok(())),
        }
    }

    #[tracing::instrument(level = Level::TRACE, skip_all, ret)]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().project().state.project() {
            WsTransportStateProj::Connecting { .. } => {
                // abort the in-flight connection attempt
                self.project().state.set(WsTransportState::Closed);
                Poll::Ready(Ok(()))
            }
            WsTransportStateProj::Stream { stream, .. } => {
                stream.poll_close(cx).map_err(WsError::Websocket)
            }
            WsTransportStateProj::Closed => Poll::Ready(Ok(())),
        }
    }
}

impl<S: WsSvc> fmt::Debug for WsTransport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WsTransport")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl<S: WsSvc> fmt::Debug for WsTransportState<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting { .. } => f.debug_struct("Connecting").finish_non_exhaustive(),
            Self::Stream { .. } => f.debug_struct("Stream").finish_non_exhaustive(),
            Self::Closed => f.write_str("Closed"),
        }
    }
}
