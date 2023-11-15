//! ## A tower [`Service`] for socket.io so it can be used with frameworks supporting tower services.
//!
//! #### Example with a `Warp` inner service :
//! ```rust
//! # use socketioxide::SocketIo;
//! # use warp::Filter;
//! let filter = warp::any().map(|| "Hello From Warp!");
//! let warp_svc = warp::service(filter);
//!
//! let (svc, io) = SocketIo::new_inner_svc(warp_svc);
//! let svc = svc.into_make_service();  // Create a Make Service for hyper
//! // Add io namespaces and events...
//!
//! // Spawn hyper server
//! ```
//!
//! #### Example with a `hyper` standalone service :
//! ```rust
//! # use socketioxide::SocketIo;
//! let (svc, io) = SocketIo::new_svc();
//!
//! // Add io namespaces and events...
//! let svc = svc.into_make_service(); // Create a Make Service for hyper
//!
//! // Spawn hyper server
//! ```

use engineioxide::service::{EngineIoService, MakeEngineIoService};
use http::{Request, Response};
use http_body::Body;
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    SocketIoConfig,
};

/// A [`Service`] that wraps [`EngineIoService`] and redirect every request to it
pub struct SocketIoService<S: Clone, A: Adapter = LocalAdapter> {
    engine_svc: EngineIoService<Arc<Client<A>>, S>,
}
impl<A: Adapter, ReqBody, ResBody, S> Service<Request<ReqBody>> for SocketIoService<S, A>
where
    ResBody: Body + Send + 'static,
    ReqBody: Body + Send + 'static + std::fmt::Debug + Unpin,
    <ReqBody as Body>::Error: std::fmt::Debug,
    <ReqBody as Body>::Data: Send,
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

impl<A: Adapter, S: Clone> SocketIoService<S, A> {
    /// Create a MakeService which can be used as a hyper service
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Arc<Client<A>>, S> {
        self.engine_svc.into_make_service()
    }

    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub(crate) fn with_config_inner(
        inner: S,
        config: Arc<SocketIoConfig>,
    ) -> (Self, Arc<Client<A>>) {
        let engine_config = config.engine_config.clone();
        let client = Arc::new(Client::new(config));
        let svc = EngineIoService::with_config_inner(inner, client.clone(), engine_config);
        (Self { engine_svc: svc }, client)
    }

    /// Create a new [`EngineIoService`] with a custom inner service and an existing client
    /// It is mainly used with a [`SocketIoLayer`](crate::layer::SocketIoLayer) that owns the client
    pub(crate) fn with_client(inner: S, client: Arc<Client<A>>) -> Self {
        let engine_config = client.config.engine_config.clone();
        let svc = EngineIoService::with_config_inner(inner, client, engine_config);
        Self { engine_svc: svc }
    }

    /// Convert this [`Service`] into a [`SocketIoHyperService`](crate::hyper_v1::SocketIoHyperService)
    /// to use with hyper v1 and its dependent frameworks.
    ///
    /// This is only available when the `hyper-v1` feature is enabled.
    #[inline(always)]
    #[cfg(feature = "hyper-v1")]
    pub fn with_hyper_v1(self) -> crate::hyper_v1::SocketIoHyperService<S, A> {
        crate::hyper_v1::SocketIoHyperService::new(self.engine_svc.with_hyper_v1())
    }
}

impl<A: Adapter, S: Clone> Clone for SocketIoService<S, A> {
    fn clone(&self) -> Self {
        Self {
            engine_svc: self.engine_svc.clone(),
        }
    }
}
