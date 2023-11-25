//! ## A Hyper v1 service for engine.io so it can be used with frameworks working with hyper v1
//!
//! This module is only enabled through the feature flag `hyper-v1`
//!
//! #### Example with a `hyper` v1 standalone service :
//! ```no_run
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::{Socket, DisconnectReason};
//! # use engineioxide::handler::EngineIoHandler;
//! # use std::net::SocketAddr;
//! # use std::sync::Arc;
//! # use tokio::net::TcpListener;
//! #[derive(Debug, Clone)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(&self, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(&self, msg: String, socket: Arc<Socket<()>>) { }
//!     fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<()>>) { }
//! }
//!
//! // Create a new engine.io service that will return a 404 not found response for other requests
//! let service = EngineIoService::new(MyHandler)
//!     .with_hyper_v1();    // It is required to enable the `hyper-v1` feature flag to use this
//!
//! let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//! let listener = TcpListener::bind(addr);
//!
//! // We start a loop to continuously accept incoming connections and spawn a hyper v1 service for each of them
//! // See the hyper v1 example for details
//! ```

use crate::{
    body::{
        request::IncomingBody,
        response::{Empty, ResponseBody},
    },
    handler::EngineIoHandler,
};
use bytes::Bytes;
use futures::future::{self, Ready};
use http::Request;
use hyper::Response;
use hyper_v1::body::Incoming;
use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use super::{futures::ResponseFuture, parser::dispatch_req, EngineIoService, NotFoundService};

/// A wrapper of [`EngineIoService`] that handles engine.io requests as a middleware for `hyper-v1`.
pub struct EngineIoHyperService<H: EngineIoHandler, S = NotFoundService>(EngineIoService<H, S>);
impl<H, S> EngineIoHyperService<H, S>
where
    H: EngineIoHandler,
{
    pub(crate) fn new(svc: EngineIoService<H, S>) -> Self {
        EngineIoHyperService(svc)
    }
}

/// Tower Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] response for `hyper-v1`
impl<ReqBody, ResBody, S, H> tower::Service<Request<ReqBody>> for EngineIoHyperService<H, S>
where
    ResBody: http_body_v1::Body + Send + 'static,
    ReqBody: http_body_v1::Body + Send + Unpin + 'static + std::fmt::Debug,
    ReqBody::Error: std::fmt::Debug,
    ReqBody::Data: Send,
    S: tower::Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let path = self.0.engine.config.req_path.as_ref();
        if req.uri().path().starts_with(path) {
            let req = req.map(IncomingBody::new);
            dispatch_req(
                req,
                self.0.engine.clone(),
                #[cfg(feature = "hyper-v1")]
                true, // hyper-v1 enabled
            )
        } else {
            ResponseFuture::new(self.0.inner.call(req))
        }
    }
}

/// Hyper 1.0 Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] response
impl<ResBody, S, H> hyper_v1::service::Service<Request<Incoming>> for EngineIoHyperService<H, S>
where
    ResBody: http_body_v1::Body + Send + 'static,
    S: hyper_v1::service::Service<Request<Incoming>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let path = self.0.engine.config.req_path.as_ref();
        if req.uri().path().starts_with(path) {
            let req = req.map(IncomingBody::new);
            dispatch_req(
                req,
                self.0.engine.clone(),
                true, // hyper-v1 enabled
            )
        } else {
            ResponseFuture::new(self.0.inner.call(req))
        }
    }
}

impl<H: EngineIoHandler, S: Clone> Clone for EngineIoHyperService<H, S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<H: EngineIoHandler, S> std::fmt::Debug for EngineIoHyperService<H, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EngineIoHyperService")
            .field(&self.0)
            .finish()
    }
}

/// Implement a custom [`hyper_v1::service::Service`] for the [`NotFoundService`]
impl hyper_v1::service::Service<Request<Incoming>> for NotFoundService {
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn call(&self, _: Request<Incoming>) -> Self::Future {
        future::ready(Ok(Response::builder()
            .status(404)
            .body(ResponseBody::empty_response())
            .unwrap()))
    }
}
