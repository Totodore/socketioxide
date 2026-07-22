use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use engineioxide_core::Str;
use futures_core::{Stream, future::BoxFuture};
use futures_util::{FutureExt, Sink};
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::ResponseFuture;
use pin_project_lite::pin_project;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream,
    tungstenite::{self, Message, Utf8Bytes},
};

use crate::{
    flavors::hyper::HyperFlavor,
    transport::ws::{WebSocket, WsMessage},
};

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

#[derive(Debug, Clone)]
pub struct HyperTungsteniteFlavor {
    hyper_svc: HyperFlavor,
}

impl HyperTungsteniteFlavor {
    pub fn new() -> Self {
        Self {
            hyper_svc: HyperFlavor::new(),
        }
    }
}
impl Default for HyperTungsteniteFlavor {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP Service implementation
impl hyper::service::Service<http::Request<BoxBody<Bytes, Infallible>>> for HyperTungsteniteFlavor {
    type Response = Response<Incoming>;
    type Error = hyper_util::client::legacy::Error;
    type Future = ResponseFuture;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        self.hyper_svc.call(req)
    }
}

/// WS Service Implementation
impl hyper::service::Service<http::Request<()>> for HyperTungsteniteFlavor {
    type Response = TokioTungsteniteWS<MaybeTlsStream<TcpStream>>;
    type Error = tungstenite::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: http::Request<()>) -> Self::Future {
        async move {
            let (ws, _) = tokio_tungstenite::connect_async(req).await?;
            Ok(ws.into())
        }
        .boxed()
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
