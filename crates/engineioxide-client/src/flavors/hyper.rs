//! A hyper-only flavor that only works with HTTP polling,
//! websocket is disabled with this implementation.
//!
//! The connection pool is shared across all instances of `HyperFlavor` in order to reuse connections.

use std::{convert::Infallible, future::Ready};

use bytes::Bytes;
use engineioxide_core::TransportType;
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::{
    Client, ResponseFuture,
    connect::{HttpConnector, dns::GaiResolver},
};
use tower_service::Service;

use crate::flavors::{Flavor, noop::NoopWebSocket};

static CONN_POOL: std::sync::OnceLock<
    Client<HttpConnector<GaiResolver>, BoxBody<Bytes, Infallible>>,
> = std::sync::OnceLock::new();

fn get_conn_pool() -> &'static Client<HttpConnector<GaiResolver>, BoxBody<Bytes, Infallible>> {
    CONN_POOL.get_or_init(|| Client::builder(hyper_util::rt::TokioExecutor::new()).build_http())
}

#[derive(Debug, Clone, Default)]
pub struct HyperFlavor;

impl HyperFlavor {
    pub fn new() -> Self {
        Self
    }
}

impl Flavor for HyperFlavor {
    const SUPPORTED_TRANSPORTS: &'static [TransportType] = &[TransportType::Polling];
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
        get_conn_pool().poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        get_conn_pool().request(req)
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
