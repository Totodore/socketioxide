use std::{convert::Infallible, future::Ready};

use bytes::Bytes;
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::{
    Client, ResponseFuture,
    connect::{HttpConnector, dns::GaiResolver},
};
use tower_service::Service;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::Sink;

use crate::transport::ws::{WebSocket, WsMessage};

#[derive(Debug, Clone)]
pub struct HyperFlavor {
    client: Client<HttpConnector<GaiResolver>, BoxBody<Bytes, Infallible>>,
}

impl HyperFlavor {
    pub fn new() -> Self {
        Self {
            client: Client::builder(hyper_util::rt::TokioExecutor::new()).build_http(),
        }
    }
}
impl Default for HyperFlavor {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP Service implementation
impl Service<http::Request<BoxBody<Bytes, Infallible>>> for HyperFlavor {
    type Response = Response<Incoming>;
    type Error = hyper_util::client::legacy::Error;
    type Future = ResponseFuture;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.client.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        self.client.request(req)
    }
}

/// WS Service Implementation
impl Service<http::Request<()>> for HyperFlavor {
    type Response = NoopWebSocket;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: http::Request<()>) -> Self::Future {
        std::future::ready(Ok(NoopWebSocket))
    }
}

#[derive(Debug, Default, Clone)]
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

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, _item: WsMessage) -> Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
