//! ## A Tower [`Service`](tower::Service) and Hyper [`Service`](hyper::service::Service) for socket.io so it
//! can be used with frameworks supporting tower and hyper services.
//! #### Example with a raw `hyper` standalone service (most of the time it easier to use a framework like `axum` or `salvo`):
//! ```no_run
//! # use socketioxide::SocketIo;
//! let (svc, io) = SocketIo::new_svc();
//!
//! // Add io namespaces and events...
//!
//! // Spawn raw hyper server
//! let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//! let listener = TcpListener::bind(addr).await?;
//!
//! // We start a loop to continuously accept incoming connections
//! loop {
//!     let (stream, _) = listener.accept().await?;
//!
//!     // Use an adapter to access something implementing `tokio::io` traits as if they implement
//!     // `hyper::rt` IO traits.
//!     let io = hyper_util::rt::TokioIo::new(stream);
//!     let svc = svc.clone();
//!
//!     // Spawn a tokio task to serve multiple connections concurrently
//!     tokio::task::spawn(async move {
//!         // Finally, we bind the incoming connection to our `hello` service
//!         if let Err(err) = http1::Builder::new()
//!             .serve_connection(io, svc)
//!             .with_upgrades()
//!             .await
//!         {
//!             println!("Error serving connection: {:?}", err);
//!         }
//!     });
//! }
//! ```

use engineioxide::service::{EngineIoService, MakeEngineIoService};
use http::{Request, Response};
use http_body::Body;
use hyper::{body::Incoming, service::Service as HyperSvc};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service as TowerSvc;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    SocketIoConfig,
};

/// A [`Tower`](tower::Service)/[`Hyper`](hyper::service::Service) Service that wraps [`EngineIoService`] and
/// redirect every request to it
pub struct SocketIoService<S: Clone, A: Adapter = LocalAdapter> {
    engine_svc: EngineIoService<Arc<Client<A>>, S>,
}
impl<A: Adapter, ReqBody, ResBody, S> TowerSvc<Request<ReqBody>> for SocketIoService<S, A>
where
    ResBody: Body + Send + 'static,
    ReqBody: Body + Send + 'static + std::fmt::Debug + Unpin,
    <ReqBody as Body>::Error: std::fmt::Debug,
    <ReqBody as Body>::Data: Send,
    S: TowerSvc<Request<ReqBody>, Response = Response<ResBody>> + Clone,
{
    type Response = <EngineIoService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Response;
    type Error = <EngineIoService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Error;
    type Future = <EngineIoService<Arc<Client<A>>, S> as TowerSvc<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.engine_svc.poll_ready(cx)
    }
    #[inline(always)]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

/// Hyper 1.0 Service implementation with an [`Incoming`] body and a [`http_body::Body`] Body
impl<ResBody, S, A> HyperSvc<Request<Incoming>> for SocketIoService<S, A>
where
    ResBody: http_body::Body + Send + 'static,
    S: hyper::service::Service<Request<Incoming>, Response = Response<ResBody>>,
    S: Clone,
    A: Adapter,
{
    type Response = <EngineIoService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Response;
    type Error = <EngineIoService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Error;
    type Future = <EngineIoService<Arc<Client<A>>, S> as HyperSvc<Request<Incoming>>>::Future;

    #[inline(always)]
    fn call(&self, req: Request<Incoming>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

impl<A: Adapter, S: Clone> SocketIoService<S, A> {
    /// Creates a MakeService which can be used as a hyper service
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Arc<Client<A>>, S> {
        self.engine_svc.into_make_service()
    }

    /// Creates a new [`EngineIoService`] with a custom inner service and a custom config.
    pub(crate) fn with_config_inner(
        inner: S,
        config: Arc<SocketIoConfig>,
    ) -> (Self, Arc<Client<A>>) {
        let engine_config = config.engine_config.clone();
        let client = Arc::new(Client::new(config));
        let svc = EngineIoService::with_config_inner(inner, client.clone(), engine_config);
        (Self { engine_svc: svc }, client)
    }

    /// Creates a new [`EngineIoService`] with a custom inner service and an existing client
    /// It is mainly used with a [`SocketIoLayer`](crate::layer::SocketIoLayer) that owns the client
    pub(crate) fn with_client(inner: S, client: Arc<Client<A>>) -> Self {
        let engine_config = client.config.engine_config.clone();
        let svc = EngineIoService::with_config_inner(inner, client, engine_config);
        Self { engine_svc: svc }
    }
}

impl<A: Adapter, S: Clone> Clone for SocketIoService<S, A> {
    fn clone(&self) -> Self {
        Self {
            engine_svc: self.engine_svc.clone(),
        }
    }
}
