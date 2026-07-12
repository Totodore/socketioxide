use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide_core::{OpenPacket, Packet, PacketParseError, ProtocolVersion, Sid, Str};
use futures_core::Stream;
use futures_util::{Sink, StreamExt};
use http::Request;
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;
use tokio_tungstenite::tungstenite::{self, Message, Utf8Bytes, handshake::client::generate_key};
use tracing::Level;

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
            upgrade: UpgradeHandshakeState,
        },
    }
}
#[derive(Debug)]
enum UpgradeHandshakeState {
    ShouldSendPingUpgrade,
    ShouldFlushPingUpgrade,
    WaitingPong,
    ShouldSendUpgrade,
    ShouldFlushUpgrade,
    Done,
}

pub enum WsTransportError<S: WsSvc> {
    Websocket(<S as WsSvc>::Error),
    Packet(PacketParseError),
    InvalidPacket {
        expected: Box<Packet>,
        got: Box<Packet>,
    },
    Closed,
}

impl<S: WsSvc> WsTransportError<S> {
    fn invalid_packet(expected: Packet, got: Packet) -> Self {
        Self::InvalidPacket {
            expected: Box::new(expected),
            got: Box::new(got),
        }
    }
}
impl<S: WsSvc> From<PacketParseError> for WsTransportError<S> {
    fn from(e: PacketParseError) -> Self {
        WsTransportError::Packet(e)
    }
}
impl<S: WsSvc> fmt::Debug for WsTransportError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}
impl<S: WsSvc> fmt::Display for WsTransportError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsTransportError::Websocket(e) => write!(f, "websocket error: {}", e),
            WsTransportError::Packet(e) => write!(f, "packet error: {}", e),
            WsTransportError::InvalidPacket { expected, got } => write!(
                f,
                "invalid packet received, expected {expected:?}, got {got:?}"
            ),
            WsTransportError::Closed => write!(f, "websocket closed"),
        }
    }
}
impl<S: WsSvc> std::error::Error for WsTransportError<S> {}

impl<S: WsSvc> WsTransport<S> {
    #[tracing::instrument(skip(svc))]
    pub fn connect_with_upgrade(svc: S, sid: Sid) -> Self {
        tracing::trace!("websocket connection with upgrade");
        let req = Request::builder()
            .method("GET")
            .header("Host", "127.0.0.1")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .uri(format!(
                "ws://127.0.0.1/engine.io/?EIO=4&transport=websocket&sid={sid}"
            ))
            .body(())
            .unwrap();

        let fut = svc.call(req);
        Self {
            svc,
            state: WsTransportState::Connecting { fut },
        }
    }

    #[tracing::instrument(skip(svc))]
    pub async fn connect(svc: S) -> Result<(Self, OpenPacket), WsTransportError<S>> {
        tracing::trace!("websocket connection without upgrade");

        let req = Request::builder()
            .method("GET")
            .header("Host", "127.0.0.1")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .uri("ws://127.0.0.1/engine.io/?EIO=4&transport=websocket")
            .body(())
            .unwrap();

        let mut stream = svc.call(req).await.map_err(WsTransportError::Websocket)?;
        tracing::debug!("handshake response received, waiting for open packet");

        let packet = match stream.next().await.ok_or(WsTransportError::Closed)? {
            Ok(msg) => parse_packet(msg)?,
            Err(e) => return Err(WsTransportError::Websocket(e)),
        };
        tracing::debug!("open packet received, switching to streaming");

        let ws = Self {
            svc,
            state: WsTransportState::Stream {
                stream,
                upgrade: UpgradeHandshakeState::Done,
            },
        };

        match packet {
            Packet::Open(open_packet) => Ok((ws, open_packet)),
            _ => Err(WsTransportError::Packet(
                PacketParseError::InvalidPacketType(None),
            )),
        }
    }
}

pub trait WsSvc:
    HyperSvc<http::Request<()>, Response = Self::WebSocket, Error = <Self as WsSvc>::Error> + Clone
{
    type Error: fmt::Debug + std::error::Error;
    type WebSocket: WebSocket<Error = <Self as WsSvc>::Error>;
}

impl<S, WS> WsSvc for S
where
    S: HyperSvc<http::Request<()>, Response = WS> + Clone,
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

fn parse_packet<S: WsSvc>(msg: WsMessage) -> Result<Packet, WsTransportError<S>> {
    match msg {
        WsMessage::Text(msg) => {
            let msg_str = unsafe { Str::from_bytes_unchecked(msg.into()) };
            let packet = Packet::parse(ProtocolVersion::V4, msg_str)?;
            Ok(packet)
        }
        WsMessage::Binary(data) => Ok(Packet::Binary(data)),
        WsMessage::Close => {
            todo!("impl ws close");
        }
    }
}

#[tracing::instrument(level = Level::TRACE, skip(cx, stream), ret)]
fn poll_upgrade<S: WsSvc>(
    cx: &mut Context<'_>,
    mut stream: Pin<&mut S::WebSocket>,
    curr: &mut UpgradeHandshakeState,
) -> Poll<Result<UpgradeHandshakeState, WsTransportError<S>>> {
    match curr {
        UpgradeHandshakeState::ShouldSendPingUpgrade => {
            ready!(stream.as_mut().poll_ready(cx)).map_err(WsTransportError::Websocket)?;
            stream
                .start_send(WsMessage::Text(Packet::PingUpgrade.into()))
                .map_err(WsTransportError::Websocket)?;
            Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushPingUpgrade))
        }
        UpgradeHandshakeState::ShouldFlushPingUpgrade => {
            ready!(stream.as_mut().poll_flush(cx)).map_err(WsTransportError::Websocket)?;
            Poll::Ready(Ok(UpgradeHandshakeState::WaitingPong))
        }
        UpgradeHandshakeState::WaitingPong => {
            match ready!(stream.as_mut().poll_next(cx)).map(|v| v.map(parse_packet::<S>)) {
                Some(Ok(Ok(Packet::PongUpgrade))) => {
                    Poll::Ready(Ok(UpgradeHandshakeState::ShouldSendUpgrade))
                }
                Some(Ok(Ok(p))) => Poll::Ready(Err(WsTransportError::invalid_packet(
                    Packet::PongUpgrade,
                    p,
                ))),
                Some(Ok(Err(parsing_err))) => Poll::Ready(Err(parsing_err)),
                Some(Err(err)) => Poll::Ready(Err(WsTransportError::Websocket(err))),
                None => Poll::Ready(Err(WsTransportError::Closed)),
            }
        }
        UpgradeHandshakeState::ShouldSendUpgrade => {
            ready!(stream.as_mut().poll_ready(cx)).map_err(WsTransportError::Websocket)?;
            stream
                .start_send(WsMessage::Text(Packet::Upgrade.into()))
                .map_err(WsTransportError::Websocket)?;
            Poll::Ready(Ok(UpgradeHandshakeState::ShouldFlushUpgrade))
        }
        UpgradeHandshakeState::ShouldFlushUpgrade => {
            ready!(stream.as_mut().poll_flush(cx)).map_err(WsTransportError::Websocket)?;
            Poll::Ready(Ok(UpgradeHandshakeState::Done))
        }
        UpgradeHandshakeState::Done => {
            unreachable!("poll_upgrade should never be called once upgrade as been performed")
        }
    }
}

impl<S: WsSvc> Stream for WsTransport<S> {
    type Item = Result<Packet, WsTransportError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project().state.project() {
            // if we were connecting it means that's an upgrade.
            // TODO: this assertion might be brittle, add a test to prove that.
            WsTransportStateProj::Connecting { fut } => {
                match ready!(fut.poll(cx)) {
                    Ok(stream) => {
                        self.project().state.set(WsTransportState::Stream {
                            stream,
                            upgrade: UpgradeHandshakeState::ShouldSendPingUpgrade,
                        });
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Err(_e) => Poll::Ready(None), // stop everything but find a way to report the error
                }
            }
            WsTransportStateProj::Stream {
                stream,
                upgrade: UpgradeHandshakeState::Done,
                ..
            } => match ready!(stream.poll_next(cx)) {
                Some(Ok(msg)) => match parse_packet(msg) {
                    Ok(packet) => Poll::Ready(Some(Ok(packet))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                },
                Some(Err(e)) => Poll::Ready(Some(Err(WsTransportError::Websocket(e)))),
                None => Poll::Ready(None),
            },
            WsTransportStateProj::Stream { stream, upgrade } => {
                match ready!(poll_upgrade(cx, stream, upgrade)) {
                    Ok(UpgradeHandshakeState::Done) => {
                        *upgrade = UpgradeHandshakeState::Done;
                        tracing::debug!("upgrade done, switching in nominal state");
                        cx.waker().wake_by_ref();
                        Poll::Ready(Some(Ok(Packet::Upgrade)))
                    }
                    Ok(next) => {
                        // switch to next upgrade state
                        *upgrade = next;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Err(err) => Poll::Ready(Some(Err(err))),
                }
            }
        }
    }
}

impl<S: WsSvc> Sink<Packet> for WsTransport<S> {
    type Error = WsTransportError<S>;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().project().state.project() {
            WsTransportStateProj::Stream {
                stream,
                upgrade: UpgradeHandshakeState::Done,
                ..
            } => stream.poll_ready(cx).map_err(WsTransportError::Websocket),
            _ => Poll::Pending,
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.project().state.project() {
            WsTransportStateProj::Stream {
                stream,
                upgrade: UpgradeHandshakeState::Done,
            } => {
                let msg = match item {
                    Packet::Binary(bin) => WsMessage::Binary(bin),
                    Packet::Noop => return Ok(()),
                    p => WsMessage::Text(String::from(p).into()),
                };
                stream.start_send(msg).map_err(WsTransportError::Websocket)
            }
            _ => {
                panic!("Sink is not ready")
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project().state.project() {
            WsTransportStateProj::Stream {
                stream,
                upgrade: UpgradeHandshakeState::Done,
            } => stream.poll_flush(cx).map_err(WsTransportError::Websocket),
            _ => Poll::Ready(Ok(())),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project().state.project() {
            WsTransportStateProj::Connecting { .. } => Poll::Ready(Ok(())),
            WsTransportStateProj::Stream { stream, .. } => {
                stream.poll_close(cx).map_err(WsTransportError::Websocket)
            }
        }
    }
}

pub mod noop_impl {
    use std::convert::Infallible;

    use super::*;

    #[derive(Debug, Default, Clone)]
    pub struct NoopWebSocket;
    impl HyperSvc<http::Request<()>> for NoopWebSocket {
        type Response = NoopWebSocket;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn call(&self, _: http::Request<()>) -> Self::Future {
            std::future::ready(Ok(NoopWebSocket))
        }
    }

    impl WebSocket for NoopWebSocket {
        type Error = Infallible;
    }

    impl Stream for NoopWebSocket {
        type Item = Result<WsMessage, Infallible>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(None)
        }
    }
    impl Sink<WsMessage> for NoopWebSocket {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _item: WsMessage) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}
pub mod tungstenite_impl {
    use super::*;

    impl From<WsMessage> for tungstenite::Message {
        fn from(value: WsMessage) -> Self {
            match value {
                WsMessage::Text(v) => {
                    tungstenite::Message::Text(unsafe { Utf8Bytes::from_bytes_unchecked(v.into()) })
                }
                WsMessage::Binary(bytes) => tungstenite::Message::Binary(bytes),
                WsMessage::Close => tungstenite::Message::Close(None),
            }
        }
    }
    pin_project! {
        pub struct TokioTungsteniteWS<S> {
            #[pin]
            inner: tokio_tungstenite::WebSocketStream<S>,
        }
    }

    impl<S> From<tokio_tungstenite::WebSocketStream<S>> for TokioTungsteniteWS<S>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        fn from(inner: tokio_tungstenite::WebSocketStream<S>) -> Self {
            Self { inner }
        }
    }

    impl<S> WebSocket for TokioTungsteniteWS<S>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        type Error = tungstenite::Error;
    }

    impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Sink<WsMessage>
        for TokioTungsteniteWS<S>
    {
        type Error = tungstenite::Error;

        fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.project().inner.poll_ready(cx)
        }

        fn start_send(self: Pin<&mut Self>, item: WsMessage) -> Result<(), Self::Error> {
            self.project().inner.start_send(item.into())
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.project().inner.poll_flush(cx)
        }

        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.project().inner.poll_close(cx)
        }
    }

    impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Stream for TokioTungsteniteWS<S> {
        type Item = Result<WsMessage, tungstenite::Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match ready!(self.project().inner.poll_next(cx)) {
                Some(Ok(Message::Text(v))) => Poll::Ready(Some(Ok(WsMessage::Text(unsafe {
                    Str::from_bytes_unchecked(v.into())
                })))),
                Some(Ok(Message::Binary(v))) => Poll::Ready(Some(Ok(WsMessage::Binary(v)))),
                Some(Ok(Message::Close(_))) => Poll::Ready(Some(Ok(WsMessage::Close))),
                Some(Ok(_)) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(Err(e)) => Poll::Ready(Some(Err(e))),
                None => Poll::Pending,
            }
        }
    }
}
