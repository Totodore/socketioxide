//! ## A tower [`Service`] for engine.io so it can be used with frameworks supporting tower services
//!
//! #### Example with a `Warp` inner service :
//! ```rust
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::{Socket, DisconnectReason};
//! # use std::sync::Arc;
//! # use warp::Filter;
//!
//! #[derive(Debug)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(&self, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(&self, msg: String, socket: Arc<Socket<()>>) { }
//!     fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<()>>) { }
//! }
//! let filter = warp::any().map(|| "Hello From Warp!");
//! let warp_svc = warp::service(filter);
//!
//! // Create a new engineio service by wrapping the warp service
//! let service = EngineIoService::with_inner(warp_svc, MyHandler)
//!     .into_make_service(); // Create a MakeService from the EngineIoService to give it to hyper
//! ```
//!
//! #### Example with a `hyper` standalone service :
//! ```rust
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::{Socket, DisconnectReason};
//! # use std::sync::Arc;
//! #[derive(Debug)]
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
//!     .into_make_service(); // Create a MakeService from the EngineIoService to give it to hyper
//! ```

use std::{
    convert::Infallible,
    sync::Arc,
    task::{Context, Poll},
};

use ::futures::future::{self, Ready};
use bytes::Bytes;
use http::{Request, Response};
use http_body::Body;
use tower::Service;

use crate::{
    body::response::{Empty, ResponseBody},
    config::EngineIoConfig,
    engine::EngineIo,
    handler::EngineIoHandler,
};

#[cfg(feature = "hyper-v1")]
pub mod hyper_v1;

mod futures;
mod parser;

pub use self::parser::{ProtocolVersion, TransportType};
use self::{futures::ResponseFuture, parser::dispatch_req};

/// A [`Service`] that handles engine.io requests as a middleware.
/// If the request is not an engine.io request, it forwards it to the inner service.
/// If it is an engine.io request it will forward it to the appropriate `transport`.
///
/// By default, it uses a [`NotFoundService`] as the inner service so it can be used as a standalone [`Service`].
pub struct EngineIoService<H: EngineIoHandler, S = NotFoundService> {
    inner: S,
    engine: Arc<EngineIo<H>>,
}

impl<H: EngineIoHandler> EngineIoService<H, NotFoundService> {
    /// Create a new [`EngineIoService`] with a [`NotFoundService`] as the inner service.
    /// If the request is not an `EngineIo` request, it will always return a 404 response.
    pub fn new(handler: H) -> Self {
        EngineIoService::with_config(handler, EngineIoConfig::default())
    }
    /// Create a new [`EngineIoService`] with a custom config
    pub fn with_config(handler: H, config: EngineIoConfig) -> Self {
        EngineIoService::with_config_inner(NotFoundService, handler, config)
    }
}

impl<S: Clone, H: EngineIoHandler> EngineIoService<H, S> {
    /// Create a new [`hyper_v1::EngineIoHyperService`] with this [`EngineIoService`] as the inner service.
    ///
    /// It can be used as a compatibility layer for hyper v1.
    #[cfg(feature = "hyper-v1")]
    #[inline(always)]
    pub fn with_hyper_v1(self) -> hyper_v1::EngineIoHyperService<H, S> {
        hyper_v1::EngineIoHyperService::new(self)
    }
    /// Create a new [`EngineIoService`] with a custom inner service.
    pub fn with_inner(inner: S, handler: H) -> Self {
        EngineIoService::with_config_inner(inner, handler, EngineIoConfig::default())
    }

    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(inner: S, handler: H, config: EngineIoConfig) -> Self {
        EngineIoService {
            inner,
            engine: Arc::new(EngineIo::new(handler, config)),
        }
    }

    /// Convert this [`EngineIoService`] into a [`MakeEngineIoService`].
    /// This is useful when using [`EngineIoService`] without layers.
    pub fn into_make_service(self) -> MakeEngineIoService<H, S> {
        MakeEngineIoService::new(self)
    }
}

impl<S: Clone, H: EngineIoHandler> Clone for EngineIoService<H, S> {
    fn clone(&self) -> Self {
        EngineIoService {
            inner: self.inner.clone(),
            engine: self.engine.clone(),
        }
    }
}
impl<H: EngineIoHandler, S> std::fmt::Debug for EngineIoService<H, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineIoService").finish()
    }
}

/// Tower [`Service`] implementation for [`EngineIoService`].
impl<ReqBody, ResBody, S, H> Service<Request<ReqBody>> for EngineIoService<H, S>
where
    ResBody: Body + Send + 'static,
    ReqBody: Body + Send + Unpin + 'static + std::fmt::Debug,
    ReqBody::Error: std::fmt::Debug,
    ReqBody::Data: Send,
    S: tower::Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with(&self.engine.config.req_path) {
            dispatch_req(
                req,
                self.engine.clone(),
                #[cfg(feature = "hyper-v1")]
                false, // hyper-v1 disabled
            )
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

/// A MakeService that always returns a clone of the [`EngineIoService`] it was created with.
pub struct MakeEngineIoService<H: EngineIoHandler, S> {
    svc: EngineIoService<H, S>,
}

impl<H: EngineIoHandler, S> MakeEngineIoService<H, S> {
    /// Create a new [`MakeEngineIoService`] with a custom inner service.
    pub fn new(svc: EngineIoService<H, S>) -> Self {
        MakeEngineIoService { svc }
    }
}

impl<H: EngineIoHandler, S: Clone, T> Service<T> for MakeEngineIoService<H, S> {
    type Response = EngineIoService<H, S>;

    type Error = Infallible;

    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: T) -> Self::Future {
        future::ready(Ok(self.svc.clone()))
    }
}

/// A [`Service`] that always returns a 404 response and that is compatible with [`EngineIoService`]
/// *and* [`hyper_v1::EngineIoHyperService`].
#[derive(Debug, Clone)]
pub struct NotFoundService;

impl<ReqBody> Service<Request<ReqBody>> for NotFoundService {
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request<ReqBody>) -> Self::Future {
        future::ready(Ok(Response::builder()
            .status(404)
            .body(ResponseBody::empty_response())
            .unwrap()))
    }
}
