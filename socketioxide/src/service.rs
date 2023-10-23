use engineioxide::service::{EngineIoService, MakeEngineIoService};
use hyper::service::Service;
use hyper::{
    body::{Body, Incoming},
    Request, Response,
};
use std::sync::Arc;

use crate::{adapter::Adapter, client::Client, SocketIoConfig};

/// The service for Socket.IO
///
/// It is a wrapper around the Engine.IO service.
/// Its main purpose is to be able to use it as standalone Socket.IO service
pub struct SocketIoService<A: Adapter, S: Clone> {
    engine_svc: EngineIoService<Arc<Client<A>>, S>,
}
impl<A: Adapter, ResBody, S> Service<Request<Incoming>> for SocketIoService<A, S>
where
    ResBody: Body + Send + 'static,
    ResBody::Data: Send,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S: Service<Request<Incoming>, Response = Response<ResBody>> + Clone,
{
    type Response = <EngineIoService<Arc<Client<A>>, S> as Service<Request<Incoming>>>::Response;
    type Error = <EngineIoService<Arc<Client<A>>, S> as Service<Request<Incoming>>>::Error;
    type Future = <EngineIoService<Arc<Client<A>>, S> as Service<Request<Incoming>>>::Future;

    #[inline(always)]
    fn call(&self, req: Request<Incoming>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

impl<A: Adapter, ResBody, S> tower::Service<Request<Incoming>> for SocketIoService<A, S>
where
    ResBody: Body + Send + 'static,
    ResBody::Data: Send,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S: tower::Service<Request<Incoming>, Response = Response<ResBody>> + Clone,
{
    type Response =
        <EngineIoService<Arc<Client<A>>, S> as tower::Service<Request<Incoming>>>::Response;
    type Error = <EngineIoService<Arc<Client<A>>, S> as tower::Service<Request<Incoming>>>::Error;
    type Future = <EngineIoService<Arc<Client<A>>, S> as tower::Service<Request<Incoming>>>::Future;

    #[inline(always)]
    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        self.engine_svc.call(req)
    }

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.engine_svc.poll_ready(cx)
    }
}

impl<A: Adapter, S: Clone> SocketIoService<A, S> {
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Arc<Client<A>>, S> {
        self.engine_svc.into_make_service()
    }

    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(inner: S, config: Arc<SocketIoConfig>) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(config.clone()));
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
