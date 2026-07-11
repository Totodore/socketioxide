use std::{
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide::Str;
use futures_core::Stream;
use futures_util::Sink;
use pin_project_lite::pin_project;
use tokio_tungstenite::tungstenite::{self, Message, Utf8Bytes};

pub struct WsTransport<S> {}

pub trait SocketIoWebSocket:
    Stream<Item = Result<SocketIoWsMessage, Self::Error>> + Sink<SocketIoWsMessage>
{
    type Error;
}

pub enum SocketIoWsMessage {
    Text(Str),
    Binary(Bytes),
    Close,
}

impl From<SocketIoWsMessage> for tungstenite::Message {
    fn from(value: SocketIoWsMessage) -> Self {
        match value {
            SocketIoWsMessage::Text(v) => {
                tungstenite::Message::Text(unsafe { Utf8Bytes::from_bytes_unchecked(v.into()) })
            }
            SocketIoWsMessage::Binary(bytes) => tungstenite::Message::Binary(bytes.into()),
            SocketIoWsMessage::Close => tungstenite::Message::Close(None),
        }
    }
}
pin_project! {
    struct TokioTungsteniteWebSocket<S> {
        #[pin]
        inner: tokio_tungstenite::WebSocketStream<S>,
    }
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> SocketIoWebSocket
    for TokioTungsteniteWebSocket<S>
{
    type Error = tungstenite::Error;
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Sink
    for TokioTungsteniteWebSocket<S>
{
    type Error = tungstenite::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        self.project().inner.poll_ready(cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Stream
    for TokioTungsteniteWebSocket<S>
{
    type Item = Result<SocketIoWsMessage, tungstenite::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.project().inner.poll_next(cx)) {
            Some(Ok(Message::Text(v))) => Poll::Ready(Some(Ok(SocketIoWsMessage::Text(unsafe {
                Str::from_bytes_unchecked(v.into())
            })))),
            Some(Ok(Message::Binary(v))) => {
                Poll::Ready(Some(Ok(SocketIoWsMessage::Binary(v.into()))))
            }
            Some(Ok(Message::Close(_))) => Poll::Ready(Some(Ok(SocketIoWsMessage::Close))),
            Some(Ok(_)) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Pending,
        }
    }
}
