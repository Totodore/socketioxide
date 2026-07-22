use std::{convert::Infallible, future::Ready};

use bytes::Bytes;
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::{
    Client, ResponseFuture,
    connect::{HttpConnector, dns::GaiResolver},
};

use crate::flavors::noop_impl::NoopWebSocket;

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
impl hyper::service::Service<http::Request<BoxBody<Bytes, Infallible>>> for HyperFlavor {
    type Response = Response<Incoming>;
    type Error = hyper_util::client::legacy::Error;
    type Future = ResponseFuture;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        self.client.request(req)
    }
}

/// WS Service Implementation
impl hyper::service::Service<http::Request<()>> for HyperFlavor {
    type Response = NoopWebSocket;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn call(&self, _: http::Request<()>) -> Self::Future {
        std::future::ready(Ok(NoopWebSocket))
    }
}
