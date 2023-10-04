use engineioxide::service::{EngineIoService, MakeEngineIoService};
use http::{Request, Response};
use http_body::Body;
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

use crate::{adapter::Adapter, client::Client, ns::NsHandlers, SocketIoConfig};

/// The service for Socket.IO
///
/// It is a wrapper around the Engine.IO service.
/// Its main purpose is to be able to use it as standalone Socket.IO service
pub struct SocketIoService<A: Adapter, S: Clone> {
    engine_svc: EngineIoService<Arc<Client<A>>, S>,
}
impl<A: Adapter, ReqBody, ResBody, S> Service<Request<ReqBody>> for SocketIoService<A, S>
where
    ResBody: Body + Send + 'static,
    ReqBody: http_body::Body + Send + 'static + std::fmt::Debug + Unpin,
    <ReqBody as http_body::Body>::Error: std::fmt::Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
{
    type Response = <EngineIoService<Arc<Client<A>>, S> as Service<Request<ReqBody>>>::Response;
    type Error = <EngineIoService<Arc<Client<A>>, S> as Service<Request<ReqBody>>>::Error;
    type Future = <EngineIoService<Arc<Client<A>>, S> as Service<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.engine_svc.poll_ready(cx)
    }
    #[inline(always)]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

impl<A: Adapter, S: Clone> SocketIoService<A, S> {
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Arc<Client<A>>, S> {
        self.engine_svc.into_make_service()
    }

    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(
        inner: S,
        ns_handlers: NsHandlers<A>,
        config: Arc<SocketIoConfig>,
    ) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(config.clone(), ns_handlers.clone()));
        let svc =
            EngineIoService::with_config_inner(inner, client.clone(), config.engine_config.clone());
        (Self { engine_svc: svc }, client)
    }

    /// Create a new [`EngineIoService`] with a custom inner service and an existing client
    /// It is mainly used with a [`SocketIoLayer`](crate::layer::SocketIoLayer) that owns the client
    pub fn with_client(inner: S, client: Arc<Client<A>>) -> Self {
        let svc = EngineIoService::with_config_inner(
            inner,
            client.clone(),
            client.config.engine_config.clone(),
        );
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
