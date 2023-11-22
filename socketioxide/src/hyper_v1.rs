//! ## A Hyper v1 [`Service`](HyperSvc) for socket.io so it can be used with frameworks working with hyper v1
//!
//! This module is only enabled through the feature flag `hyper-v1`
//!
//! #### Example with a `hyper` v1 standalone service :
//! ```rust
//! # use socketioxide::SocketIo;
//!
//! // Create a new engine.io service that will return a 404 not found response for other requests
//! let (svc, io) = SocketIo::new_svc();
//!
//! // Add io namespaces and events...
//!
//! // Convert the `SocketIoService` so it works with hyper 1.0
//! let svc = svc.with_hyper_v1();
//!
//! // Create a hyper 1.0 server (see example)
//! ```
use std::{
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
};
use engineioxide::service::hyper_v1::EngineIoHyperService;
use http::{Request, Response};
use http_body_v1::Body;
use hyper_v1::body::Incoming;
use hyper_v1::service::Service as HyperSvc;
use tower::Service as TowerSvc;

/// [`Service`](tower::Service) implementation for `hyper 1.0`
/// It can be created with `with_hyper_v1` fn on [`SocketIoService`](crate::service::SocketIoService)
/// or [`SocketIoLayer`](crate::layer::SocketIoLayer)
pub struct SocketIoHyperService<S: Clone, A: Adapter = LocalAdapter>(
    EngineIoHyperService<Arc<Client<A>>, S>,
);

impl<A: Adapter, S: Clone> SocketIoHyperService<S, A> {
    pub(crate) fn new(svc: EngineIoHyperService<Arc<Client<A>>, S>) -> Self {
        Self(svc)
    }
}

/// Tower Service implementation with a [`http_body_v1::Body`] Body
impl<A: Adapter, ReqBody, ResBody, S> TowerSvc<Request<ReqBody>> for SocketIoHyperService<S, A>
where
    ResBody: Body + Send + 'static,
    ReqBody: Body + Send + 'static + std::fmt::Debug + Unpin,
    ReqBody::Error: std::fmt::Debug,
    ReqBody::Data: Send,
    S: TowerSvc<Request<ReqBody>, Response = Response<ResBody>> + Clone,
{
    type Response =
        <EngineIoHyperService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Response;
    type Error = <EngineIoHyperService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Error;
    type Future = <EngineIoHyperService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }
    #[inline(always)]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.0.call(req)
    }
}

/// Hyper 1.0 Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] Body
impl<ResBody, S, A> HyperSvc<Request<Incoming>> for SocketIoHyperService<S, A>
where
    ResBody: http_body_v1::Body + Send + 'static,
    S: hyper_v1::service::Service<Request<Incoming>, Response = Response<ResBody>>,
    S: Clone,
    A: Adapter,
{
    type Response =
        <EngineIoHyperService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Response;
    type Error = <EngineIoHyperService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Error;
    type Future = <EngineIoHyperService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Future;

    #[inline(always)]
    fn call(&self, req: Request<Incoming>) -> Self::Future {
        self.0.call(req)
    }
}

impl<A: Adapter, S: Clone> Clone for SocketIoHyperService<S, A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
