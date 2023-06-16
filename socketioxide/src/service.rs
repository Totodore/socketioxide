use engineioxide::service::{EngineIoService, MakeEngineIoService, NotFoundService};
use http::{Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower::Service;

use crate::{adapter::Adapter, client::Client, ns::NsHandlers, SocketIoConfig};

/// The service for Socket.IO
/// It is a wrapper around the Engine.IO service
/// Its main purpose is to be able to use it as standalone Socket.IO service
pub struct SocketIoService<A: Adapter, S: Clone> {
    engine_svc: EngineIoService<Client<A>, S>,
}
impl<A: Adapter, ReqBody, ResBody, S> Service<Request<ReqBody>> for SocketIoService<A, S>
where
    ResBody: Body + Send + 'static,
    ReqBody: http_body::Body + Send + 'static + std::fmt::Debug,
    <ReqBody as http_body::Body>::Error: std::fmt::Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
{
    type Response = <EngineIoService<Client<A>, S> as Service<Request<ReqBody>>>::Response;
    type Error = <EngineIoService<Client<A>, S> as Service<Request<ReqBody>>>::Error;
    type Future = <EngineIoService<Client<A>, S> as Service<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.engine_svc.poll_ready(cx)
    }
    #[inline(always)]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

impl<A: Adapter> SocketIoService<A, NotFoundService> {
    /// Create a new [`SocketIoService`] with a [`NotFoundService`] as the inner service.
    /// If the request is not an [`EngineIo`](crate::engine::EngineIo) request, it will always return a 404 response.
    pub fn new(ns_handlers: NsHandlers<A>) -> Self {
        SocketIoService::with_config(ns_handlers, SocketIoConfig::default())
    }

    /// Create a new [`SocketIoService`] with a custom config
    pub fn with_config(ns_handlers: NsHandlers<A>, config: SocketIoConfig) -> Self {
        let client = Client::new(config.clone(), ns_handlers.clone());
        let svc = EngineIoService::with_config(client.into(), config.engine_config);
        Self { engine_svc: svc }
    }
}

impl<A: Adapter, S: Clone> SocketIoService<A, S> {
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Client<A>, S> {
        self.engine_svc.into_make_service()
    }
    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(inner: S, ns_handlers: NsHandlers<A>, config: SocketIoConfig) -> Self {
        let client = Client::new(config.clone(), ns_handlers.clone());
        let svc = EngineIoService::with_config_inner(inner, client.into(), config.engine_config);
        Self { engine_svc: svc }
    }
}

impl<A: Adapter, S: Clone> Clone for SocketIoService<A, S> {
    fn clone(&self) -> Self {
        Self {
            engine_svc: self.engine_svc.clone(),
        }
    }
}