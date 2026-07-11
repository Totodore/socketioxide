use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide_core::{OpenPacket, Packet, PacketParseError, ProtocolVersion, Str};
use futures_core::Stream;
use futures_util::{Sink, StreamExt};
use pin_project_lite::pin_project;
use tokio_tungstenite::tungstenite::{self, Message, Utf8Bytes};

pin_project! {
    pub struct WsTransport<S> {
        #[pin]
        inner: S,
    }
}

pub enum WsTransportError<WS: WebSocket> {
    Websocket(<WS as WebSocket>::Error),
    Packet(PacketParseError),
    Closed,
}
impl<WS: WebSocket> From<PacketParseError> for WsTransportError<WS> {
    fn from(e: PacketParseError) -> Self {
        WsTransportError::Packet(e)
    }
}
impl<WS: WebSocket> fmt::Debug for WsTransportError<WS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}
impl<WS: WebSocket> fmt::Display for WsTransportError<WS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsTransportError::Websocket(e) => write!(f, "websocket error: {}", e),
            WsTransportError::Packet(e) => write!(f, "packet error: {}", e),
            WsTransportError::Closed => write!(f, "websocket closed"),
        }
    }
}
impl<WS: WebSocket> std::error::Error for WsTransportError<WS> {}

impl<WS: WebSocket> WsTransport<WS> {
    pub fn new(inner: impl Into<WS>) -> Self {
        Self {
            inner: inner.into(),
        }
    }

    pub async fn connect(inner: impl Into<WS>) -> Result<(Self, OpenPacket), WsTransportError<WS>> {
        tracing::trace!("handshake request");
        let mut ws = Self::new(inner);

        match ws.next().await.ok_or(WsTransportError::Closed)?? {
            Packet::Open(open_packet) => Ok((ws, open_packet)),
            _ => Err(WsTransportError::Packet(
                PacketParseError::InvalidPacketType(None),
            )),
        }
    }
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

impl<WS: WebSocket> WsTransport<WS> {
    fn parse_packet(&self, msg: WsMessage) -> Result<Packet, WsTransportError<WS>> {
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
}

impl<WS: WebSocket> Stream for WsTransport<WS> {
    type Item = Result<Packet, WsTransportError<WS>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().project().inner.poll_next(cx)) {
            Some(Ok(msg)) => match self.parse_packet(msg) {
                Ok(packet) => Poll::Ready(Some(Ok(packet))),
                Err(e) => Poll::Ready(Some(Err(e))),
            },
            Some(Err(e)) => Poll::Ready(Some(Err(WsTransportError::Websocket(e)))),
            None => Poll::Ready(None),
        }
    }
}

impl<WS: WebSocket> Sink<Packet> for WsTransport<WS> {
    type Error = WsTransportError<WS>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_ready(cx)
            .map_err(WsTransportError::Websocket)
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        let msg = match item {
            Packet::Binary(bin) => WsMessage::Binary(bin),
            Packet::Noop => return Ok(()),
            p => WsMessage::Text(String::from(p).into()),
        };
        self.project()
            .inner
            .start_send(msg)
            .map_err(WsTransportError::Websocket)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_flush(cx)
            .map_err(WsTransportError::Websocket)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_close(cx)
            .map_err(WsTransportError::Websocket)
    }
}

pub mod noop_impl {
    use std::convert::Infallible;

    use super::*;

    #[derive(Debug, Default)]
    pub struct NoopWebSocket;

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
        pub struct TokioTungsteniteWebSocket<S> {
            #[pin]
            inner: tokio_tungstenite::WebSocketStream<S>,
        }
    }

    impl<S> From<tokio_tungstenite::WebSocketStream<S>> for TokioTungsteniteWebSocket<S>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        fn from(inner: tokio_tungstenite::WebSocketStream<S>) -> Self {
            Self { inner }
        }
    }

    impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> WebSocket
        for TokioTungsteniteWebSocket<S>
    {
        type Error = tungstenite::Error;
    }

    impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Sink<WsMessage>
        for TokioTungsteniteWebSocket<S>
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

    impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Stream
        for TokioTungsteniteWebSocket<S>
    {
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
